use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::storage::{PriceBarRecord, PriceStore};

#[test]
fn price_store_saves_and_loads_record() {
    let store = PriceStore::new_in_memory().unwrap();
    let timestamp = Utc.timestamp_opt(0, 0).unwrap();
    let record = PriceBarRecord {
        timestamp,
        eth_mid: Some(dec!(2000)),
        eth_mark: Some(dec!(2001)),
        eth_close: Some(dec!(1999)),
        btc_mid: Some(dec!(30000)),
        btc_mark: Some(dec!(30010)),
        btc_close: Some(dec!(29990)),
        funding_eth: Some(dec!(0.0001)),
        funding_btc: Some(dec!(0.0002)),
        funding_interval_hours: Some(8),
    };

    store.save(&record).unwrap();
    let loaded = store.load(timestamp).unwrap().expect("record");

    assert_eq!(loaded, record);
}
