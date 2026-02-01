use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, anyhow};
use chrono::{DateTime, Utc};
use clap::Parser;
use tokio::sync::watch;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use eth_btc_strategy::backtest::{
    BacktestEngine, export_equity_csv, export_metrics_json, export_trades_csv,
    load_backtest_bars,
};
use eth_btc_strategy::cli::{Cli, Command};
use eth_btc_strategy::config::load_config;
use eth_btc_strategy::core::strategy::StrategyEngine;
use eth_btc_strategy::data::{HyperliquidPriceSource, PriceFetcher};
use eth_btc_strategy::backtest::download::HyperliquidDownloader;
use eth_btc_strategy::execution::{
    ExecutionEngine, LiveOrderExecutor, PaperOrderExecutor, RetryConfig,
};
use eth_btc_strategy::funding::{FundingFetcher, HyperliquidFundingSource};
use eth_btc_strategy::logging::{BarLogFileWriter, TradeLogFileWriter};
use eth_btc_strategy::runtime::{LiveRunner, StateStoreWriter, StateWriter};
use eth_btc_strategy::state::{StateStore, recover_state};

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
                let bars = load_backtest_bars(&args.bars).context("load backtest bars")?;
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
                    let payload = serde_json::to_string_pretty(&result.metrics)
                        .context("format metrics")?;
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
                let payload =
                    serde_json::to_string_pretty(&bars).context("serialize backtest bars")?;
                std::fs::write(&args.output, payload).context("write output file")?;
                info!(count = bars.len(), path = %args.output.display(), "download complete");
                return Ok(());
            }
        }
    }

    let price_source = HyperliquidPriceSource::new(base_url.clone());
    let price_fetcher = PriceFetcher::new(Arc::new(price_source), config.data.price_field);

    let funding_fetcher = if disable_funding {
        None
    } else {
        let source = HyperliquidFundingSource::new(base_url.clone());
        Some(FundingFetcher::new(Arc::new(source)))
    };

    let execution = if paper {
        ExecutionEngine::new(Arc::new(PaperOrderExecutor), RetryConfig::fast())
    } else {
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
        ExecutionEngine::new(Arc::new(executor), RetryConfig::fast())
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
    if let Some(writer) = state_writer {
        runner = runner.with_state_writer(writer);
    }
    if let Some(path) = config.logging.stats_path.as_ref() {
        let format = config
            .logging
            .stats_format
            .unwrap_or(config.logging.format);
        let writer = BarLogFileWriter::new(PathBuf::from(path), format)
            .context("create stats logger")?;
        runner = runner.with_stats_writer(Arc::new(writer));
    }
    if let Some(path) = config.logging.trade_path.as_ref() {
        let format = config
            .logging
            .trade_format
            .unwrap_or(config.logging.format);
        let writer = TradeLogFileWriter::new(PathBuf::from(path), format)
            .context("create trade logger")?;
        runner = runner.with_trade_writer(Arc::new(writer));
    }

    if run_once {
        runner.run_once().await.context("run once")?;
        return Ok(());
    }

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
