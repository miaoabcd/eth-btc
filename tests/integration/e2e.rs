use chrono::{TimeZone, Utc};
use rust_decimal::MathematicalOps;
use rust_decimal_macros::dec;

use eth_btc_strategy::config::{Config, SigmaFloorMode};
use eth_btc_strategy::core::strategy::{StrategyBar, StrategyEngine};
use eth_btc_strategy::execution::{ExecutionEngine, MockOrderExecutor, RetryConfig};
use eth_btc_strategy::logging::LogEvent;
use eth_btc_strategy::state::StrategyStatus;

fn bar(timestamp: i64, r: rust_decimal::Decimal) -> StrategyBar {
    let base = dec!(100);
    let eth = base * r.exp();
    StrategyBar {
        timestamp: Utc.timestamp_opt(timestamp, 0).unwrap(),
        eth_price: eth,
        btc_price: base,
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: None,
    }
}

fn engine_with_mock(mut config: Config, executor: MockOrderExecutor) -> StrategyEngine {
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.sigma_floor.mode = SigmaFloorMode::Const;
    config.sigma_floor.sigma_floor_const = dec!(0.02);
    let execution = ExecutionEngine::new(std::sync::Arc::new(executor), RetryConfig::fast());
    StrategyEngine::new(config, execution).unwrap()
}

#[tokio::test]
async fn e2e_tp_flow() {
    let mut config = Config::default();
    config.strategy.entry_z = dec!(1.0);
    config.strategy.tp_z = dec!(1.0);

    let mut executor = MockOrderExecutor::default();
    executor.push_submit_response(eth_btc_strategy::config::Symbol::EthPerp, Ok(dec!(1)));
    executor.push_submit_response(eth_btc_strategy::config::Symbol::BtcPerp, Ok(dec!(1)));
    executor.push_close_response(eth_btc_strategy::config::Symbol::EthPerp, Ok(dec!(1)));
    executor.push_close_response(eth_btc_strategy::config::Symbol::BtcPerp, Ok(dec!(1)));

    let mut engine = engine_with_mock(config, executor);
    let bars = vec![
        bar(0, dec!(0.0)),
        bar(900, dec!(0.0)),
        bar(1800, dec!(0.01)),
        bar(2700, dec!(0.04)),
        bar(3600, dec!(0.0)),
    ];

    let mut last = None;
    for bar in bars {
        last = Some(engine.process_bar(bar).await.unwrap());
    }

    let outcome = last.unwrap();
    assert_eq!(outcome.state, StrategyStatus::Flat);
    assert!(
        outcome
            .events
            .iter()
            .any(|event| matches!(event, LogEvent::Exit(_)))
    );
}

#[tokio::test]
async fn e2e_sl_flow_with_cooldown() {
    let mut config = Config::default();
    config.strategy.entry_z = dec!(1.0);
    config.strategy.sl_z = dec!(1.2);

    let mut executor = MockOrderExecutor::default();
    executor.push_submit_response(eth_btc_strategy::config::Symbol::EthPerp, Ok(dec!(1)));
    executor.push_submit_response(eth_btc_strategy::config::Symbol::BtcPerp, Ok(dec!(1)));
    executor.push_close_response(eth_btc_strategy::config::Symbol::EthPerp, Ok(dec!(1)));
    executor.push_close_response(eth_btc_strategy::config::Symbol::BtcPerp, Ok(dec!(1)));

    let mut engine = engine_with_mock(config, executor);
    let bars = vec![
        bar(0, dec!(0.0)),
        bar(900, dec!(0.01)),
        bar(1800, dec!(0.02)),
        bar(2700, dec!(0.05)),
        bar(3600, dec!(0.09)),
    ];
    let mut outcome = None;
    for bar in bars {
        outcome = Some(engine.process_bar(bar).await.unwrap());
    }
    let outcome = outcome.unwrap();
    assert_eq!(outcome.state, StrategyStatus::Cooldown);
}

#[tokio::test]
async fn e2e_time_stop_flow() {
    let mut config = Config::default();
    config.strategy.tp_z = dec!(0.1);
    config.risk.max_hold_hours = 1;

    let mut executor = MockOrderExecutor::default();
    executor.push_submit_response(eth_btc_strategy::config::Symbol::EthPerp, Ok(dec!(1)));
    executor.push_submit_response(eth_btc_strategy::config::Symbol::BtcPerp, Ok(dec!(1)));
    executor.push_close_response(eth_btc_strategy::config::Symbol::EthPerp, Ok(dec!(1)));
    executor.push_close_response(eth_btc_strategy::config::Symbol::BtcPerp, Ok(dec!(1)));

    let mut engine = engine_with_mock(config, executor);
    let bars = vec![
        bar(0, dec!(0.0)),
        bar(900, dec!(0.0)),
        bar(1800, dec!(0.0)),
        bar(2700, dec!(0.04)),
        bar(7200, dec!(0.04)),
    ];

    let mut last = None;
    for bar in bars {
        last = Some(engine.process_bar(bar).await.unwrap());
    }
    let outcome = last.unwrap();
    assert_eq!(outcome.state, StrategyStatus::Flat);
}
