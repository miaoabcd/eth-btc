use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::config::{CapitalMode, Config};
use eth_btc_strategy::core::strategy::StrategyEngine;
use eth_btc_strategy::core::TradeDirection;
use eth_btc_strategy::logging::{LogEvent, TradeEvent, TradeLog};
use eth_btc_strategy::execution::{
    ExecutionEngine, OrderExecutor, OrderRequest, PaperOrderExecutor, RetryConfig,
};
use eth_btc_strategy::funding::estimate_funding_cost;
use eth_btc_strategy::state::{PositionLeg, PositionSnapshot, StrategyState, StrategyStatus};

#[derive(Default)]
struct RecordingExecutor {
    submitted: std::sync::Mutex<Vec<OrderRequest>>,
}

#[async_trait::async_trait]
impl OrderExecutor for RecordingExecutor {
    async fn submit(&self, order: &OrderRequest) -> Result<rust_decimal::Decimal, eth_btc_strategy::execution::ExecutionError> {
        self.submitted
            .lock()
            .expect("submit lock")
            .push(order.clone());
        Ok(order.qty)
    }

    async fn close(&self, order: &OrderRequest) -> Result<rust_decimal::Decimal, eth_btc_strategy::execution::ExecutionError> {
        self.submitted
            .lock()
            .expect("submit lock")
            .push(order.clone());
        Ok(order.qty)
    }
}

#[test]
fn strategy_engine_applies_state() {
    let config = Config::default();
    let execution =
        ExecutionEngine::new(std::sync::Arc::new(PaperOrderExecutor), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config, execution).unwrap();

    let position = PositionSnapshot {
        direction: TradeDirection::LongEthShortBtc,
        entry_time: Utc.timestamp_opt(0, 0).unwrap(),
        eth: PositionLeg {
            qty: dec!(1),
            avg_price: dec!(100),
            notional: dec!(100),
        },
        btc: PositionLeg {
            qty: dec!(-1),
            avg_price: dec!(200),
            notional: dec!(200),
        },
    };

    let state = StrategyState {
        status: StrategyStatus::InPosition,
        position: Some(position),
        cooldown_until: None,
    };

    engine.apply_state(state).unwrap();
    assert_eq!(engine.state().state().status, StrategyStatus::InPosition);
}

#[test]
fn strategy_engine_is_debuggable() {
    let config = Config::default();
    let execution =
        ExecutionEngine::new(std::sync::Arc::new(PaperOrderExecutor), RetryConfig::fast());
    let engine = StrategyEngine::new(config, execution).unwrap();
    let _ = format!("{engine:?}");
}

#[tokio::test]
async fn strategy_engine_uses_bar_funding_interval() {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.sl_z = dec!(10.0);
    config.position.c_value = Some(dec!(100));
    config.risk.max_hold_hours = 2;
    config.funding.funding_cost_threshold = Some(dec!(2.0));

    let execution =
        ExecutionEngine::new(std::sync::Arc::new(PaperOrderExecutor), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config.clone(), execution).unwrap();

    for offset in [0, 900, 1800] {
        let bar = eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(offset, 0).unwrap(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: Some(1),
        };
        engine.process_bar(bar).await.unwrap();
    }

    let bar = eth_btc_strategy::core::strategy::StrategyBar {
        timestamp: Utc.timestamp_opt(2700, 0).unwrap(),
        eth_price: dec!(271.8281828),
        btc_price: dec!(100),
        funding_eth: Some(dec!(0.001)),
        funding_btc: Some(dec!(0.01)),
        funding_interval_hours: Some(1),
    };

    let outcome = engine.process_bar(bar).await.unwrap();
    let expected = estimate_funding_cost(
        TradeDirection::ShortEthLongBtc,
        dec!(50),
        dec!(50),
        &eth_btc_strategy::funding::FundingRate {
            symbol: eth_btc_strategy::config::Symbol::EthPerp,
            rate: dec!(0.001),
            timestamp: outcome.bar_log.timestamp,
            interval_hours: 1,
        },
        &eth_btc_strategy::funding::FundingRate {
            symbol: eth_btc_strategy::config::Symbol::BtcPerp,
            rate: dec!(0.01),
            timestamp: outcome.bar_log.timestamp,
            interval_hours: 1,
        },
        config.risk.max_hold_hours,
    )
    .unwrap();

    assert_eq!(outcome.bar_log.funding_cost_est, Some(expected.cost_est));
}

