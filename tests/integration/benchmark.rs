use chrono::{TimeZone, Utc};
use rust_decimal::MathematicalOps;
use rust_decimal_macros::dec;

use eth_btc_strategy::backtest::{BacktestBar, BacktestEngine};
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
fn performance_benchmark_smoke() {
    let config = Config::default();
    let mut bars = Vec::new();
    for i in 0..100 {
        bars.push(bar(i * 900, dec!(0.0)));
    }

    let engine = BacktestEngine::new(config);
    let result = engine.run(&bars).unwrap();
    assert_eq!(result.bar_logs.len(), bars.len());
}
