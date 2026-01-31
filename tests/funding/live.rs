use std::sync::Arc;

use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;
use serde_json::json;

use eth_btc_strategy::config::Symbol;
use eth_btc_strategy::funding::{
    FundingError, FundingHttpClient, FundingHttpResponse, FundingSource, HyperliquidFundingSource,
};

#[derive(Debug, Clone)]
struct TestHttpClient {
    expected_url: String,
    expected_body: serde_json::Value,
    response: FundingHttpResponse,
}

#[async_trait::async_trait]
impl FundingHttpClient for TestHttpClient {
    async fn post(
        &self,
        url: &str,
        body: serde_json::Value,
    ) -> Result<FundingHttpResponse, FundingError> {
        assert_eq!(url, self.expected_url);
        assert_eq!(body, self.expected_body);
        Ok(self.response.clone())
    }
}

#[tokio::test]
async fn hyperliquid_funding_source_fetch_rate_parses_response() {
    let timestamp = Utc.timestamp_opt(0, 0).unwrap();
    let body = json!({
        "data": [
            {
                "universe": [
                    {"name": "ETH"},
                    {"name": "BTC"}
                ]
            },
            [
                {"funding": "0.01"},
                {"funding": "0.02"}
            ]
        ]
    })
    .to_string();

    let client = TestHttpClient {
        expected_url: "http://localhost/info".to_string(),
        expected_body: json!({
            "type": "metaAndAssetCtxs"
        }),
        response: FundingHttpResponse { status: 200, body },
    };

    let source = HyperliquidFundingSource::with_client("http://localhost", Arc::new(client));
    let rate = source.fetch_rate(Symbol::EthPerp, timestamp).await.unwrap();

    assert_eq!(rate.rate, dec!(0.01));
    assert_eq!(rate.interval_hours, 1);
}
