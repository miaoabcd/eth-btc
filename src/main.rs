use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, anyhow};
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
use eth_btc_strategy::execution::{
    ExecutionEngine, LiveOrderExecutor, PaperOrderExecutor, RetryConfig,
};
use eth_btc_strategy::funding::{FundingFetcher, HyperliquidFundingSource};
use eth_btc_strategy::runtime::{LiveRunner, StateStoreWriter, StateWriter};
use eth_btc_strategy::state::{StateStore, recover_state};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config = load_config(cli.config.as_deref()).context("load config")?;

    if let Some(Command::Backtest(args)) = cli.command {
        let bars = load_backtest_bars(&args.bars).context("load backtest bars")?;
        let engine = BacktestEngine::new(config);
        let result = engine.run(&bars).context("run backtest")?;
        if let Some(dir) = args.output_dir {
            std::fs::create_dir_all(&dir).context("create output dir")?;
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

    let price_source = HyperliquidPriceSource::new(cli.base_url.clone());
    let price_fetcher = PriceFetcher::new(Arc::new(price_source), config.data.price_field);

    let funding_fetcher = if cli.disable_funding {
        None
    } else {
        let source = HyperliquidFundingSource::new(cli.base_url.clone());
        Some(FundingFetcher::new(Arc::new(source)))
    };

    let execution = if cli.paper {
        ExecutionEngine::new(Arc::new(PaperOrderExecutor), RetryConfig::fast())
    } else {
        let private_key = cli
            .private_key
            .or(cli.api_key)
            .or_else(|| std::env::var("HYPERLIQUID_PRIVATE_KEY").ok())
            .or_else(|| std::env::var("STRATEGY_PRIVATE_KEY").ok())
            .or_else(|| std::env::var("STRATEGY_API_KEY").ok());
        let vault_address = cli
            .vault_address
            .or_else(|| std::env::var("HYPERLIQUID_VAULT_ADDRESS").ok())
            .or_else(|| std::env::var("HYPERLIQUID_VAULT").ok());
        let key = private_key.ok_or_else(|| anyhow!("missing Hyperliquid private key"))?;
        let mut executor = LiveOrderExecutor::with_private_key(cli.base_url.clone(), key);
        if let Some(vault) = vault_address {
            executor = executor.with_vault_address(vault);
        }
        ExecutionEngine::new(Arc::new(executor), RetryConfig::fast())
    };
    let mut engine = StrategyEngine::new(config.clone(), execution).context("create engine")?;

    let mut state_writer: Option<Arc<dyn StateWriter>> = None;
    if let Some(path) = cli.state_path.as_ref() {
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

    if cli.once {
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

    info!(interval_secs = cli.interval_secs, "starting live loop");
    runner
        .run_loop(Duration::from_secs(cli.interval_secs), shutdown_rx)
        .await
        .context("run loop")?;

    let _ = shutdown_handle.await;
    Ok(())
}
