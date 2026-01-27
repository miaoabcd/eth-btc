use chrono::{TimeZone, Utc};
use rust_decimal::MathematicalOps;
use rust_decimal_macros::dec;

use eth_btc_strategy::backtest::{BacktestBar, run_sensitivity};
use eth_btc_strategy::config::Config;

fn bar(timestamp: i64, r: rust_decimal::Decimal) -> BacktestBar {
    let base = dec!(100);
    let eth = base * r.exp();
    BacktestBar {
        timestamp: Utc.timestamp_opt(timestamp, 0).unwrap(),
        eth_price: eth,
        btc_price: base,
        funding_eth: None,
        funding_btc: None,
    }
}

#[test]
fn sensitivity_runs_multiple_configs() {
    let bars = vec![
        bar(0, dec!(0.0)),
        bar(900, dec!(0.0)),
        bar(1800, dec!(0.0)),
        bar(2700, dec!(0.04)),
        bar(3600, dec!(0.0)),
    ];
    let mut config1 = Config::default();
    config1.strategy.n_z = 4;
    let mut config2 = Config::default();
    config2.strategy.n_z = 4;
    config2.strategy.entry_z = dec!(1.2);

    let results = run_sensitivity(&[config1, config2], &bars).unwrap();
    assert_eq!(results.len(), 2);
}
