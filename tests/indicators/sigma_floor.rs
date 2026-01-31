use rust_decimal::Decimal;
use rust_decimal::MathematicalOps;
use rust_decimal_macros::dec;

use eth_btc_strategy::config::{SigmaFloorConfig, SigmaFloorMode};
use eth_btc_strategy::indicators::{SigmaFloorCalculator, ewma_std};

fn assert_close(
    actual: rust_decimal::Decimal,
    expected: rust_decimal::Decimal,
    tol: rust_decimal::Decimal,
) {
    let diff = (actual - expected).abs();
    assert!(diff <= tol, "diff {diff} > tol {tol}");
}

#[test]
fn sigma_floor_const_returns_configured_value() {
    let mut config = SigmaFloorConfig::default();
    config.mode = SigmaFloorMode::Const;
    config.sigma_floor_const = dec!(0.5);

    let mut calc = SigmaFloorCalculator::new(config, 1).unwrap();
    let floor = calc.update(dec!(0.1), &[dec!(1.0)]).unwrap();

    assert_eq!(floor, dec!(0.5));
}

#[test]
fn sigma_floor_quantile_uses_history_window() {
    let mut config = SigmaFloorConfig::default();
    config.mode = SigmaFloorMode::Quantile;
    config.sigma_floor_quantile_window = 3;
    config.sigma_floor_quantile_p = dec!(0.1);

    let mut calc = SigmaFloorCalculator::new(config, 1).unwrap();
    assert!(calc.update(dec!(0.1), &[]).is_none());
    assert!(calc.update(dec!(0.2), &[]).is_none());
    let floor = calc.update(dec!(0.3), &[]).unwrap();

    assert_eq!(floor, dec!(0.1));
}

#[test]
fn ewma_std_matches_expected_decay() {
    let values = vec![dec!(1.0), dec!(2.0)];
    let std = ewma_std(&values, 1).unwrap();
    let expected_var = dec!(0.125);
    let expected_std = expected_var.sqrt().unwrap();
    assert_close(std, expected_std, dec!(0.000001));
}

#[test]
fn ewma_std_uses_decimal_decay() {
    let values = vec![dec!(1.0), dec!(2.0), dec!(3.0)];
    let half_life = 7;
    let decay = dec!(0.5).powd(Decimal::ONE / Decimal::from(half_life));
    let alpha = Decimal::ONE - decay;
    let mut mean = values[0];
    let mut variance = Decimal::ZERO;
    for value in values.iter().skip(1) {
        let delta = *value - mean;
        mean += alpha * delta;
        let diff = *value - mean;
        variance = alpha * diff * diff + (Decimal::ONE - alpha) * variance;
    }
    let expected = variance.sqrt().unwrap();
    let actual = ewma_std(&values, half_life).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn sigma_floor_ewma_mix_uses_max_floor() {
    let mut config = SigmaFloorConfig::default();
    config.mode = SigmaFloorMode::EwmaMix;
    config.sigma_floor_quantile_window = 2;
    config.sigma_floor_quantile_p = dec!(0.5);
    config.ewma_half_life = 1;

    let mut calc = SigmaFloorCalculator::new(config, 1).unwrap();
    assert!(calc.update(dec!(0.1), &[dec!(1.0), dec!(2.0)]).is_none());
    let floor = calc.update(dec!(0.2), &[dec!(1.0), dec!(2.0)]).unwrap();

    let ewma = ewma_std(&[dec!(1.0), dec!(2.0)], 1).unwrap();
    let expected_quantile = dec!(0.1);
    let expected = if ewma > expected_quantile {
        ewma
    } else {
        expected_quantile
    };

    assert_close(floor, expected, dec!(0.000001));
}
