use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;

use eth_btc_strategy::config::Symbol;
use eth_btc_strategy::data::{PriceBar, PriceHistorySet};

fn bar(symbol: Symbol, ts: i64, price: i64) -> PriceBar {
    PriceBar::new(
        symbol,
        Utc.timestamp_opt(ts, 0).unwrap(),
        Some(Decimal::from(price)),
        None,
        None,
    )
}

#[test]
fn data_integrity_rejects_mismatched_timestamps() {
    let mut history = PriceHistorySet::new(2, 2, 2).unwrap();
    let result = history.push_pair(bar(Symbol::EthPerp, 0, 100), bar(Symbol::BtcPerp, 1, 200));
    assert!(result.is_err());
}
