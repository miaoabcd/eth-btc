use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::config::Symbol;
use eth_btc_strategy::core::TradeDirection;
use eth_btc_strategy::execution::{ExecutionEngine, MockOrderExecutor, RetryConfig};
use eth_btc_strategy::state::{PositionLeg, PositionSnapshot};

#[tokio::test]
async fn residual_repair_closes_remaining_leg() {
    let position = PositionSnapshot {
        direction: TradeDirection::LongEthShortBtc,
        entry_time: Utc.timestamp_opt(0, 0).unwrap(),
        eth: PositionLeg {
            qty: dec!(1),
            avg_price: dec!(100),
            notional: dec!(100),
        },
        btc: PositionLeg {
            qty: dec!(0),
            avg_price: dec!(200),
            notional: dec!(0),
        },
    };

    let mut executor = MockOrderExecutor::default();
    executor.push_close_response(Symbol::EthPerp, Ok(dec!(1)));

    let engine = ExecutionEngine::new(std::sync::Arc::new(executor), RetryConfig::fast());
    let result = engine.repair_residual(&position).await;

    assert!(result.is_ok());
}
