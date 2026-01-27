use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::core::TradeDirection;
use eth_btc_strategy::state::{PositionLeg, PositionSnapshot};

#[test]
fn position_residual_detection() {
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

    assert!(position.has_residual());
    assert!(!position.is_flat());

    let position = PositionSnapshot {
        btc: PositionLeg {
            qty: dec!(1),
            avg_price: dec!(200),
            notional: dec!(200),
        },
        ..position
    };

    assert!(!position.has_residual());

    let flat = PositionSnapshot {
        eth: PositionLeg {
            qty: dec!(0),
            avg_price: dec!(0),
            notional: dec!(0),
        },
        btc: PositionLeg {
            qty: dec!(0),
            avg_price: dec!(0),
            notional: dec!(0),
        },
        ..position
    };

    assert!(flat.is_flat());
    assert!(!flat.has_residual());
}
