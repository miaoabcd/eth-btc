use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
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

#[test]
fn metrics_compute_annualized_and_drawdown() {
    let trades = vec![Trade {
        entry_time: Utc.timestamp_opt(0, 0).unwrap(),
        exit_time: Utc.timestamp_opt(3600, 0).unwrap(),
        pnl: dec!(10),
        exit_reason: TradeExitReason::TakeProfit,
    }];
    let equity = vec![
        EquityPoint {
            timestamp: Utc.timestamp_opt(0, 0).unwrap(),
            equity: dec!(100),
        },
        EquityPoint {
            timestamp: Utc.timestamp_opt(365 * 24 * 3600, 0).unwrap(),
            equity: dec!(120),
        },
        EquityPoint {
            timestamp: Utc.timestamp_opt(2 * 365 * 24 * 3600, 0).unwrap(),
            equity: dec!(90),
        },
    ];

    let metrics = compute_metrics(&trades, &equity, dec!(0)).unwrap();

    let expected = Decimal::from_f64(0.9f64.sqrt() - 1.0).unwrap();
    let diff = (metrics.annualized_return - expected).abs();
    assert!(diff <= dec!(0.0001));
    assert_eq!(metrics.max_drawdown, dec!(0.25));
}

#[test]
fn metrics_compute_sharpe_ratio() {
    let trades = vec![Trade {
        entry_time: Utc.timestamp_opt(0, 0).unwrap(),
        exit_time: Utc.timestamp_opt(3600, 0).unwrap(),
        pnl: dec!(10),
        exit_reason: TradeExitReason::TakeProfit,
    }];
    let equity = vec![
        EquityPoint {
            timestamp: Utc.timestamp_opt(0, 0).unwrap(),
            equity: dec!(100),
        },
        EquityPoint {
            timestamp: Utc.timestamp_opt(365 * 24 * 3600, 0).unwrap(),
            equity: dec!(110),
        },
        EquityPoint {
            timestamp: Utc.timestamp_opt(2 * 365 * 24 * 3600, 0).unwrap(),
            equity: dec!(104.5),
        },
    ];

    let metrics = compute_metrics(&trades, &equity, dec!(0)).unwrap();

    let expected = dec!(0.3333333333);
    let diff = (metrics.sharpe_ratio - expected).abs();
    assert!(diff <= dec!(0.0001));
}
