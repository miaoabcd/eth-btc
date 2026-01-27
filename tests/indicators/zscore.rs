use rust_decimal::MathematicalOps;
use rust_decimal_macros::dec;

use eth_btc_strategy::config::{SigmaFloorConfig, SigmaFloorMode};
use eth_btc_strategy::indicators::{IndicatorError, ZScoreCalculator};

fn assert_close(
    actual: rust_decimal::Decimal,
    expected: rust_decimal::Decimal,
    tol: rust_decimal::Decimal,
) {
    let diff = (actual - expected).abs();
    assert!(diff <= tol, "diff {diff} > tol {tol}");
}

#[test]
fn zscore_returns_none_until_warm() {
    let mut config = SigmaFloorConfig::default();
    config.mode = SigmaFloorMode::Const;
    config.sigma_floor_const = dec!(0.5);

    let mut calc = ZScoreCalculator::new(3, config, 1).unwrap();
    let snapshot = calc.update(dec!(1.0)).unwrap();
    assert!(snapshot.zscore.is_none());
    let snapshot = calc.update(dec!(2.0)).unwrap();
    assert!(snapshot.zscore.is_none());
}

#[test]
fn zscore_uses_sigma_floor_when_sigma_is_small() {
    let mut config = SigmaFloorConfig::default();
    config.mode = SigmaFloorMode::Const;
    config.sigma_floor_const = dec!(0.5);

    let mut calc = ZScoreCalculator::new(3, config, 1).unwrap();
    calc.update(dec!(1.0)).unwrap();
    calc.update(dec!(1.0)).unwrap();
    let snapshot = calc.update(dec!(1.0)).unwrap();

    assert_eq!(snapshot.sigma_eff.unwrap(), dec!(0.5));
    assert_eq!(snapshot.zscore.unwrap(), dec!(0));
}

#[test]
fn zscore_matches_expected_value() {
    let mut config = SigmaFloorConfig::default();
    config.mode = SigmaFloorMode::Const;
    config.sigma_floor_const = dec!(0.1);

    let mut calc = ZScoreCalculator::new(3, config, 1).unwrap();
    calc.update(dec!(1.0)).unwrap();
    calc.update(dec!(2.0)).unwrap();
    let snapshot = calc.update(dec!(3.0)).unwrap();

    let mean = dec!(2.0);
    let variance = dec!(2) / dec!(3);
    let sigma = variance.sqrt().unwrap();
    let expected = (dec!(3.0) - mean) / sigma;

    assert_close(snapshot.zscore.unwrap(), expected, dec!(0.000001));
}

#[test]
fn zscore_rejects_nan_sigma_floor() {
    let mut config = SigmaFloorConfig::default();
    config.mode = SigmaFloorMode::Const;
    config.sigma_floor_const = dec!(0);

    let err = ZScoreCalculator::new(3, config, 1).unwrap_err();
    assert!(matches!(err, IndicatorError::InvalidConfig(_)));
}
