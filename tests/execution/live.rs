use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use rust_decimal_macros::dec;

use eth_btc_strategy::config::{OrderType, Symbol};
use eth_btc_strategy::execution::{
    ExecutionError, HyperliquidSigner, LiveOrderExecutor, NonceProvider, OrderExecutor,
    OrderHttpClient, OrderHttpResponse, OrderRequest, OrderSide,
};
use eth_btc_strategy::util::rate_limiter::RateLimiter;

#[derive(Debug, Clone)]
struct RecordedRequest {
    url: String,
    body: serde_json::Value,
}

#[derive(Default)]
struct MockOrderHttpClient {
    responses: Mutex<VecDeque<OrderHttpResponse>>,
    requests: Mutex<Vec<RecordedRequest>>,
}

#[derive(Default)]
struct FixedNonce {
    value: u64,
}

impl FixedNonce {
    fn new(value: u64) -> Self {
        Self { value }
    }
}

impl NonceProvider for FixedNonce {
    fn next_nonce(&self) -> u64 {
        self.value
    }
}

#[derive(Default)]
struct CountingRateLimiter {
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl RateLimiter for CountingRateLimiter {
    async fn wait(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

impl MockOrderHttpClient {
    fn push_response(&self, response: OrderHttpResponse) {
        self.responses
            .lock()
            .expect("responses lock")
            .push_back(response);
    }
}

fn signed_executor(client: std::sync::Arc<MockOrderHttpClient>) -> LiveOrderExecutor {
    let signer = HyperliquidSigner::new(
        "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
    );
    let nonce = std::sync::Arc::new(FixedNonce::new(1_700_000_000_000));
    LiveOrderExecutor::with_client("http://localhost", client)
        .with_signer(signer)
        .with_nonce_provider(nonce)
}

#[async_trait::async_trait]
impl OrderHttpClient for MockOrderHttpClient {
    async fn post(
        &self,
        url: &str,
        body: serde_json::Value,
    ) -> Result<OrderHttpResponse, ExecutionError> {
        self.requests
            .lock()
            .expect("requests lock")
            .push(RecordedRequest {
                url: url.to_string(),
                body,
            });
        self.responses
            .lock()
            .expect("responses lock")
            .pop_front()
            .ok_or_else(|| ExecutionError::Fatal("no response queued".to_string()))
    }
}

fn order() -> OrderRequest {
    OrderRequest {
        symbol: Symbol::EthPerp,
        side: OrderSide::Buy,
        qty: dec!(1.25),
        order_type: OrderType::Market,
        limit_price: Some(dec!(2010.0)),
    }
}

#[tokio::test]
async fn live_executor_posts_order_payload() {
    let client = std::sync::Arc::new(MockOrderHttpClient::default());
    client.push_response(OrderHttpResponse {
        status: 200,
        body: r#"{"universe":[{"name":"ETH","szDecimals":3},{"name":"BTC","szDecimals":3}]}"#
            .to_string(),
    });
    client.push_response(OrderHttpResponse {
        status: 200,
        body: r#"{"status":"ok","response":{"type":"order","data":{"statuses":[{"filled":{"totalSz":"1.25","avgPx":"2000","oid":1}}]}}}"#
            .to_string(),
    });

    let executor = signed_executor(client.clone());
    let filled = executor.submit(&order()).await.unwrap();

    assert_eq!(filled, dec!(1.25));

    let requests = client.requests.lock().expect("requests lock");
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].url, "http://localhost/info");
    assert_eq!(requests[0].body, serde_json::json!({"type":"meta"}));

    let recorded = requests.last().expect("recorded request");
    assert_eq!(recorded.url, "http://localhost/exchange");
    assert_eq!(recorded.body["nonce"], 1_700_000_000_000u64);
    assert_eq!(recorded.body["action"]["type"], "order");
    assert_eq!(recorded.body["action"]["orders"][0]["a"], 0);
    assert_eq!(recorded.body["action"]["orders"][0]["b"], true);
    match &recorded.body["action"]["orders"][0]["s"] {
        serde_json::Value::String(value) => assert_eq!(value, "1.25"),
        serde_json::Value::Number(value) => assert_eq!(value.to_string(), "1.25"),
        _ => panic!("unexpected qty format"),
    }
    match &recorded.body["action"]["orders"][0]["p"] {
        serde_json::Value::String(value) => assert_eq!(value, "2010.0"),
        serde_json::Value::Number(value) => assert_eq!(value.to_string(), "2010.0"),
        _ => panic!("unexpected price format"),
    }
    let sig_r = recorded.body["signature"]["r"].as_str().expect("r");
    let sig_s = recorded.body["signature"]["s"].as_str().expect("s");
    let sig_v = recorded.body["signature"]["v"].as_u64().expect("v");
    assert!(sig_r.starts_with("0x") && sig_r.len() == 66);
    assert!(sig_s.starts_with("0x") && sig_s.len() == 66);
    assert!(sig_v == 27 || sig_v == 28);
}

#[tokio::test]
async fn live_executor_maps_server_errors_to_transient() {
    let client = std::sync::Arc::new(MockOrderHttpClient::default());
    client.push_response(OrderHttpResponse {
        status: 200,
        body: r#"{"universe":[{"name":"ETH","szDecimals":3},{"name":"BTC","szDecimals":3}]}"#
            .to_string(),
    });
    client.push_response(OrderHttpResponse {
        status: 500,
        body: "oops".to_string(),
    });

    let executor = signed_executor(client);
    let result = executor.close(&order()).await;

    assert!(matches!(result, Err(ExecutionError::Transient(_))));
}

#[tokio::test]
async fn live_executor_applies_rate_limiter() {
    let client = std::sync::Arc::new(MockOrderHttpClient::default());
    client.push_response(OrderHttpResponse {
        status: 200,
        body: r#"{"universe":[{"name":"ETH","szDecimals":3},{"name":"BTC","szDecimals":3}]}"#
            .to_string(),
    });
    client.push_response(OrderHttpResponse {
        status: 200,
        body: r#"{"status":"ok","response":{"type":"order","data":{"statuses":[{"filled":{"totalSz":"1.25","avgPx":"2000","oid":1}}]}}}"#
            .to_string(),
    });
    let limiter = std::sync::Arc::new(CountingRateLimiter::default());

    let executor =
        LiveOrderExecutor::with_client_and_rate_limiter("http://localhost", client, limiter.clone())
            .with_signer(HyperliquidSigner::new(
                "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
            ))
            .with_nonce_provider(std::sync::Arc::new(FixedNonce::new(1_700_000_000_000)));
    executor.submit(&order()).await.unwrap();

    assert_eq!(limiter.calls.load(Ordering::SeqCst), 1);
}
