use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rust_decimal_macros::dec;

use eth_btc_strategy::account::{
    AccountBalanceSource, AccountHttpClient, AccountHttpResponse, AccountPositionSource,
    HyperliquidAccountSource,
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

#[derive(Clone)]
struct CapturingAccountClient {
    status: u16,
    body: String,
    last_body: Arc<Mutex<Option<serde_json::Value>>>,
}

#[async_trait]
impl AccountHttpClient for CapturingAccountClient {
    async fn post(
        &self,
        _url: &str,
        body: serde_json::Value,
    ) -> Result<AccountHttpResponse, eth_btc_strategy::account::AccountError> {
        let mut guard = self.last_body.lock().expect("capture lock");
        *guard = Some(body);
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

#[tokio::test]
async fn account_source_uses_clearinghouse_state_request_type() {
    let body = serde_json::json!({
        "data": {
            "marginSummary": {
                "totalRawUsd": "100"
            }
        }
    })
    .to_string();
    let captured = Arc::new(Mutex::new(None));
    let client = CapturingAccountClient {
        status: 200,
        body,
        last_body: Arc::clone(&captured),
    };
    let source = HyperliquidAccountSource::with_client_and_rate_limiter(
        "https://api.hyperliquid.xyz",
        "0x0000000000000000000000000000000000000000",
        Arc::new(client),
        Arc::new(NoopRateLimiter),
    );

    source.fetch_available_balance().await.unwrap();

    let request = captured
        .lock()
        .expect("capture lock")
        .clone()
        .expect("request captured");
    assert_eq!(
        request.get("type").and_then(|value| value.as_str()),
        Some("clearinghouseState")
    );
}

#[tokio::test]
async fn account_source_parses_pair_exposure() {
    let body = serde_json::json!({
        "data": {
            "marginSummary": {
                "totalRawUsd": "100"
            },
            "assetPositions": [
                {
                    "position": {
                        "coin": "ETH",
                        "szi": "-0.088",
                        "entryPx": "2077.03",
                        "positionValue": "182.77864"
                    }
                },
                {
                    "position": {
                        "coin": "BTC",
                        "szi": "0",
                        "entryPx": null,
                        "positionValue": "0"
                    }
                }
            ]
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

    let exposure = source.fetch_pair_exposure().await.unwrap();

    assert_eq!(exposure.eth_qty(), dec!(-0.088));
    assert_eq!(exposure.btc_qty(), dec!(0));
    assert!(exposure.has_residual());
}
