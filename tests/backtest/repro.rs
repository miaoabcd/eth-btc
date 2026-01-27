use chrono::{TimeZone, Utc};
use rust_decimal::MathematicalOps;
use rust_decimal_macros::dec;

use eth_btc_strategy::backtest::{BacktestBar, verify_reproducibility};
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
fn backtest_is_reproducible() {
    let bars = vec![
        bar(0, dec!(0.0)),
        bar(900, dec!(0.0)),
        bar(1800, dec!(0.0)),
        bar(2700, dec!(0.04)),
        bar(3600, dec!(0.0)),
    ];
    let mut config = Config::default();
    config.strategy.n_z = 4;

    verify_reproducibility(&config, &bars).unwrap();
}
