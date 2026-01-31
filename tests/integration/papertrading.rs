use rust_decimal_macros::dec;

use eth_btc_strategy::backtest::Metrics;
use eth_btc_strategy::integration::papertrading_gate;

#[test]
fn papertrading_gate_checks_drawdown() {
    let mut metrics = Metrics::default();
    metrics.max_drawdown = dec!(0.10);
    metrics.sharpe_ratio = dec!(1.0);
    metrics.win_rate = dec!(0.55);
    metrics.profit_factor = dec!(1.2);
    metrics.trade_count = 40;
    assert!(papertrading_gate(&metrics));

    metrics.max_drawdown = dec!(0.20);
    assert!(!papertrading_gate(&metrics));
}

#[test]
fn papertrading_gate_requires_minimum_trades() {
    let mut metrics = Metrics::default();
    metrics.max_drawdown = dec!(0.10);
    metrics.sharpe_ratio = dec!(1.0);
    metrics.win_rate = dec!(0.55);
    metrics.profit_factor = dec!(1.2);
    metrics.trade_count = 5;
    assert!(!papertrading_gate(&metrics));
}
