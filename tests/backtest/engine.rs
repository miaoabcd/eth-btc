use chrono::{TimeZone, Utc};
use rust_decimal::MathematicalOps;
use rust_decimal_macros::dec;

use eth_btc_strategy::backtest::{BacktestBar, BacktestEngine};
use eth_btc_strategy::config::{CapitalMode, Config, SigmaFloorMode};

fn bar(timestamp: i64, r: rust_decimal::Decimal) -> BacktestBar {
    let base = dec!(100);
    let eth = base * r.exp();
    BacktestBar {
        timestamp: Utc.timestamp_opt(timestamp, 0).unwrap(),
        eth_price: eth,
        btc_price: base,
        funding_eth: None,
        funding_btc: None,
    }
}

#[test]
fn backtest_engine_runs_and_generates_trades() {
    let mut config = Config::default();
    config.strategy.n_z = 4;
    config.strategy.entry_z = dec!(1.5);
    config.strategy.tp_z = dec!(0.6);
    config.position.n_vol = 2;
    config.sigma_floor.mode = SigmaFloorMode::Const;

    let bars = vec![
        bar(0, dec!(0.0)),
        bar(900, dec!(0.0)),
        bar(1800, dec!(0.0)),
        bar(2700, dec!(0.0)),
        bar(3600, dec!(0.04)),
        bar(4500, dec!(0.0)),
    ];

    let engine = BacktestEngine::new(config);
    let result = engine.run(&bars).unwrap();

    assert!(!result.trades.is_empty());
    assert_eq!(result.bar_logs.len(), bars.len());
}

#[test]
fn backtest_engine_skips_entry_below_minimum_size() {
    let mut config = Config::default();
    config.strategy.n_z = 4;
    config.strategy.entry_z = dec!(1.5);
    config.strategy.tp_z = dec!(0.6);
    config.position.n_vol = 2;
    config.sigma_floor.mode = SigmaFloorMode::Const;
    config.position.c_value = Some(dec!(1));

    let bars = vec![
        bar(0, dec!(0.0)),
        bar(900, dec!(0.0)),
        bar(1800, dec!(0.0)),
        bar(2700, dec!(0.0)),
        bar(3600, dec!(0.04)),
        bar(4500, dec!(0.0)),
    ];

    let engine = BacktestEngine::new(config);
    let result = engine.run(&bars).unwrap();

    assert!(result.trades.is_empty());
}

#[test]
fn backtest_engine_requires_equity_value_for_equity_ratio() {
    let mut config = Config::default();
    config.position.c_mode = CapitalMode::EquityRatio;
    config.position.equity_ratio_k = Some(dec!(0.1));
    config.position.equity_value = None;
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);

    let bars = vec![bar(0, dec!(0.0)), bar(900, dec!(0.0)), bar(1800, dec!(0.04))];

    let engine = BacktestEngine::new(config);
    let result = engine.run(&bars);
    assert!(result.is_err());
}
