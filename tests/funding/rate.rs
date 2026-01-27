use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::config::Symbol;
use eth_btc_strategy::funding::{FundingFetcher, FundingRate, MockFundingSource};

#[tokio::test]
async fn funding_fetcher_returns_pair_rates() {
    let mut source = MockFundingSource::default();
    let timestamp = Utc.timestamp_opt(0, 0).unwrap();

    source.insert_rate(FundingRate {
        symbol: Symbol::EthPerp,
        rate: dec!(0.01),
        timestamp,
        interval_hours: 8,
    });
    source.insert_rate(FundingRate {
        symbol: Symbol::BtcPerp,
        rate: dec!(0.005),
        timestamp,
        interval_hours: 8,
    });

    let fetcher = FundingFetcher::new(std::sync::Arc::new(source));
    let snapshot = fetcher.fetch_pair_rates(timestamp).await.unwrap();

    assert_eq!(snapshot.eth.rate, dec!(0.01));
    assert_eq!(snapshot.btc.rate, dec!(0.005));
    assert_eq!(snapshot.interval_hours, 8);
}
