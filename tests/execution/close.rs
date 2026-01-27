use rust_decimal_macros::dec;

use eth_btc_strategy::config::{OrderType, Symbol};
use eth_btc_strategy::execution::{
    ExecutionEngine, MockOrderExecutor, OrderRequest, OrderSide, RetryConfig,
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
