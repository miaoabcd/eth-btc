use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, anyhow};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use clap::Parser;
use rust_decimal::{Decimal, RoundingStrategy};
use tokio::sync::watch;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use alloy_signer_local::PrivateKeySigner;
use eth_btc_strategy::account::{
    AccountBalanceSource, AccountPositionSource, HyperliquidAccountSource,
};
use eth_btc_strategy::backtest::download::{HyperliquidDownloader, write_bars_to_output};
use eth_btc_strategy::backtest::{
    BacktestEngine, export_equity_csv, export_metrics_json, export_trades_csv, load_backtest_bars,
    load_backtest_bars_from_db,
};
use eth_btc_strategy::cli::{Cli, Command};
use eth_btc_strategy::config::{CapitalMode, load_config};
use eth_btc_strategy::core::strategy::StrategyEngine;
use eth_btc_strategy::data::{
    HyperliquidPriceSource, PriceFetcher, PriceSource, align_to_bar_close,
};
use eth_btc_strategy::execution::{
    ExecutionEngine, LiveOrderExecutor, OrderExecutor, OrderRequest, OrderSide, PaperOrderExecutor,
    RetryConfig,
};
use eth_btc_strategy::funding::{FundingFetcher, HyperliquidFundingSource};
use eth_btc_strategy::logging::{BarLogFileWriter, TradeLogFileWriter};
use eth_btc_strategy::runtime::backfill::{
    ensure_price_history, latest_completed_bar, replay_warmup_gap_window,
};
use eth_btc_strategy::runtime::{LiveRunner, StateStoreWriter, StateWriter};
use eth_btc_strategy::state::{StateStore, recover_state};
use eth_btc_strategy::storage::{PriceStore, PriceStoreWriter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config = load_config(cli.config.as_deref()).context("load config")?;

    let runtime = &config.runtime;
    let base_url = cli
        .base_url
        .clone()
        .unwrap_or_else(|| runtime.base_url.clone());
    let interval_secs = cli.interval_secs.unwrap_or(runtime.interval_secs);
    let run_once = if cli.once { true } else { runtime.once };
    let paper = if cli.paper { true } else { runtime.paper };
    let disable_funding = if cli.disable_funding {
        true
    } else {
        runtime.disable_funding
    };
    let state_path = cli
        .state_path
        .clone()
        .or_else(|| runtime.state_path.clone().map(PathBuf::from));

    if let Some(command) = &cli.command {
        match command {
            Command::Backtest(args) => {
                let bars = if let Some(db) = args.db.as_ref() {
                    let start = args
                        .start
                        .as_ref()
                        .ok_or_else(|| anyhow!("--start required when --db is set"))?;
                    let end = args
                        .end
                        .as_ref()
                        .ok_or_else(|| anyhow!("--end required when --db is set"))?;
                    let start = parse_rfc3339(start).context("parse --start")?;
                    let end = parse_rfc3339(end).context("parse --end")?;
                    load_backtest_bars_from_db(db, start, end, config.data.price_field)
                        .context("load backtest bars from db")?
                } else if let Some(bars_path) = args.bars.as_ref() {
                    load_backtest_bars(bars_path).context("load backtest bars")?
                } else {
                    return Err(anyhow!("--bars or --db is required for backtest"));
                };
                let engine = BacktestEngine::new(config);
                let result = engine.run(&bars).context("run backtest")?;
                if let Some(dir) = args.output_dir.as_ref() {
                    std::fs::create_dir_all(dir).context("create output dir")?;
                    export_metrics_json(&dir.join("metrics.json"), &result.metrics)
                        .context("write metrics")?;
                    export_trades_csv(&dir.join("trades.csv"), &result.trades)
                        .context("write trades")?;
                    export_equity_csv(&dir.join("equity.csv"), &result.equity_curve)
                        .context("write equity")?;
                } else {
                    let payload =
                        serde_json::to_string_pretty(&result.metrics).context("format metrics")?;
                    println!("{payload}");
                }
                return Ok(());
            }
            Command::Download(args) => {
                let start = parse_rfc3339(&args.start).context("parse --start")?;
                let end = parse_rfc3339(&args.end).context("parse --end")?;
                let downloader = HyperliquidDownloader::new(base_url.clone());
                let bars = downloader
                    .fetch_backtest_bars(start, end)
                    .await
                    .context("download bars")?;
                write_bars_to_output(&bars, &args.output).context("write output")?;
                info!(count = bars.len(), path = %args.output.display(), "download complete");
                return Ok(());
            }
            Command::OrderTest(args) => {
                if args.dry_run {
                    let payload = serde_json::json!({
                        "symbol": args.symbol,
                        "side": args.side,
                        "qty": args.qty,
                        "limit_price": args.limit_price,
                        "reduce_only": args.reduce_only,
                        "base_url": base_url,
                    });
                    let pretty =
                        serde_json::to_string_pretty(&payload).context("format dry-run")?;
                    println!("{pretty}");
                    return Ok(());
                }
                if paper {
                    return Err(anyhow!("order-test does not support paper mode"));
                }
                let private_key = cli
                    .private_key
                    .or(cli.api_key)
                    .or_else(|| config.auth.private_key.clone());
                let vault_address = cli
                    .vault_address
                    .or_else(|| config.auth.vault_address.clone());
                let key = private_key.ok_or_else(|| anyhow!("missing Hyperliquid private key"))?;
                let mut executor = LiveOrderExecutor::with_private_key(base_url.clone(), key);
                if let Some(vault) = vault_address {
                    executor = executor.with_vault_address(vault);
                }
                if let Some(leverage) = config.execution.leverage {
                    executor = executor
                        .with_leverage_config(leverage, config.execution.margin_mode.is_cross());
                }
                let order = OrderRequest {
                    symbol: args.symbol,
                    side: args.side,
                    qty: args.qty,
                    order_type: config.execution.order_type,
                    limit_price: Some(args.limit_price),
                };
                let filled = if args.reduce_only {
                    executor.close(&order).await.context("close test order")?
                } else {
                    executor.submit(&order).await.context("submit test order")?
                };
                info!(filled = %filled, "order-test complete");
                println!("{filled}");
                return Ok(());
            }
            Command::MarketTest(args) => {
                if paper {
                    return Err(anyhow!("market-test does not support paper mode"));
                }
                if args.dry_run {
                    let payload = serde_json::json!({
                        "symbol": args.symbol,
                        "side": args.side,
                        "qty": args.qty,
                        "slippage_bps": args.slippage_bps,
                        "price_field": config.data.price_field,
                        "base_url": base_url,
                        "note": "dry-run does not fetch market price or submit orders"
                    });
                    let pretty =
                        serde_json::to_string_pretty(&payload).context("format dry-run")?;
                    println!("{pretty}");
                    return Ok(());
                }
                let now = Utc::now();
                let bar_time = align_to_bar_close(now).context("align market-test timestamp")?;
                let price_source = HyperliquidPriceSource::new(base_url.clone());
                let bar = price_source
                    .fetch_bar(args.symbol, bar_time)
                    .await
                    .context("fetch market-test price bar")?;
                let ref_price = bar
                    .effective_price(config.data.price_field)
                    .ok_or_else(|| anyhow!("market-test missing effective price"))?;
                let open_limit = ioc_limit_from_ref_price(ref_price, args.side, args.slippage_bps)
                    .context("compute open limit price")?;
                let close_side = match args.side {
                    OrderSide::Buy => OrderSide::Sell,
                    OrderSide::Sell => OrderSide::Buy,
                };
                let close_limit =
                    ioc_limit_from_ref_price(ref_price, close_side, args.slippage_bps)
                        .context("compute close limit price")?;
                let private_key = cli
                    .private_key
                    .or(cli.api_key)
                    .or_else(|| config.auth.private_key.clone());
                let vault_address = cli
                    .vault_address
                    .or_else(|| config.auth.vault_address.clone());
                let key = private_key.ok_or_else(|| anyhow!("missing Hyperliquid private key"))?;
                let mut executor = LiveOrderExecutor::with_private_key(base_url.clone(), key);
                if let Some(vault) = vault_address {
                    executor = executor.with_vault_address(vault);
                }
                if let Some(leverage) = config.execution.leverage {
                    executor = executor
                        .with_leverage_config(leverage, config.execution.margin_mode.is_cross());
                }

                let open_order = OrderRequest {
                    symbol: args.symbol,
                    side: args.side,
                    qty: args.qty,
                    order_type: config.execution.order_type,
                    limit_price: Some(open_limit),
                };
                let filled = executor
                    .submit(&open_order)
                    .await
                    .context("submit market-test open order")?;
                if filled <= Decimal::ZERO {
                    return Err(anyhow!("market-test open order filled zero"));
                }

                let close_order = OrderRequest {
                    symbol: args.symbol,
                    side: close_side,
                    qty: filled.abs(),
                    order_type: config.execution.order_type,
                    limit_price: Some(close_limit),
                };
                let closed = executor
                    .close(&close_order)
                    .await
                    .context("submit market-test close order")?;
                info!(
                    open_filled = %filled,
                    close_filled = %closed,
                    "market-test complete"
                );
                println!(
                    "{}",
                    serde_json::json!({
                        "open_filled": filled,
                        "close_filled": closed,
                        "ref_price": ref_price,
                        "open_limit_price": open_limit,
                        "close_limit_price": close_limit
                    })
                );
                return Ok(());
            }
        }
    }

    let price_source = HyperliquidPriceSource::new(base_url.clone());
    let price_fetcher = PriceFetcher::new(Arc::new(price_source.clone()), config.data.price_field);

    let funding_fetcher = if disable_funding {
        None
    } else {
        let source = HyperliquidFundingSource::new(base_url.clone());
        Some(FundingFetcher::new(Arc::new(source)))
    };

    let (execution, account_source, position_source) = if paper {
        (
            ExecutionEngine::new(Arc::new(PaperOrderExecutor), RetryConfig::fast()),
            None,
            None,
        )
    } else {
        let private_key = cli
            .private_key
            .or(cli.api_key)
            .or_else(|| config.auth.private_key.clone());
        let wallet_address = cli
            .wallet_address
            .or_else(|| config.auth.wallet_address.clone());
        let vault_address = cli
            .vault_address
            .or_else(|| config.auth.vault_address.clone());
        let key = private_key.ok_or_else(|| anyhow!("missing Hyperliquid private key"))?;
        let signer = PrivateKeySigner::from_str(key.trim_start_matches("0x"))
            .map_err(|err| anyhow!("invalid private key: {err}"))?;
        let signer_wallet = signer.address().to_string();
        let account_wallet = wallet_address
            .clone()
            .or_else(|| vault_address.clone())
            .unwrap_or_else(|| signer_wallet.clone());
        let execution_wallet = vault_address
            .clone()
            .unwrap_or_else(|| signer_wallet.clone());
        info!(
            signer_wallet = %signer_wallet,
            wallet_address = ?wallet_address,
            vault_address = ?vault_address,
            account_wallet = %account_wallet,
            execution_wallet = %execution_wallet,
            "resolved live trading wallet"
        );
        let live_account_source = Arc::new(HyperliquidAccountSource::new(
            base_url.clone(),
            account_wallet.clone(),
        ));
        let account_source: Option<Arc<dyn AccountBalanceSource>> =
            if matches!(config.position.c_mode, CapitalMode::EquityRatio) {
                Some(live_account_source.clone())
            } else {
                None
            };
        let position_source: Option<Arc<dyn AccountPositionSource>> =
            Some(live_account_source.clone());
        let mut executor = LiveOrderExecutor::with_private_key(base_url.clone(), key);
        if let Some(vault) = vault_address {
            executor = executor.with_vault_address(vault);
        }
        if let Some(leverage) = config.execution.leverage {
            executor =
                executor.with_leverage_config(leverage, config.execution.margin_mode.is_cross());
        }
        (
            ExecutionEngine::new(Arc::new(executor), RetryConfig::fast()),
            account_source,
            position_source,
        )
    };
    let mut engine = StrategyEngine::new(config.clone(), execution).context("create engine")?;

    let mut state_writer: Option<Arc<dyn StateWriter>> = None;
    if let Some(path) = state_path.as_ref() {
        let store = StateStore::new(path.to_string_lossy().as_ref()).context("open state store")?;
        if let Some(state) = store.load().context("load state")? {
            let report = recover_state(state, chrono::Utc::now());
            if !report.alerts.is_empty() {
                warn!(alerts = ?report.alerts, "state recovery alerts");
            }
            engine
                .apply_state(report.state)
                .context("apply recovered state")?;
        }
        state_writer = Some(Arc::new(StateStoreWriter::new(store)));
    }

    let mut runner = LiveRunner::new(engine, price_fetcher, funding_fetcher);
    if let Some(source) = account_source {
        runner = runner.with_account_source(source);
    }
    if let Some(source) = position_source {
        runner = runner.with_position_source(source);
    }
    if let Some(writer) = state_writer {
        runner = runner.with_state_writer(writer);
    }
    if let Some(path) = config.logging.stats_path.as_ref() {
        let format = config.logging.stats_format.unwrap_or(config.logging.format);
        let writer =
            BarLogFileWriter::new(PathBuf::from(path), format).context("create stats logger")?;
        runner = runner.with_stats_writer(Arc::new(writer));
    }
    if let Some(path) = config.logging.trade_path.as_ref() {
        let format = config.logging.trade_format.unwrap_or(config.logging.format);
        let writer =
            TradeLogFileWriter::new(PathBuf::from(path), format).context("create trade logger")?;
        runner = runner.with_trade_writer(Arc::new(writer));
    }
    if let Some(path) = config.logging.price_db_path.as_ref() {
        let store = PriceStore::new(path).context("open price db")?;
        let writer = PriceStoreWriter::new(store);
        runner = runner.with_price_writer(Arc::new(writer));
    }

    let mut first_run_at = align_to_bar_close(Utc::now()).context("align first run")?;
    if let Some(db_path) = config.logging.price_db_path.as_ref() {
        let warmup_bars = config.strategy.n_z.max(config.position.n_vol).max(384);
        ensure_price_history(
            &price_source,
            db_path,
            config.data.price_field,
            warmup_bars,
            first_run_at,
        )
        .await
        .context("backfill price history")?;
        let end = latest_completed_bar(first_run_at).context("align warmup end")?;
        let span_secs = 900 * (warmup_bars.saturating_sub(1)) as i64;
        let start = end - ChronoDuration::seconds(span_secs);
        let store = PriceStore::new(db_path).context("open price db for warmup")?;
        let records = store
            .load_range(start, end)
            .context("load warmup records")?;
        runner
            .engine_mut()
            .warm_up_with_records(&records)
            .context("warm up pipeline")?;

        let latest_run_at = align_to_bar_close(Utc::now()).context("align latest first run")?;
        if latest_run_at > first_run_at {
            ensure_price_history(
                &price_source,
                db_path,
                config.data.price_field,
                warmup_bars,
                latest_run_at,
            )
            .await
            .context("backfill price history catchup")?;
            if let Some((gap_start, gap_end)) =
                replay_warmup_gap_window(first_run_at, latest_run_at)
            {
                let catchup = store
                    .load_range(gap_start, gap_end)
                    .context("load warmup catchup records")?;
                runner
                    .engine_mut()
                    .warm_up_with_records(&catchup)
                    .context("warm up pipeline catchup")?;
            }
            first_run_at = latest_run_at;
        }
    }

    if run_once {
        runner.run_once_at(first_run_at).await.context("run once")?;
        return Ok(());
    }

    let _ = runner
        .run_once_at(first_run_at)
        .await
        .context("run initial bar")?;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let shutdown_handle = tokio::spawn(async move {
        if let Err(err) = tokio::signal::ctrl_c().await {
            warn!(error = ?err, "failed to listen for ctrl-c");
        }
        let _ = shutdown_tx.send(true);
    });

    info!(interval_secs, "starting live loop");
    runner
        .run_loop(Duration::from_secs(interval_secs), shutdown_rx)
        .await
        .context("run loop")?;

    let _ = shutdown_handle.await;
    Ok(())
}

