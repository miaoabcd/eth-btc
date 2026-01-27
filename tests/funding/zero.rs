use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;

use eth_btc_strategy::config::Symbol;
use eth_btc_strategy::funding::{FundingSource, ZeroFundingSource};

#[tokio::test]
async fn zero_funding_source_returns_zero_rates() {
    let source = ZeroFundingSource::default();
    let timestamp = Utc.timestamp_opt(0, 0).unwrap();

    let rate = source.fetch_rate(Symbol::EthPerp, timestamp).await.unwrap();

    assert_eq!(rate.rate, Decimal::ZERO);
    assert_eq!(rate.interval_hours, 8);
}
