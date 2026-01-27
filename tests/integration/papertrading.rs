use rust_decimal_macros::dec;

use eth_btc_strategy::backtest::Metrics;
use eth_btc_strategy::integration::papertrading_gate;

#[test]
fn papertrading_gate_checks_drawdown() {
    let mut metrics = Metrics::default();
    metrics.max_drawdown = dec!(0.10);
    assert!(papertrading_gate(&metrics));

    metrics.max_drawdown = dec!(0.20);
    assert!(!papertrading_gate(&metrics));
}
