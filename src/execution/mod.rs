use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::time::sleep;

use crate::config::{OrderType, Symbol};
use crate::state::PositionSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderSide {
    Buy,
    Sell,
}

impl OrderSide {
    pub fn close_for_qty(qty: Decimal) -> OrderSide {
        if qty > Decimal::ZERO {
            OrderSide::Sell
        } else {
            OrderSide::Buy
        }
    }
}

#[derive(Debug, Clone)]
pub struct OrderRequest {
    pub symbol: Symbol,
    pub side: OrderSide,
    pub qty: Decimal,
    pub order_type: OrderType,
    pub limit_price: Option<Decimal>,
}

#[derive(Debug, Clone)]
pub struct OrderHttpResponse {
    pub status: u16,
    pub body: String,
}

#[async_trait::async_trait]
pub trait OrderHttpClient: Send + Sync {
    async fn post(&self, url: &str, body: Value) -> Result<OrderHttpResponse, ExecutionError>;
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum ExecutionError {
    #[error("transient error: {0}")]
    Transient(String),
    #[error("fatal error: {0}")]
    Fatal(String),
    #[error("partial fill: {0}")]
    PartialFill(String),
}

impl ExecutionError {
    fn is_transient(&self) -> bool {
        matches!(self, ExecutionError::Transient(_))
    }
}

#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts: usize,
    pub base_delay_ms: u64,
}

impl RetryConfig {
    pub fn fast() -> Self {
        Self {
            max_attempts: 2,
            base_delay_ms: 1,
        }
    }
}

#[async_trait::async_trait]
pub trait OrderExecutor: Send + Sync {
    async fn submit(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError>;
    async fn close(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError>;
}

#[derive(Clone)]
pub struct ReqwestOrderClient {
    client: reqwest::Client,
    api_key: Option<String>,
}

impl ReqwestOrderClient {
    pub fn new(api_key: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
        }
    }
}

#[async_trait::async_trait]
impl OrderHttpClient for ReqwestOrderClient {
    async fn post(&self, url: &str, body: Value) -> Result<OrderHttpResponse, ExecutionError> {
        let mut request = self.client.post(url).json(&body);
        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }
        let response = request
            .send()
            .await
            .map_err(|err| ExecutionError::Transient(err.to_string()))?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|err| ExecutionError::Transient(err.to_string()))?;
        Ok(OrderHttpResponse { status, body })
    }
}

#[derive(Debug, Serialize)]
struct LiveOrderPayload {
    symbol: Symbol,
    side: OrderSide,
    qty: Decimal,
    order_type: OrderType,
    limit_price: Option<Decimal>,
    reduce_only: bool,
}

#[derive(Debug, Deserialize)]
struct LiveOrderResponse {
    filled_qty: Decimal,
}

#[derive(Clone)]
pub struct LiveOrderExecutor {
    base_url: String,
    client: Arc<dyn OrderHttpClient>,
}

impl LiveOrderExecutor {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_client(base_url, Arc::new(ReqwestOrderClient::new(None)))
    }

    pub fn with_api_key(base_url: impl Into<String>, api_key: String) -> Self {
        Self::with_client(base_url, Arc::new(ReqwestOrderClient::new(Some(api_key))))
    }

    pub fn with_client(base_url: impl Into<String>, client: Arc<dyn OrderHttpClient>) -> Self {
        Self {
            base_url: base_url.into(),
            client,
        }
    }

    fn order_url(&self) -> String {
        format!("{}/v1/orders", self.base_url.trim_end_matches('/'))
    }

    async fn post_order(
        &self,
        order: &OrderRequest,
        reduce_only: bool,
    ) -> Result<Decimal, ExecutionError> {
        let payload = LiveOrderPayload {
            symbol: order.symbol,
            side: order.side,
            qty: order.qty,
            order_type: order.order_type,
            limit_price: order.limit_price,
            reduce_only,
        };
        let body =
            serde_json::to_value(payload).map_err(|err| ExecutionError::Fatal(err.to_string()))?;
        let response = self.client.post(&self.order_url(), body).await?;
        if response.status >= 500 {
            return Err(ExecutionError::Transient(format!(
                "server error {status}",
                status = response.status
            )));
        }
        if response.status >= 400 {
            return Err(ExecutionError::Fatal(format!(
                "client error {status}",
                status = response.status
            )));
        }
        let parsed: LiveOrderResponse = serde_json::from_str(&response.body)
            .map_err(|err| ExecutionError::Fatal(err.to_string()))?;
        Ok(parsed.filled_qty)
    }
}

