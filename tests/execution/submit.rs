use rust_decimal_macros::dec;

use eth_btc_strategy::config::{OrderType, Symbol};
use eth_btc_strategy::execution::{
    ExecutionEngine, ExecutionError, MockOrderExecutor, OrderExecutor, OrderRequest, OrderSide,
    OrderSubmitResult, RetryConfig,
};

fn order(symbol: Symbol, side: OrderSide) -> OrderRequest {
    OrderRequest {
        symbol,
        side,
        qty: dec!(1),
        order_type: OrderType::Market,
        limit_price: Some(dec!(1)),
        expires_after: None,
    }
}

#[derive(Default)]
struct RestingThenFailingExecutor {
    cancelled: std::sync::Mutex<Vec<(Symbol, u64)>>,
}

#[async_trait::async_trait]
impl OrderExecutor for RestingThenFailingExecutor {
    async fn submit(&self, _order: &OrderRequest) -> Result<rust_decimal::Decimal, ExecutionError> {
        Err(ExecutionError::Fatal("submit should not be used".into()))
    }

    async fn close(&self, _order: &OrderRequest) -> Result<rust_decimal::Decimal, ExecutionError> {
        Err(ExecutionError::Fatal("close should not be used".into()))
    }

    async fn submit_result(
        &self,
        order: &OrderRequest,
    ) -> Result<OrderSubmitResult, ExecutionError> {
        match order.symbol {
            Symbol::EthPerp => Ok(OrderSubmitResult::Resting { oid: 42 }),
            Symbol::BtcPerp => Err(ExecutionError::Fatal(
                "Post only order would have immediately matched".into(),
            )),
        }
    }

    async fn cancel(&self, symbol: Symbol, oid: u64) -> Result<(), ExecutionError> {
        self.cancelled
            .lock()
            .expect("cancel lock")
            .push((symbol, oid));
        Ok(())
    }
}

#[test]
fn retry_config_has_default() {
    let config = RetryConfig::default();
    assert!(config.max_attempts >= 1);
}

#[tokio::test]
async fn open_pair_retries_transient_failures() {
    let mut executor = MockOrderExecutor::default();
    executor.push_submit_response(
        Symbol::EthPerp,
        Err(ExecutionError::Transient("temp".into())),
    );
    executor.push_submit_response(Symbol::EthPerp, Ok(dec!(1)));
    executor.push_submit_response(Symbol::BtcPerp, Ok(dec!(1)));

    let engine = ExecutionEngine::new(std::sync::Arc::new(executor), RetryConfig::fast());
    let result = engine
        .open_pair(
            order(Symbol::EthPerp, OrderSide::Sell),
            order(Symbol::BtcPerp, OrderSide::Buy),
        )
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn open_pair_repairs_on_partial_fill() {
    let mut executor = MockOrderExecutor::default();
    executor.push_submit_response(Symbol::EthPerp, Ok(dec!(1)));
    executor.push_submit_response(Symbol::BtcPerp, Err(ExecutionError::Fatal("fail".into())));
    executor.push_close_response(Symbol::EthPerp, Ok(dec!(1)));

    let engine = ExecutionEngine::new(std::sync::Arc::new(executor), RetryConfig::fast());
    let result = engine
        .open_pair(
            order(Symbol::EthPerp, OrderSide::Sell),
            order(Symbol::BtcPerp, OrderSide::Buy),
        )
        .await;

    assert!(matches!(result, Err(ExecutionError::PartialFill(_))));
}

#[tokio::test]
async fn open_pair_reports_rollback_failure() {
    let mut executor = MockOrderExecutor::default();
    executor.push_submit_response(Symbol::EthPerp, Ok(dec!(1)));
    executor.push_submit_response(Symbol::BtcPerp, Err(ExecutionError::Fatal("fail".into())));
    executor.push_close_response(
        Symbol::EthPerp,
        Err(ExecutionError::Fatal("rollback close failed".into())),
    );

    let engine = ExecutionEngine::new(std::sync::Arc::new(executor), RetryConfig::fast());
    let result = engine
        .open_pair(
            order(Symbol::EthPerp, OrderSide::Sell),
            order(Symbol::BtcPerp, OrderSide::Buy),
        )
        .await;

    match result {
        Err(ExecutionError::PartialFill(message)) => {
            assert!(message.contains("rollback failed"));
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[tokio::test]
async fn open_pair_cancels_first_resting_leg_when_second_leg_fails() {
    let executor = std::sync::Arc::new(RestingThenFailingExecutor::default());
    let engine = ExecutionEngine::new(executor.clone(), RetryConfig::fast());
    let result = engine
        .open_pair(
            OrderRequest {
                order_type: OrderType::PostOnly,
                ..order(Symbol::EthPerp, OrderSide::Sell)
            },
            OrderRequest {
                order_type: OrderType::PostOnly,
                ..order(Symbol::BtcPerp, OrderSide::Buy)
            },
        )
        .await;

    assert!(
        matches!(result, Err(ExecutionError::Fatal(message)) if message.contains("first leg cancelled"))
    );
    assert_eq!(
        executor.cancelled.lock().expect("cancel lock").as_slice(),
        &[(Symbol::EthPerp, 42)]
    );
}

#[tokio::test]
async fn retry_with_zero_max_attempts_still_attempts_once() {
    let mut executor = MockOrderExecutor::default();
    executor.push_submit_response(Symbol::EthPerp, Ok(dec!(1)));
    executor.push_submit_response(Symbol::BtcPerp, Ok(dec!(1)));

    let engine = ExecutionEngine::new(
        std::sync::Arc::new(executor),
        RetryConfig {
            max_attempts: 0,
            base_delay_ms: 0,
        },
    );
    let result = engine
        .open_pair(
            order(Symbol::EthPerp, OrderSide::Sell),
            order(Symbol::BtcPerp, OrderSide::Buy),
        )
        .await;

    assert!(result.is_ok());
}