fn parse_rfc3339(value: &str) -> anyhow::Result<DateTime<Utc>> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("invalid RFC3339 timestamp: {value}"))?;
    Ok(parsed.with_timezone(&Utc))
}

fn ioc_limit_from_ref_price(
    ref_price: Decimal,
    side: OrderSide,
    slippage_bps: u32,
) -> anyhow::Result<Decimal> {
    if ref_price <= Decimal::ZERO {
        return Err(anyhow!("reference price must be > 0"));
    }
    let bps = Decimal::from(slippage_bps);
    let scale = Decimal::from(10_000u32);
    let multiplier = match side {
        OrderSide::Buy => Decimal::ONE + bps / scale,
        OrderSide::Sell => Decimal::ONE - bps / scale,
    };
    if multiplier <= Decimal::ZERO {
        return Err(anyhow!("invalid slippage bps: {slippage_bps}"));
    }
    let raw = ref_price * multiplier;
    let int_digits = if raw.abs() < Decimal::ONE {
        0usize
    } else {
        raw.abs()
            .trunc()
            .to_string()
            .chars()
            .filter(|ch| ch.is_ascii_digit())
            .count()
    };
    let decimals = if int_digits >= 5 {
        0u32
    } else {
        (5usize - int_digits) as u32
    }
    .min(6);
    let strategy = match side {
        OrderSide::Buy => RoundingStrategy::ToPositiveInfinity,
        OrderSide::Sell => RoundingStrategy::ToNegativeInfinity,
    };
    let rounded = raw.round_dp_with_strategy(decimals, strategy);
    if rounded <= Decimal::ZERO {
        return Err(anyhow!("computed limit price is not positive"));
    }
    Ok(rounded.normalize())
}
