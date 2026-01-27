use rust_decimal_macros::dec;

use eth_btc_strategy::config::{OrderType, Symbol};
use eth_btc_strategy::execution::{OrderExecutor, OrderRequest, OrderSide, PaperOrderExecutor};

fn order(symbol: Symbol, side: OrderSide, qty: rust_decimal::Decimal) -> OrderRequest {
    OrderRequest {
        symbol,
        side,
        qty,
        order_type: OrderType::Market,
        limit_price: None,
    }
}

#[tokio::test]
async fn paper_executor_echoes_qty() {
    let executor = PaperOrderExecutor;
    let order = order(Symbol::EthPerp, OrderSide::Buy, dec!(1.25));

    let submitted = executor.submit(&order).await.unwrap();
    let closed = executor.close(&order).await.unwrap();

    assert_eq!(submitted, dec!(1.25));
    assert_eq!(closed, dec!(1.25));
}
