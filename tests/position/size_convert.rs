use rust_decimal_macros::dec;

use eth_btc_strategy::config::{InstrumentConstraints, RoundingMode};
use eth_btc_strategy::position::{MinSizePolicy, PositionError, SizeConverter};

#[test]
fn size_convert_skips_when_below_minimum() {
    let constraints = InstrumentConstraints {
        min_qty: dec!(0.1),
        min_notional: dec!(10),
        step_size: dec!(0.05),
        tick_size: dec!(0.1),
        qty_precision: 2,
        price_precision: 1,
        rounding_mode: RoundingMode::Floor,
    };

    let converter = SizeConverter::new(constraints, MinSizePolicy::Skip);
    let err = converter.convert_notional(dec!(5), dec!(100)).unwrap_err();
    assert!(matches!(err, PositionError::BelowMinimum(_)));
}

#[test]
fn size_convert_adjusts_to_minimum() {
    let constraints = InstrumentConstraints {
        min_qty: dec!(0.1),
        min_notional: dec!(10),
        step_size: dec!(0.05),
        tick_size: dec!(0.1),
        qty_precision: 2,
        price_precision: 1,
        rounding_mode: RoundingMode::Floor,
    };

    let converter = SizeConverter::new(constraints, MinSizePolicy::Adjust);
    let order = converter.convert_notional(dec!(5), dec!(100)).unwrap();
    assert_eq!(order.qty, dec!(0.1));
    assert_eq!(order.notional, dec!(10));
}

#[test]
fn size_convert_rounds_to_step_size() {
    let constraints = InstrumentConstraints {
        min_qty: dec!(0.01),
        min_notional: dec!(1),
        step_size: dec!(0.05),
        tick_size: dec!(0.1),
        qty_precision: 2,
        price_precision: 1,
        rounding_mode: RoundingMode::Floor,
    };

    let converter = SizeConverter::new(constraints, MinSizePolicy::Skip);
    let order = converter.convert_notional(dec!(12), dec!(100)).unwrap();

    assert_eq!(order.qty, dec!(0.1));
    assert_eq!(order.notional, dec!(10));
}
