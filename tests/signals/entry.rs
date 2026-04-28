use rust_decimal_macros::dec;

use eth_btc_strategy::config::{StaleCrossConfig, StrategyConfig};
use eth_btc_strategy::core::TradeDirection;
use eth_btc_strategy::signals::EntrySignalDetector;
use eth_btc_strategy::state::StrategyStatus;

#[test]
fn entry_signal_requires_crossing_and_flat_state() {
    let config = StrategyConfig::default();
    let mut detector = EntrySignalDetector::new(config);

    assert!(
        detector
            .update(Some(dec!(1.4)), StrategyStatus::Flat)
            .is_none()
    );

    let signal = detector
        .update(Some(dec!(1.6)), StrategyStatus::Flat)
        .unwrap();
    assert_eq!(signal.direction, TradeDirection::ShortEthLongBtc);

    let no_repeat = detector
        .update(Some(dec!(1.7)), StrategyStatus::Flat)
        .is_none();
    assert!(no_repeat);

    let blocked = detector.update(Some(dec!(-1.6)), StrategyStatus::InPosition);
    assert!(blocked.is_none());
}

#[test]
fn entry_signal_negative_z_goes_long_eth_short_btc() {
    let config = StrategyConfig::default();
    let mut detector = EntrySignalDetector::new(config);

    detector.update(Some(dec!(-1.4)), StrategyStatus::Flat);
    let signal = detector
        .update(Some(dec!(-1.6)), StrategyStatus::Flat)
        .unwrap();

    assert_eq!(signal.direction, TradeDirection::LongEthShortBtc);
}

#[test]
fn entry_signal_does_not_trigger_on_first_bar_in_zone() {
    let config = StrategyConfig::default();
    let mut detector = EntrySignalDetector::new(config);

    let first = detector.update(Some(dec!(1.6)), StrategyStatus::Flat);
    assert!(first.is_none());

    let still_in_zone = detector.update(Some(dec!(1.7)), StrategyStatus::Flat);
    assert!(still_in_zone.is_none());

    let below = detector.update(Some(dec!(1.4)), StrategyStatus::Flat);
    assert!(below.is_none());

    let crossed = detector.update(Some(dec!(1.6)), StrategyStatus::Flat);
    assert!(crossed.is_some());
}

#[test]
fn stale_cross_signal_can_recover_after_cooldown_release_when_reverting() {
    let config = StrategyConfig::default();
    let stale = StaleCrossConfig {
        enabled: true,
        max_age_bars: 3,
        require_reverting: true,
    };
    let mut detector = EntrySignalDetector::with_stale_cross(config, stale);

    assert!(
        detector
            .update(Some(dec!(1.7)), StrategyStatus::Cooldown)
            .is_none()
    );

    let signal = detector
        .update(Some(dec!(1.6)), StrategyStatus::Flat)
        .expect("expected cooldown-aware stale-cross recovery");
    assert_eq!(signal.direction, TradeDirection::ShortEthLongBtc);
}

#[test]
fn stale_cross_signal_does_not_fire_when_zscore_is_worsening() {
    let config = StrategyConfig::default();
    let stale = StaleCrossConfig {
        enabled: true,
        max_age_bars: 3,
        require_reverting: true,
    };
    let mut detector = EntrySignalDetector::with_stale_cross(config, stale);

    detector.update(Some(dec!(1.6)), StrategyStatus::Cooldown);
    let signal = detector.update(Some(dec!(1.7)), StrategyStatus::Flat);

    assert!(signal.is_none());
}

#[test]
fn stale_cross_signal_does_not_recover_after_non_cooldown_state() {
    let config = StrategyConfig::default();
    let stale = StaleCrossConfig {
        enabled: true,
        max_age_bars: 3,
        require_reverting: true,
    };
    let mut detector = EntrySignalDetector::with_stale_cross(config, stale);

    detector.update(Some(dec!(1.7)), StrategyStatus::InPosition);
    let signal = detector.update(Some(dec!(1.6)), StrategyStatus::Flat);

    assert!(signal.is_none());
}

#[test]
fn stale_cross_signal_expires_after_cooldown_recovery_window() {
    let config = StrategyConfig::default();
    let stale = StaleCrossConfig {
        enabled: true,
        max_age_bars: 1,
        require_reverting: true,
    };
    let mut detector = EntrySignalDetector::with_stale_cross(config, stale);

    detector.update(Some(dec!(1.8)), StrategyStatus::Cooldown);
    assert!(
        detector
            .update(Some(dec!(1.9)), StrategyStatus::Flat)
            .is_none()
    );
    let signal = detector.update(Some(dec!(1.7)), StrategyStatus::Flat);

    assert!(signal.is_none());
}
