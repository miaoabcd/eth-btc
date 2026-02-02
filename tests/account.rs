use std::sync::Arc;

use async_trait::async_trait;
use rust_decimal_macros::dec;

use eth_btc_strategy::account::{
    AccountBalanceSource, AccountHttpClient, AccountHttpResponse, HyperliquidAccountSource,
};
use eth_btc_strategy::util::rate_limiter::NoopRateLimiter;

#[derive(Clone)]
struct StaticAccountClient {
    status: u16,
    body: String,
}

#[async_trait]
impl AccountHttpClient for StaticAccountClient {
    async fn post(
        &self,
        _url: &str,
        _body: serde_json::Value,
    ) -> Result<AccountHttpResponse, eth_btc_strategy::account::AccountError> {
        Ok(AccountHttpResponse {
            status: self.status,
            body: self.body.clone(),
        })
    }
}

#[tokio::test]
async fn account_source_prefers_total_raw_usd() {
    let body = serde_json::json!({
        "data": {
            "marginSummary": {
                "totalRawUsd": "100",
                "availableBalance": "10"
            }
        }
    })
    .to_string();
    let client = StaticAccountClient { status: 200, body };
    let source = HyperliquidAccountSource::with_client_and_rate_limiter(
        "https://api.hyperliquid.xyz",
        "0x0000000000000000000000000000000000000000",
        Arc::new(client),
        Arc::new(NoopRateLimiter),
    );

    let balance = source.fetch_available_balance().await.unwrap();
    assert_eq!(balance, dec!(100));
}
