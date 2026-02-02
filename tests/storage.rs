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

#[test]
fn price_store_loads_range_in_order() {
    let store = PriceStore::new_in_memory().unwrap();
    let t1 = Utc.timestamp_opt(0, 0).unwrap();
    let t2 = Utc.timestamp_opt(900, 0).unwrap();
    let t3 = Utc.timestamp_opt(1800, 0).unwrap();

    store
        .save(&PriceBarRecord {
            timestamp: t2,
            eth_mid: Some(dec!(2000)),
            eth_mark: None,
            eth_close: None,
            btc_mid: Some(dec!(30000)),
            btc_mark: None,
            btc_close: None,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        })
        .unwrap();
    store
        .save(&PriceBarRecord {
            timestamp: t1,
            eth_mid: Some(dec!(1900)),
            eth_mark: None,
            eth_close: None,
            btc_mid: Some(dec!(29000)),
            btc_mark: None,
            btc_close: None,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        })
        .unwrap();
    store
        .save(&PriceBarRecord {
            timestamp: t3,
            eth_mid: Some(dec!(2100)),
            eth_mark: None,
            eth_close: None,
            btc_mid: Some(dec!(31000)),
            btc_mark: None,
            btc_close: None,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        })
        .unwrap();

    let records = store.load_range(t1, t3).unwrap();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].timestamp, t1);
    assert_eq!(records[1].timestamp, t2);
    assert_eq!(records[2].timestamp, t3);
}
