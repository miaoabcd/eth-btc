use std::sync::Arc;

use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::config::{PriceField, Symbol};
use eth_btc_strategy::data::{MockPriceSource, PriceBar, PriceFetcher};

#[tokio::test]
async fn price_fetcher_falls_back_to_last_snapshot_when_all_prices_missing() {
    let t0 = Utc.timestamp_opt(0, 0).unwrap();
    let t1 = Utc.timestamp_opt(900, 0).unwrap();
    let mut source = MockPriceSource::default();
    source.insert_bar(PriceBar::new(
        Symbol::EthPerp,
        t0,
        Some(dec!(100)),
        None,
        None,
    ));
    source.insert_bar(PriceBar::new(
        Symbol::BtcPerp,
        t0,
        Some(dec!(200)),
        None,
        None,
    ));
    source.insert_bar(PriceBar::new(Symbol::EthPerp, t1, None, None, None));
    source.insert_bar(PriceBar::new(Symbol::BtcPerp, t1, None, None, None));

    let fetcher = PriceFetcher::new(Arc::new(source), PriceField::Mid);

    let first = fetcher.fetch_pair_prices(t0).await.unwrap();
    let fallback = fetcher.fetch_pair_prices(t1).await.unwrap();

    assert_eq!(fallback.eth, first.eth);
    assert_eq!(fallback.btc, first.btc);
}
