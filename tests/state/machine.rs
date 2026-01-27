use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::config::RiskConfig;
use eth_btc_strategy::core::{ExitReason, TradeDirection};
use eth_btc_strategy::state::{
    PositionLeg, PositionSnapshot, StateError, StateMachine, StrategyStatus,
};

fn sample_position(timestamp: i64) -> PositionSnapshot {
    PositionSnapshot {
        direction: TradeDirection::LongEthShortBtc,
        entry_time: Utc.timestamp_opt(timestamp, 0).unwrap(),
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
    }
}

#[test]
fn state_machine_transitions_and_cooldown() {
    let config = RiskConfig::default();
    let mut machine = StateMachine::new(config);
    assert_eq!(machine.state().status, StrategyStatus::Flat);

    let now = Utc.timestamp_opt(100, 0).unwrap();
    machine.enter(sample_position(100), now).unwrap();
    assert_eq!(machine.state().status, StrategyStatus::InPosition);

    let stop_loss_time = Utc.timestamp_opt(200, 0).unwrap();
    machine.exit(ExitReason::StopLoss, stop_loss_time).unwrap();
    assert_eq!(machine.state().status, StrategyStatus::Cooldown);

    let err = machine.enter(sample_position(210), Utc.timestamp_opt(210, 0).unwrap());
    assert!(matches!(err, Err(StateError::InvalidTransition(_))));

    let after_cooldown = stop_loss_time + chrono::Duration::hours(24);
    machine.update(after_cooldown);
    assert_eq!(machine.state().status, StrategyStatus::Flat);
}

#[test]
fn state_machine_take_profit_returns_to_flat() {
    let config = RiskConfig::default();
    let mut machine = StateMachine::new(config);

    let now = Utc.timestamp_opt(100, 0).unwrap();
    machine.enter(sample_position(100), now).unwrap();

    let exit_time = Utc.timestamp_opt(200, 0).unwrap();
    machine.exit(ExitReason::TakeProfit, exit_time).unwrap();

    assert_eq!(machine.state().status, StrategyStatus::Flat);
}

#[test]
fn state_machine_time_stop_returns_to_flat() {
    let config = RiskConfig::default();
    let mut machine = StateMachine::new(config);

    let now = Utc.timestamp_opt(100, 0).unwrap();
    machine.enter(sample_position(100), now).unwrap();

    let exit_time = Utc.timestamp_opt(200, 0).unwrap();
    machine.exit(ExitReason::TimeStop, exit_time).unwrap();

    assert_eq!(machine.state().status, StrategyStatus::Flat);
}
