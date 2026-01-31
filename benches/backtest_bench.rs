use chrono::{TimeZone, Utc};
use criterion::{criterion_group, criterion_main, Criterion};
use rust_decimal_macros::dec;

use eth_btc_strategy::backtest::{EquityPoint, Trade, TradeExitReason, compute_metrics};

fn bench_compute_metrics(c: &mut Criterion) {
    let trades = vec![
        Trade {
            entry_time: Utc.timestamp_opt(0, 0).unwrap(),
            exit_time: Utc.timestamp_opt(3600, 0).unwrap(),
            pnl: dec!(10),
            exit_reason: TradeExitReason::TakeProfit,
        },
        Trade {
            entry_time: Utc.timestamp_opt(7200, 0).unwrap(),
            exit_time: Utc.timestamp_opt(10800, 0).unwrap(),
            pnl: dec!(-5),
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
            equity: dec!(1010),
        },
        EquityPoint {
            timestamp: Utc.timestamp_opt(7200, 0).unwrap(),
            equity: dec!(1005),
        },
    ];

    c.bench_function("compute_metrics", |b| {
        b.iter(|| compute_metrics(&trades, &equity, dec!(0)).unwrap());
    });
}

criterion_group!(benches, bench_compute_metrics);
criterion_main!(benches);
