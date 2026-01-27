use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::backtest::{Trade, TradeExitReason, breakdown_monthly};

#[test]
fn breakdown_groups_by_month() {
    let trades = vec![
        Trade {
            entry_time: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            exit_time: Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap(),
            pnl: dec!(10),
            exit_reason: TradeExitReason::TakeProfit,
        },
        Trade {
            entry_time: Utc.with_ymd_and_hms(2024, 2, 1, 0, 0, 0).unwrap(),
            exit_time: Utc.with_ymd_and_hms(2024, 2, 2, 0, 0, 0).unwrap(),
            pnl: dec!(-5),
            exit_reason: TradeExitReason::StopLoss,
        },
    ];

    let rows = breakdown_monthly(&trades);
    assert_eq!(rows.len(), 2);
}
