use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::core::TradeDirection;
use eth_btc_strategy::state::{
    PositionLeg, PositionSnapshot, RecoveryAction, StrategyState, StrategyStatus, recover_state,
};

#[test]
fn recovery_handles_missing_position() {
    let state = StrategyState {
        status: StrategyStatus::InPosition,
        position: None,
        cooldown_until: None,
    };

    let report = recover_state(state, Utc.timestamp_opt(0, 0).unwrap());
    assert_eq!(report.state.status, StrategyStatus::Flat);
    assert!(!report.alerts.is_empty());
}

#[test]
fn recovery_flags_residual_position() {
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

    let state = StrategyState {
        status: StrategyStatus::InPosition,
        position: Some(position),
        cooldown_until: None,
    };

    let report = recover_state(state, Utc.timestamp_opt(0, 0).unwrap());
    assert!(
        report
            .actions
            .iter()
            .any(|action| matches!(action, RecoveryAction::RepairResidual))
    );
    assert!(!report.alerts.is_empty());
}
