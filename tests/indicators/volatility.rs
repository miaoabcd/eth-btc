use rust_decimal_macros::dec;

use eth_btc_strategy::indicators::{IndicatorError, VolatilityCalculator};

fn assert_close(
    actual: rust_decimal::Decimal,
    expected: rust_decimal::Decimal,
    tol: rust_decimal::Decimal,
) {
    let diff = (actual - expected).abs();
    assert!(diff <= tol, "diff {diff} > tol {tol}");
}

#[test]
fn volatility_returns_none_until_warm() {
    let mut calc = VolatilityCalculator::new(3).unwrap();
    assert!(calc.update(dec!(100), dec!(200)).unwrap().vol_eth.is_none());
    assert!(calc.update(dec!(110), dec!(210)).unwrap().vol_eth.is_none());
}

#[test]
fn volatility_of_constant_returns_is_zero() {
    let mut calc = VolatilityCalculator::new(3).unwrap();
    let prices = [dec!(100), dec!(110), dec!(121), dec!(133.1)];
    let btc_prices = [dec!(200), dec!(220), dec!(242), dec!(266.2)];

    for i in 0..prices.len() {
        let snapshot = calc.update(prices[i], btc_prices[i]).unwrap();
        if i < 3 {
            assert!(snapshot.vol_eth.is_none());
        } else {
            assert_close(snapshot.vol_eth.unwrap(), dec!(0), dec!(0.000001));
            assert_close(snapshot.vol_btc.unwrap(), dec!(0), dec!(0.000001));
        }
    }
}

#[test]
fn volatility_rejects_non_positive_prices() {
    let mut calc = VolatilityCalculator::new(2).unwrap();
    let err = calc.update(dec!(0), dec!(1)).unwrap_err();
    assert!(matches!(err, IndicatorError::InvalidPrice(_)));
}
