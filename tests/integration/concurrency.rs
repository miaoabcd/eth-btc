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
async fn concurrency_safety_for_execution() {
    let mut executor = MockOrderExecutor::default();
    executor.push_submit_response(Symbol::EthPerp, Ok(dec!(1)));
    executor.push_submit_response(Symbol::EthPerp, Ok(dec!(1)));
    executor.push_submit_response(Symbol::BtcPerp, Ok(dec!(1)));
    executor.push_submit_response(Symbol::BtcPerp, Ok(dec!(1)));

    let engine = ExecutionEngine::new(std::sync::Arc::new(executor), RetryConfig::fast());
    let engine2 = engine.clone();

    let task1 = tokio::spawn(async move {
        engine
            .open_pair(
                order(Symbol::EthPerp, OrderSide::Sell),
                order(Symbol::BtcPerp, OrderSide::Buy),
            )
            .await
            .unwrap();
    });
    let task2 = tokio::spawn(async move {
        engine2
            .open_pair(
                order(Symbol::EthPerp, OrderSide::Sell),
                order(Symbol::BtcPerp, OrderSide::Buy),
            )
            .await
            .unwrap();
    });

    task1.await.unwrap();
    task2.await.unwrap();
}
