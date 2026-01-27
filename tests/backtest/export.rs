use std::fs;

use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;
use uuid::Uuid;

use eth_btc_strategy::backtest::{
    BacktestResult, EquityPoint, Metrics, Trade, TradeExitReason, export_equity_csv,
    export_metrics_json, export_trades_csv,
};

#[test]
fn exports_results_to_files() {
    let dir = std::env::temp_dir().join(format!("eth_btc_export_{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();

    let result = BacktestResult {
        trades: vec![Trade {
            entry_time: Utc.timestamp_opt(0, 0).unwrap(),
            exit_time: Utc.timestamp_opt(3600, 0).unwrap(),
            pnl: dec!(10),
            exit_reason: TradeExitReason::TakeProfit,
        }],
        equity_curve: vec![EquityPoint {
            timestamp: Utc.timestamp_opt(0, 0).unwrap(),
            equity: dec!(1000),
        }],
        bar_logs: vec![],
        metrics: Metrics::default(),
    };

    let metrics_path = dir.join("metrics.json");
    let trades_path = dir.join("trades.csv");
    let equity_path = dir.join("equity.csv");

    export_metrics_json(&metrics_path, &result.metrics).unwrap();
    export_trades_csv(&trades_path, &result.trades).unwrap();
    export_equity_csv(&equity_path, &result.equity_curve).unwrap();

    assert!(metrics_path.exists());
    assert!(trades_path.exists());
    assert!(equity_path.exists());
}
