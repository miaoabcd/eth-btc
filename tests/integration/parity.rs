use chrono::{TimeZone, Utc};
use rust_decimal::MathematicalOps;
use rust_decimal_macros::dec;

use eth_btc_strategy::backtest::{BacktestBar, BacktestEngine};
use eth_btc_strategy::config::{Config, SigmaFloorMode};
use eth_btc_strategy::core::strategy::{StrategyBar, StrategyEngine};
use eth_btc_strategy::execution::{ExecutionEngine, MockOrderExecutor, RetryConfig};
use eth_btc_strategy::logging::LogEvent;

fn backtest_bar(timestamp: i64, r: rust_decimal::Decimal) -> BacktestBar {
    let base = dec!(100);
    let eth = base * r.exp();
    BacktestBar {
        timestamp: Utc.timestamp_opt(timestamp, 0).unwrap(),
        eth_price: eth,
        btc_price: base,
        funding_eth: None,
        funding_btc: None,
    }
}

fn strategy_bar(timestamp: i64, r: rust_decimal::Decimal) -> StrategyBar {
    let base = dec!(100);
    let eth = base * r.exp();
    StrategyBar {
        timestamp: Utc.timestamp_opt(timestamp, 0).unwrap(),
        eth_price: eth,
        btc_price: base,
        funding_eth: None,
        funding_btc: None,
    }
}

#[tokio::test]
async fn backtest_and_strategy_parity_on_trade_count() {
    let mut config = Config::default();
    config.strategy.n_z = 4;
    config.position.n_vol = 1;
    config.strategy.tp_z = dec!(0.6);
    config.sigma_floor.mode = SigmaFloorMode::Const;

    let bars = vec![dec!(0.0), dec!(0.0), dec!(0.0), dec!(0.04), dec!(0.0)];

    let backtest_bars = bars
        .iter()
        .enumerate()
        .map(|(i, r)| backtest_bar((i as i64) * 900, *r))
        .collect::<Vec<_>>();
    let engine = BacktestEngine::new(config.clone());
    let backtest = engine.run(&backtest_bars).unwrap();

    let mut executor = MockOrderExecutor::default();
    executor.push_submit_response(eth_btc_strategy::config::Symbol::EthPerp, Ok(dec!(1)));
    executor.push_submit_response(eth_btc_strategy::config::Symbol::BtcPerp, Ok(dec!(1)));
    executor.push_close_response(eth_btc_strategy::config::Symbol::EthPerp, Ok(dec!(1)));
    executor.push_close_response(eth_btc_strategy::config::Symbol::BtcPerp, Ok(dec!(1)));
    let execution = ExecutionEngine::new(std::sync::Arc::new(executor), RetryConfig::fast());
    let mut strategy = StrategyEngine::new(config, execution).unwrap();

    let mut exit_count = 0;
    for (i, r) in bars.iter().enumerate() {
        let outcome = strategy
            .process_bar(strategy_bar((i as i64) * 900, *r))
            .await
            .unwrap();
        if outcome
            .events
            .iter()
            .any(|event| matches!(event, LogEvent::Exit(_)))
        {
            exit_count += 1;
        }
    }

    assert_eq!(backtest.trades.len(), exit_count);
}
