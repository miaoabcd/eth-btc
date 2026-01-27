use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::backtest::{EquityPoint, Trade, TradeExitReason, compute_metrics};

#[test]
fn metrics_compute_win_rate_and_profit_factor() {
    let trades = vec![
        Trade {
            entry_time: Utc.timestamp_opt(0, 0).unwrap(),
            exit_time: Utc.timestamp_opt(3600, 0).unwrap(),
            pnl: dec!(100),
            exit_reason: TradeExitReason::TakeProfit,
        },
        Trade {
            entry_time: Utc.timestamp_opt(7200, 0).unwrap(),
            exit_time: Utc.timestamp_opt(10800, 0).unwrap(),
            pnl: dec!(-50),
            exit_reason: TradeExitReason::StopLoss,
        },
    ];
    let equity = vec![
        EquityPoint {
            timestamp: Utc.timestamp_opt(0, 0).unwrap(),
            equity: dec!(1000),
        },
        EquityPoint {
            timestamp: Utc.timestamp_opt(3600, 0).unwrap(),
            equity: dec!(1050),
        },
    ];

    let metrics = compute_metrics(&trades, &equity, dec!(0)).unwrap();
    assert_eq!(metrics.win_rate, dec!(0.5));
    assert_eq!(metrics.profit_factor, dec!(2.0));
    assert_eq!(metrics.stop_loss_rate, dec!(0.5));
}
