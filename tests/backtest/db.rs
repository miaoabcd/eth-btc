use std::path::Path;

use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::backtest::{BacktestBar, load_backtest_bars_from_db};
use eth_btc_strategy::config::PriceField;
use eth_btc_strategy::storage::{PriceBarRecord, PriceStore};

#[test]
fn backtest_loads_bars_from_sqlite() {
    let path = format!(
        "/tmp/prices-{}.sqlite",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let store = PriceStore::new(&path).unwrap();

    let t1 = Utc.timestamp_opt(0, 0).unwrap();
    let t2 = Utc.timestamp_opt(900, 0).unwrap();
    store
        .save(&PriceBarRecord {
            timestamp: t1,
            eth_mid: Some(dec!(2000)),
            eth_mark: None,
            eth_close: None,
            btc_mid: Some(dec!(30000)),
            btc_mark: None,
            btc_close: None,
            funding_eth: Some(dec!(0.0001)),
            funding_btc: Some(dec!(0.0002)),
            funding_interval_hours: Some(8),
        })
        .unwrap();
    store
        .save(&PriceBarRecord {
            timestamp: t2,
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

    let bars = load_backtest_bars_from_db(Path::new(&path), t1, t2, PriceField::Mid).unwrap();

    let expected = vec![
        BacktestBar {
            timestamp: t1,
            eth_price: dec!(2000),
            btc_price: dec!(30000),
            funding_eth: Some(dec!(0.0001)),
            funding_btc: Some(dec!(0.0002)),
        },
        BacktestBar {
            timestamp: t2,
            eth_price: dec!(2100),
            btc_price: dec!(31000),
            funding_eth: None,
            funding_btc: None,
        },
    ];

    assert_eq!(bars.len(), expected.len());
    assert_eq!(bars[0], expected[0]);
    assert_eq!(bars[1], expected[1]);
    let _ = std::fs::remove_file(path);
}
