use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::core::TradeDirection;
use eth_btc_strategy::state::{
    PositionLeg, PositionSnapshot, StateStore, StrategyState, StrategyStatus,
};

#[test]
fn state_store_round_trip() {
    let store = StateStore::new_in_memory().unwrap();
    let state = StrategyState {
        status: StrategyStatus::InPosition,
        position: Some(PositionSnapshot {
            direction: TradeDirection::ShortEthLongBtc,
            entry_time: Utc.timestamp_opt(100, 0).unwrap(),
            eth: PositionLeg {
                qty: dec!(-1),
                avg_price: dec!(100),
                notional: dec!(100),
            },
            btc: PositionLeg {
                qty: dec!(1),
                avg_price: dec!(200),
                notional: dec!(200),
            },
        }),
        pending_entry: None,
        cooldown_until: None,
        cumulative_realized_pnl: dec!(0),
    };

    store.save(&state).unwrap();
    let loaded = store.load().unwrap().unwrap();

    assert_eq!(loaded, state);
}

#[test]
fn strategy_state_defaults_missing_cumulative_pnl() {
    let state: StrategyState = serde_json::from_str(
        r#"{"status":"Flat","position":null,"pending_entry":null,"cooldown_until":null}"#,
    )
    .unwrap();

    assert_eq!(state.cumulative_realized_pnl, dec!(0));
}
