use rust_decimal::MathematicalOps;
use rust_decimal_macros::dec;

use eth_btc_strategy::indicators::{IndicatorError, relative_price};

fn assert_close(
    actual: rust_decimal::Decimal,
    expected: rust_decimal::Decimal,
    tol: rust_decimal::Decimal,
) {
    let diff = (actual - expected).abs();
    assert!(diff <= tol, "diff {diff} > tol {tol}");
}

#[test]
fn relative_price_uses_log_ratio() {
    let eth = dec!(200);
    let btc = dec!(100);
    let r = relative_price(eth, btc).unwrap();
    let expected = (eth / btc).ln();
    assert_close(r, expected, dec!(0.0000001));
}

#[test]
fn relative_price_rejects_non_positive_prices() {
    let err = relative_price(dec!(0), dec!(100)).unwrap_err();
    assert!(matches!(err, IndicatorError::InvalidPrice(_)));

    let err = relative_price(dec!(100), dec!(-1)).unwrap_err();
    assert!(matches!(err, IndicatorError::InvalidPrice(_)));
}
