use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::config::Config;
use eth_btc_strategy::core::TradeDirection;
use eth_btc_strategy::core::strategy::StrategyEngine;
use eth_btc_strategy::execution::{ExecutionEngine, PaperOrderExecutor, RetryConfig};
use eth_btc_strategy::state::{PositionLeg, PositionSnapshot, StrategyState, StrategyStatus};

#[test]
fn strategy_engine_applies_state() {
    let config = Config::default();
    let execution =
        ExecutionEngine::new(std::sync::Arc::new(PaperOrderExecutor), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config, execution).unwrap();

    let position = PositionSnapshot {
        direction: TradeDirection::LongEthShortBtc,
        entry_time: Utc.timestamp_opt(0, 0).unwrap(),
        eth: PositionLeg {
            qty: dec!(1),
            avg_price: dec!(100),
            notional: dec!(100),
        },
        btc: PositionLeg {
            qty: dec!(-1),
            avg_price: dec!(200),
            notional: dec!(200),
        },
    };

    let state = StrategyState {
        status: StrategyStatus::InPosition,
        position: Some(position),
        cooldown_until: None,
    };

    engine.apply_state(state).unwrap();
    assert_eq!(engine.state().state().status, StrategyStatus::InPosition);
}
