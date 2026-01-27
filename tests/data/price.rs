use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::config::{PriceField, Symbol};
use eth_btc_strategy::data::{DataError, MockPriceSource, PriceBar, PriceHistory, PriceSource};

#[test]
fn price_field_fallback_chain() {
    let bar = PriceBar::new(
        Symbol::EthPerp,
        Utc.timestamp_opt(0, 0).unwrap(),
        Some(dec!(100.0)),
        Some(dec!(101.0)),
        Some(dec!(102.0)),
    );
    assert_eq!(bar.effective_price(PriceField::Mid), Some(dec!(100.0)));

    let bar = PriceBar::new(
        Symbol::EthPerp,
        Utc.timestamp_opt(0, 0).unwrap(),
        None,
        Some(dec!(101.0)),
        Some(dec!(102.0)),
    );
    assert_eq!(bar.effective_price(PriceField::Mid), Some(dec!(101.0)));

    let bar = PriceBar::new(
        Symbol::EthPerp,
        Utc.timestamp_opt(0, 0).unwrap(),
        None,
        None,
        Some(dec!(102.0)),
    );
    assert_eq!(bar.effective_price(PriceField::Mid), Some(dec!(102.0)));

    let bar = PriceBar::new(
        Symbol::EthPerp,
        Utc.timestamp_opt(0, 0).unwrap(),
        None,
        None,
        None,
    );
    assert_eq!(bar.effective_price(PriceField::Mid), None);
}

#[test]
fn price_bar_validation_rejects_non_positive_prices() {
    let bar = PriceBar::new(
        Symbol::EthPerp,
        Utc.timestamp_opt(0, 0).unwrap(),
        Some(dec!(0.0)),
        None,
        None,
    );
    assert!(matches!(bar.validate(), Err(DataError::InvalidPrice(_))));

    let bar = PriceBar::new(
        Symbol::EthPerp,
        Utc.timestamp_opt(0, 0).unwrap(),
        Some(dec!(-1.0)),
        None,
        None,
    );
    assert!(matches!(bar.validate(), Err(DataError::InvalidPrice(_))));
}

#[test]
fn price_history_ring_buffer_and_warmup() {
    let mut history = PriceHistory::new(2);
    let bar1 = PriceBar::new(
        Symbol::EthPerp,
        Utc.timestamp_opt(1, 0).unwrap(),
        Some(dec!(100.0)),
        None,
        None,
    );
    let bar2 = PriceBar::new(
        Symbol::EthPerp,
        Utc.timestamp_opt(2, 0).unwrap(),
        Some(dec!(101.0)),
        None,
        None,
    );
    let bar3 = PriceBar::new(
        Symbol::EthPerp,
        Utc.timestamp_opt(3, 0).unwrap(),
        Some(dec!(102.0)),
        None,
        None,
    );

    history.push(bar1);
    history.push(bar2);
    assert_eq!(history.len(), 2);
    assert!(history.is_warmed_up(2));

    history.push(bar3);
    assert_eq!(history.len(), 2);
    assert_eq!(history.get(0).unwrap().mid, Some(dec!(102.0)));
    assert_eq!(history.get(1).unwrap().mid, Some(dec!(101.0)));
    assert!(history.get(2).is_none());
}

#[tokio::test]
async fn mock_price_source_fetch_behaviors() {
    let mut source = MockPriceSource::default();
    let timestamp = Utc.timestamp_opt(10, 0).unwrap();
    let bar = PriceBar::new(Symbol::BtcPerp, timestamp, Some(dec!(200.0)), None, None);
    source.insert_bar(bar.clone());

    let fetched = source.fetch_bar(Symbol::BtcPerp, timestamp).await.unwrap();
    assert_eq!(fetched, bar);

    let missing = source
        .fetch_bar(Symbol::BtcPerp, Utc.timestamp_opt(11, 0).unwrap())
        .await;
    assert!(matches!(missing, Err(DataError::MissingData(_))));

    source.insert_error(Symbol::BtcPerp, timestamp, DataError::RateLimited);
    let rate_limited = source.fetch_bar(Symbol::BtcPerp, timestamp).await;
    assert!(matches!(rate_limited, Err(DataError::RateLimited)));
}
