use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use rust_decimal::Decimal;
use thiserror::Error;
use tokio::time::sleep;

use crate::config::{OrderType, Symbol};
use crate::state::PositionSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