#[tokio::test]
async fn strategy_engine_uses_equity_value_for_equity_ratio() {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.sl_z = dec!(2.0);
    config.position.c_mode = CapitalMode::EquityRatio;
    config.position.equity_ratio_k = Some(dec!(0.1));
    config.position.c_value = Some(dec!(100));
    config.position.equity_value = Some(dec!(1000));

    let execution =
        ExecutionEngine::new(std::sync::Arc::new(PaperOrderExecutor), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config.clone(), execution).unwrap();

    for offset in [0, 900, 1800] {
        let bar = eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(offset, 0).unwrap(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        };
        engine.process_bar(bar).await.unwrap();
    }

    let eth_price = dec!(271.8281828);
    let btc_price = dec!(100);
    let bar = eth_btc_strategy::core::strategy::StrategyBar {
        timestamp: Utc.timestamp_opt(2700, 0).unwrap(),
        eth_price,
        btc_price,
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: None,
    };

    let outcome = engine.process_bar(bar).await.unwrap();
    assert_eq!(outcome.bar_log.notional_eth, Some(dec!(50)));
    assert_eq!(outcome.bar_log.notional_btc, Some(dec!(50)));
}

#[tokio::test]
async fn strategy_engine_enforces_max_notional_limit() {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.sl_z = dec!(2.0);
    config.position.c_mode = CapitalMode::FixedNotional;
    config.position.c_value = Some(dec!(100));
    config.position.max_notional = Some(dec!(50));

    let execution =
        ExecutionEngine::new(std::sync::Arc::new(PaperOrderExecutor), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config, execution).unwrap();

    for offset in [0, 900, 1800] {
        let bar = eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(offset, 0).unwrap(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        };
        engine.process_bar(bar).await.unwrap();
    }

    let eth_price = dec!(271.8281828);
    let btc_price = dec!(100);
    let bar = eth_btc_strategy::core::strategy::StrategyBar {
        timestamp: Utc.timestamp_opt(2700, 0).unwrap(),
        eth_price,
        btc_price,
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: None,
    };

    let err = engine.process_bar(bar).await.unwrap_err();
    assert!(matches!(err, eth_btc_strategy::core::strategy::StrategyError::Position(_)));
}

#[tokio::test]
async fn strategy_engine_sets_limit_price_with_slippage() {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.sl_z = dec!(2.0);
    config.position.c_value = Some(dec!(100));
    config.execution.slippage_bps = 10;

    let recorder = std::sync::Arc::new(RecordingExecutor::default());
    let execution = ExecutionEngine::new(recorder.clone(), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config.clone(), execution).unwrap();

    for offset in [0, 900, 1800] {
        let bar = eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(offset, 0).unwrap(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        };
        engine.process_bar(bar).await.unwrap();
    }

    let eth_price = dec!(271.8281828);
    let btc_price = dec!(100);
    let bar = eth_btc_strategy::core::strategy::StrategyBar {
        timestamp: Utc.timestamp_opt(2700, 0).unwrap(),
        eth_price,
        btc_price,
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: None,
    };

    engine.process_bar(bar).await.unwrap();

    let submitted = recorder.submitted.lock().expect("submit lock");
    assert_eq!(submitted.len(), 2);
    let slippage = rust_decimal::Decimal::from(config.execution.slippage_bps)
        / rust_decimal::Decimal::new(10000, 0);
    for order in submitted.iter() {
        let limit = order.limit_price.expect("limit price");
        if order.symbol == eth_btc_strategy::config::Symbol::EthPerp {
            assert_eq!(limit, eth_price * (rust_decimal::Decimal::ONE - slippage));
        } else {
            assert_eq!(limit, btc_price * (rust_decimal::Decimal::ONE + slippage));
        }
    }
}

#[tokio::test]
async fn strategy_engine_emits_trade_logs_on_entry_and_exit() {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.tp_z = dec!(0.45);
    config.strategy.sl_z = dec!(2.0);
    config.position.c_value = Some(dec!(100));

    let execution =
        ExecutionEngine::new(std::sync::Arc::new(PaperOrderExecutor), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config.clone(), execution).unwrap();

    for offset in [0, 900, 1800] {
        let bar = eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(offset, 0).unwrap(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        };
        engine.process_bar(bar).await.unwrap();
    }

    let entry_bar = eth_btc_strategy::core::strategy::StrategyBar {
        timestamp: Utc.timestamp_opt(2700, 0).unwrap(),
        eth_price: dec!(271.8281828),
        btc_price: dec!(100),
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: None,
    };

    let entry_outcome = engine.process_bar(entry_bar).await.unwrap();
    assert!(entry_outcome.events.contains(&LogEvent::Entry));
    assert_eq!(entry_outcome.trade_logs.len(), 1);
    match &entry_outcome.trade_logs[0] {
        TradeLog { event: TradeEvent::Entry, .. } => {}
        other => panic!("unexpected entry trade log {other:?}"),
    }

    let exit_bar = eth_btc_strategy::core::strategy::StrategyBar {
        timestamp: Utc.timestamp_opt(3600, 0).unwrap(),
        eth_price: dec!(164.872127),
        btc_price: dec!(100),
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: None,
    };

    let exit_outcome = engine.process_bar(exit_bar).await.unwrap();
    assert_eq!(exit_outcome.trade_logs.len(), 1);
    match &exit_outcome.trade_logs[0] {
        TradeLog {
            event: TradeEvent::Exit(_),
            ..
        } => {}
        other => panic!("unexpected exit trade log {other:?}"),
    }
}
