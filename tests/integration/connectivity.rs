use std::sync::Arc;

use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::config::{PriceField, Symbol};
use eth_btc_strategy::data::{MockPriceSource, PriceBar, PriceFetcher};
use eth_btc_strategy::integration::api_connectivity_ok;

#[tokio::test]
async fn api_connectivity_ok_returns_true_when_prices_available() {
    let timestamp = Utc.timestamp_opt(0, 0).unwrap();
    let mut source = MockPriceSource::default();
    source.insert_bar(PriceBar::new(
        Symbol::EthPerp,
        timestamp,
        Some(dec!(100)),
        None,
        None,
    ));
    source.insert_bar(PriceBar::new(
        Symbol::BtcPerp,
        timestamp,
        Some(dec!(200)),
        None,
        None,
    ));
    let fetcher = PriceFetcher::new(Arc::new(source), PriceField::Mid);

    assert!(api_connectivity_ok(&fetcher, timestamp).await);
}

#[tokio::test]
async fn api_connectivity_ok_returns_false_on_error() {
    let timestamp = Utc.timestamp_opt(0, 0).unwrap();
    let fetcher = PriceFetcher::new(Arc::new(MockPriceSource::default()), PriceField::Mid);

    assert!(!api_connectivity_ok(&fetcher, timestamp).await);
}
