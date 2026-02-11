use rust_decimal_macros::dec;

use eth_btc_strategy::config::{OrderType, Symbol};
use eth_btc_strategy::execution::{
    ExecutionEngine, ExecutionError, MockOrderExecutor, OrderRequest, OrderSide, RetryConfig,
};

use std::collections::VecDeque;
use std::sync::Mutex;

fn order(symbol: Symbol, side: OrderSide) -> OrderRequest {
    OrderRequest {
        symbol,
        side,
        qty: dec!(1),
        order_type: OrderType::Market,
        limit_price: None,
    }
}

#[derive(Default)]
struct RecordingExecutor {
    close_responses: Mutex<VecDeque<Result<rust_decimal::Decimal, ExecutionError>>>,
    submit_responses: Mutex<VecDeque<Result<rust_decimal::Decimal, ExecutionError>>>,
    submitted: Mutex<Vec<OrderRequest>>,
}

#[async_trait::async_trait]
impl eth_btc_strategy::execution::OrderExecutor for RecordingExecutor {
    async fn submit(&self, order: &OrderRequest) -> Result<rust_decimal::Decimal, ExecutionError> {
        self.submitted
            .lock()
            .expect("submit lock")
            .push(order.clone());
        self.submit_responses
            .lock()
            .expect("submit responses lock")
            .pop_front()
            .unwrap_or_else(|| Ok(order.qty))
    }

    async fn close(&self, _order: &OrderRequest) -> Result<rust_decimal::Decimal, ExecutionError> {
        self.close_responses
            .lock()
            .expect("close responses lock")
            .pop_front()
            .unwrap_or_else(|| Ok(dec!(1)))
    }
}

#[tokio::test]
async fn close_pair_closes_both_legs() {
    let mut executor = MockOrderExecutor::default();
    executor.push_close_response(Symbol::EthPerp, Ok(dec!(1)));
    executor.push_close_response(Symbol::BtcPerp, Ok(dec!(1)));

    let engine = ExecutionEngine::new(std::sync::Arc::new(executor), RetryConfig::fast());
    let result = engine
        .close_pair(
            order(Symbol::EthPerp, OrderSide::Buy),
            order(Symbol::BtcPerp, OrderSide::Sell),
        )
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn close_pair_rolls_back_on_second_leg_failure() {
    let mut executor = MockOrderExecutor::default();
    executor.push_close_response(Symbol::EthPerp, Ok(dec!(1)));
    executor.push_close_response(
        Symbol::BtcPerp,
        Err(eth_btc_strategy::execution::ExecutionError::Fatal(
            "btc close failed".to_string(),
        )),
    );
    executor.push_submit_response(Symbol::EthPerp, Ok(dec!(1)));

    let engine = ExecutionEngine::new(std::sync::Arc::new(executor), RetryConfig::fast());
    let result = engine
        .close_pair(
            order(Symbol::EthPerp, OrderSide::Buy),
            order(Symbol::BtcPerp, OrderSide::Sell),
        )
        .await;

    match result {
        Err(eth_btc_strategy::execution::ExecutionError::PartialFill(message)) => {
            assert!(message.contains("rollback executed"));
        }
        other => panic!("unexpected result: {:?}", other),
    }
}

#[tokio::test]
async fn close_pair_rolls_back_with_filled_qty() {
    let executor = std::sync::Arc::new(RecordingExecutor::default());
    executor
        .close_responses
        .lock()
        .expect("close responses lock")
        .extend([
            Ok(dec!(0.4)),
            Err(ExecutionError::Fatal("btc close failed".to_string())),
        ]);
    executor
        .submit_responses
        .lock()
        .expect("submit responses lock")
        .push_back(Ok(dec!(0.4)));

    let engine = ExecutionEngine::new(executor.clone(), RetryConfig::fast());
    let result = engine
        .close_pair(
            order(Symbol::EthPerp, OrderSide::Buy),
            order(Symbol::BtcPerp, OrderSide::Sell),
        )
        .await;

    assert!(matches!(result, Err(ExecutionError::PartialFill(_))));
    let submitted = executor.submitted.lock().expect("submit lock");
    assert_eq!(submitted.len(), 1);
    assert_eq!(submitted[0].qty, dec!(0.4));
}
