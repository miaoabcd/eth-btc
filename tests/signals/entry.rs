use rust_decimal_macros::dec;

use eth_btc_strategy::config::StrategyConfig;
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
