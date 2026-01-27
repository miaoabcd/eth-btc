use chrono::{TimeZone, Utc};
use rust_decimal::MathematicalOps;
use rust_decimal_macros::dec;

use eth_btc_strategy::config::{Config, SigmaFloorMode};
use eth_btc_strategy::core::strategy::{StrategyBar, StrategyEngine};
use eth_btc_strategy::execution::{ExecutionEngine, MockOrderExecutor, RetryConfig};
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
    }
}

#[tokio::test]
async fn data_flow_triggers_state_and_logging() {
    let mut config = Config::default();
    config.strategy.n_z = 4;
    config.strategy.tp_z = dec!(0.6);
    config.position.n_vol = 1;
    config.sigma_floor.mode = SigmaFloorMode::Const;

    let mut executor = MockOrderExecutor::default();
    executor.push_submit_response(eth_btc_strategy::config::Symbol::EthPerp, Ok(dec!(1)));
    executor.push_submit_response(eth_btc_strategy::config::Symbol::BtcPerp, Ok(dec!(1)));
    executor.push_close_response(eth_btc_strategy::config::Symbol::EthPerp, Ok(dec!(1)));
    executor.push_close_response(eth_btc_strategy::config::Symbol::BtcPerp, Ok(dec!(1)));

    let execution = ExecutionEngine::new(std::sync::Arc::new(executor), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config, execution).unwrap();

    let bars = vec![
        bar(0, dec!(0.0)),
        bar(900, dec!(0.0)),
        bar(1800, dec!(0.0)),
        bar(2700, dec!(0.04)),
        bar(3600, dec!(0.0)),
    ];

    let mut last_outcome = None;
    for bar in bars {
        last_outcome = Some(engine.process_bar(bar).await.unwrap());
    }

    let outcome = last_outcome.unwrap();
    assert_eq!(outcome.state, StrategyStatus::Flat);
    assert!(!outcome.bar_log.events.is_empty());
}