#[async_trait::async_trait]
impl OrderExecutor for LiveOrderExecutor {
    async fn submit(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError> {
        self.post_order(order, false).await
    }

    async fn close(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError> {
        self.post_order(order, true).await
    }
}

#[derive(Clone)]
pub struct ExecutionEngine {
    executor: Arc<dyn OrderExecutor>,
    retry: RetryConfig,
}

impl ExecutionEngine {
    pub fn new(executor: Arc<dyn OrderExecutor>, retry: RetryConfig) -> Self {
        Self { executor, retry }
    }

    pub async fn open_pair(
        &self,
        eth_order: OrderRequest,
        btc_order: OrderRequest,
    ) -> Result<(), ExecutionError> {
        let eth_fill = self.retry_submit(&eth_order).await;
        let eth_fill = match eth_fill {
            Ok(fill) => fill,
            Err(err) => return Err(err),
        };

        match self.retry_submit(&btc_order).await {
            Ok(_) => Ok(()),
            Err(err) => {
                let _ = self
                    .retry_close(&OrderRequest {
                        symbol: eth_order.symbol,
                        side: OrderSide::close_for_qty(eth_fill),
                        qty: eth_fill.abs(),
                        order_type: OrderType::Market,
                        limit_price: None,
                    })
                    .await;
                Err(ExecutionError::PartialFill(err.to_string()))
            }
        }
    }

    pub async fn close_pair(
        &self,
        eth_order: OrderRequest,
        btc_order: OrderRequest,
    ) -> Result<(), ExecutionError> {
        let eth_result = self.retry_close(&eth_order).await;
        if eth_result.is_err() {
            return eth_result.map(|_| ());
        }
        let btc_result = self.retry_close(&btc_order).await;
        if let Err(err) = btc_result {
            return Err(ExecutionError::PartialFill(err.to_string()));
        }
        Ok(())
    }

    pub async fn repair_residual(&self, position: &PositionSnapshot) -> Result<(), ExecutionError> {
        if position.eth.qty != Decimal::ZERO && position.btc.qty == Decimal::ZERO {
            let order = OrderRequest {
                symbol: Symbol::EthPerp,
                side: OrderSide::close_for_qty(position.eth.qty),
                qty: position.eth.qty.abs(),
                order_type: OrderType::Market,
                limit_price: None,
            };
            return self.retry_close(&order).await.map(|_| ());
        }
        if position.btc.qty != Decimal::ZERO && position.eth.qty == Decimal::ZERO {
            let order = OrderRequest {
                symbol: Symbol::BtcPerp,
                side: OrderSide::close_for_qty(position.btc.qty),
                qty: position.btc.qty.abs(),
                order_type: OrderType::Market,
                limit_price: None,
            };
            return self.retry_close(&order).await.map(|_| ());
        }
        Ok(())
    }

    async fn retry_submit(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError> {
        self.retry_with(|| self.executor.submit(order)).await
    }

    async fn retry_close(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError> {
        self.retry_with(|| self.executor.close(order)).await
    }

    async fn retry_with<F, Fut>(&self, mut action: F) -> Result<Decimal, ExecutionError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<Decimal, ExecutionError>>,
    {
        let mut delay = self.retry.base_delay_ms;
        for attempt in 0..self.retry.max_attempts {
            match action().await {
                Ok(value) => return Ok(value),
                Err(err) => {
                    if err.is_transient() && attempt + 1 < self.retry.max_attempts {
                        sleep(Duration::from_millis(delay)).await;
                        delay = delay.saturating_mul(2);
                        continue;
                    }
                    return Err(err);
                }
            }
        }
        Err(ExecutionError::Transient(
            "retry attempts exhausted".to_string(),
        ))
    }
}

#[derive(Default)]
pub struct MockOrderExecutor {
    submit_responses: Mutex<HashMap<Symbol, VecDeque<Result<Decimal, ExecutionError>>>>,
    close_responses: Mutex<HashMap<Symbol, VecDeque<Result<Decimal, ExecutionError>>>>,
}

impl MockOrderExecutor {
    pub fn push_submit_response(
        &mut self,
        symbol: Symbol,
        response: Result<Decimal, ExecutionError>,
    ) {
        let queue = self
            .submit_responses
            .get_mut()
            .expect("mock submit lock poisoned")
            .entry(symbol)
            .or_default();
        queue.push_back(response);
    }

    pub fn push_close_response(
        &mut self,
        symbol: Symbol,
        response: Result<Decimal, ExecutionError>,
    ) {
        let queue = self
            .close_responses
            .get_mut()
            .expect("mock close lock poisoned")
            .entry(symbol)
            .or_default();
        queue.push_back(response);
    }

    fn pop_response(
        store: &Mutex<HashMap<Symbol, VecDeque<Result<Decimal, ExecutionError>>>>,
        symbol: Symbol,
    ) -> Result<Decimal, ExecutionError> {
        let mut guard = store.lock().expect("mock lock poisoned");
        let queue = guard.entry(symbol).or_default();
        queue
            .pop_front()
            .unwrap_or_else(|| Err(ExecutionError::Fatal("no mock response".to_string())))
    }
}

#[async_trait::async_trait]
impl OrderExecutor for MockOrderExecutor {
    async fn submit(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError> {
        Self::pop_response(&self.submit_responses, order.symbol)
    }

    async fn close(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError> {
        Self::pop_response(&self.close_responses, order.symbol)
    }
}

#[derive(Debug, Default, Clone)]
pub struct PaperOrderExecutor;

#[async_trait::async_trait]
impl OrderExecutor for PaperOrderExecutor {
    async fn submit(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError> {
        Ok(order.qty)
    }

    async fn close(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError> {
        Ok(order.qty)
    }
}
