use std::collections::VecDeque;
use std::sync::Mutex;

use rust_decimal_macros::dec;

use eth_btc_strategy::config::{OrderType, Symbol};
use eth_btc_strategy::execution::{
    ExecutionError, LiveOrderExecutor, OrderExecutor, OrderHttpClient, OrderHttpResponse,
    OrderRequest, OrderSide,
};

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

impl MockOrderHttpClient {
    fn push_response(&self, response: OrderHttpResponse) {
        self.responses
            .lock()
            .expect("responses lock")
            .push_back(response);
    }

    fn last_request(&self) -> Option<RecordedRequest> {
        self.requests.lock().expect("requests lock").last().cloned()
    }
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
        limit_price: None,
    }
}

#[tokio::test]
async fn live_executor_posts_order_payload() {
    let client = std::sync::Arc::new(MockOrderHttpClient::default());
    client.push_response(OrderHttpResponse {
        status: 200,
        body: r#"{"filled_qty":"1.25"}"#.to_string(),
    });

    let executor = LiveOrderExecutor::with_client("http://localhost", client.clone());
    let filled = executor.submit(&order()).await.unwrap();

    assert_eq!(filled, dec!(1.25));

    let recorded = client.last_request().expect("recorded request");
    assert_eq!(recorded.url, "http://localhost/v1/orders");
    assert_eq!(recorded.body["symbol"], "ETH_PERP");
    assert_eq!(recorded.body["side"], "BUY");
    match &recorded.body["qty"] {
        serde_json::Value::String(value) => assert_eq!(value, "1.25"),
        serde_json::Value::Number(value) => assert_eq!(value.to_string(), "1.25"),
        _ => panic!("unexpected qty format"),
    }
}

#[tokio::test]
async fn live_executor_maps_server_errors_to_transient() {
    let client = std::sync::Arc::new(MockOrderHttpClient::default());
    client.push_response(OrderHttpResponse {
        status: 500,
        body: "oops".to_string(),
    });

    let executor = LiveOrderExecutor::with_client("http://localhost", client);
    let result = executor.close(&order()).await;

    assert!(matches!(result, Err(ExecutionError::Transient(_))));
}
