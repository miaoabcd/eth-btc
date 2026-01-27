use rust_decimal_macros::dec;

use eth_btc_strategy::config::{OrderType, Symbol};
use eth_btc_strategy::execution::{
    ExecutionEngine, ExecutionError, MockOrderExecutor, OrderRequest, OrderSide, RetryConfig,
};

fn order(symbol: Symbol, side: OrderSide) -> OrderRequest {
    OrderRequest {
        symbol,
        side,
        qty: dec!(1),
        order_type: OrderType::Market,
        limit_price: None,
    }
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
