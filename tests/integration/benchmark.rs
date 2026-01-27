use std::fs;
use std::time::{Duration, Instant};

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

fn read_rss_kb() -> Option<u64> {
    let contents = fs::read_to_string("/proc/self/statm").ok()?;
    let pages = contents.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    Some(pages * 4096 / 1024)
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

#[test]
fn performance_benchmark_thresholds() {
    let config = Config::default();
    let mut bars = Vec::new();
    let total_bars = 365 * 24 * 4;
    for i in 0..total_bars {
        bars.push(bar(i as i64 * 900, dec!(0.0)));
    }

    let rss_before = read_rss_kb();
    let start = Instant::now();
    let engine = BacktestEngine::new(config);
    let result = engine.run(&bars).unwrap();
    let elapsed = start.elapsed();
    let rss_after = read_rss_kb();

    assert_eq!(result.bar_logs.len(), bars.len());
    assert!(
        elapsed < Duration::from_secs(60),
        "1y backtest took {:?}",
        elapsed
    );
    let per_bar = elapsed.as_secs_f64() / bars.len() as f64;
    assert!(per_bar < 0.1, "per-bar time {:.4}s", per_bar);

    if let (Some(before), Some(after)) = (rss_before, rss_after) {
        let growth = after.saturating_sub(before);
        assert!(growth < 50 * 1024, "rss grew by {growth} KB");
    }
}
