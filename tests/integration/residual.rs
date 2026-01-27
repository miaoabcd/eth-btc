use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::core::TradeDirection;
use eth_btc_strategy::execution::{ExecutionEngine, MockOrderExecutor, RetryConfig};
use eth_btc_strategy::state::{PositionLeg, PositionSnapshot, StateMachine, StrategyStatus};

#[tokio::test]
async fn residual_repair_transitions_to_flat() {
    let position = PositionSnapshot {
        direction: TradeDirection::LongEthShortBtc,
        entry_time: Utc.timestamp_opt(0, 0).unwrap(),
        eth: PositionLeg {
            qty: dec!(1),
            avg_price: dec!(100),
            notional: dec!(100),
        },
        btc: PositionLeg {
            qty: dec!(0),
            avg_price: dec!(200),
            notional: dec!(0),
        },
    };

    let mut executor = MockOrderExecutor::default();
    executor.push_close_response(eth_btc_strategy::config::Symbol::EthPerp, Ok(dec!(1)));

    let engine = ExecutionEngine::new(std::sync::Arc::new(executor), RetryConfig::fast());
    engine.repair_residual(&position).await.unwrap();

    let mut state_machine = StateMachine::new(eth_btc_strategy::config::RiskConfig::default());
    state_machine
        .enter(position, Utc.timestamp_opt(0, 0).unwrap())
        .unwrap();
    state_machine
        .exit(
            eth_btc_strategy::core::ExitReason::TimeStop,
            Utc.timestamp_opt(0, 0).unwrap(),
        )
        .unwrap();
    assert_eq!(state_machine.state().status, StrategyStatus::Flat);
}
