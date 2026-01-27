use rust_decimal_macros::dec;

use eth_btc_strategy::config::{CapitalMode, PositionConfig};
use eth_btc_strategy::position::{CapitalError, compute_capital, risk_parity_weights};

fn assert_close(
    actual: rust_decimal::Decimal,
    expected: rust_decimal::Decimal,
    tol: rust_decimal::Decimal,
) {
    let diff = (actual - expected).abs();
    assert!(diff <= tol, "diff {diff} > tol {tol}");
}

#[test]
fn risk_parity_weights_match_inverse_vol() {
    let result = risk_parity_weights(dec!(0.2), dec!(0.4)).unwrap();
    assert_close(result.w_eth, dec!(0.6666667), dec!(0.0001));
    assert_close(result.w_btc, dec!(0.3333333), dec!(0.0001));
}

#[test]
fn risk_parity_handles_zero_volatility() {
    let result = risk_parity_weights(dec!(0), dec!(0.4)).unwrap();
    assert_close(result.w_eth, dec!(0.5), dec!(0.0001));
    assert_close(result.w_btc, dec!(0.5), dec!(0.0001));

    let result = risk_parity_weights(dec!(0.0), dec!(0.0)).unwrap();
    assert_close(result.w_eth, dec!(0.5), dec!(0.0001));
}

#[test]
fn capital_allocation_fixed_and_equity_ratio() {
    let mut config = PositionConfig::default();
    config.c_mode = CapitalMode::FixedNotional;
    config.c_value = Some(dec!(50000));

    let capital = compute_capital(&config, dec!(100000)).unwrap();
    assert_eq!(capital, dec!(50000));

    config.c_mode = CapitalMode::EquityRatio;
    config.c_value = None;
    config.equity_ratio_k = Some(dec!(0.2));

    let capital = compute_capital(&config, dec!(100000)).unwrap();
    assert_eq!(capital, dec!(20000));

    config.equity_ratio_k = None;
    let err = compute_capital(&config, dec!(100000)).unwrap_err();
    assert!(matches!(err, CapitalError::InvalidConfig(_)));
}
