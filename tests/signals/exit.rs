use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::config::{RiskConfig, StrategyConfig};
use eth_btc_strategy::core::{ExitReason, TradeDirection};
use eth_btc_strategy::signals::ExitSignalDetector;
use eth_btc_strategy::state::{PositionLeg, PositionSnapshot, StrategyStatus};

fn sample_position(entry_time: i64) -> PositionSnapshot {
    PositionSnapshot {
        direction: TradeDirection::LongEthShortBtc,
        entry_time: Utc.timestamp_opt(entry_time, 0).unwrap(),
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
fn exit_signal_take_profit_with_confirmation() {
    let mut risk = RiskConfig::default();
    risk.confirm_bars_tp = 2;

    let mut detector = ExitSignalDetector::new(StrategyConfig::default(), risk);
    let position = sample_position(0);
    let now = Utc.timestamp_opt(3600, 0).unwrap();

    let first = detector.evaluate(
        Some(dec!(0.4)),
        StrategyStatus::InPosition,
        Some(&position),
        now,
    );
    assert!(first.is_none());

    let second = detector.evaluate(
        Some(dec!(0.4)),
        StrategyStatus::InPosition,
        Some(&position),
        now,
    );
    assert!(matches!(second, Some(signal) if signal.reason == ExitReason::TakeProfit));
}

#[test]
fn exit_signal_stop_loss_before_time_stop() {
    let mut detector = ExitSignalDetector::new(StrategyConfig::default(), RiskConfig::default());
    let position = sample_position(0);
    let now = Utc.timestamp_opt(60 * 60 * 49, 0).unwrap();

    let signal = detector.evaluate(
        Some(dec!(3.6)),
        StrategyStatus::InPosition,
        Some(&position),
        now,
    );
    assert!(matches!(signal, Some(signal) if signal.reason == ExitReason::StopLoss));
}

#[test]
fn exit_signal_time_stop_when_no_tp_sl() {
    let mut detector = ExitSignalDetector::new(StrategyConfig::default(), RiskConfig::default());
    let position = sample_position(0);
    let now = Utc.timestamp_opt(60 * 60 * 49, 0).unwrap();

    let signal = detector.evaluate(
        Some(dec!(1.0)),
        StrategyStatus::InPosition,
        Some(&position),
        now,
    );
    assert!(matches!(signal, Some(signal) if signal.reason == ExitReason::TimeStop));
}
