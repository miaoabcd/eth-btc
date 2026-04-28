use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::account::{AccountFillSource, ExchangeFill, ExchangePosition, PairExposure};
use eth_btc_strategy::config::{CapitalMode, Config, FundingMode, OrderType, Symbol};
use eth_btc_strategy::core::TradeDirection;
use eth_btc_strategy::core::strategy::StrategyEngine;
use eth_btc_strategy::execution::{
    ExecutionEngine, ExecutionError, OrderExecutor, OrderFill, OrderRequest, OrderSubmitResult,
    PaperOrderExecutor, RetryConfig,
};
use eth_btc_strategy::funding::{FundingRate, estimate_funding_cost};
use eth_btc_strategy::logging::{EntryBlockReason, LogEvent, PnlSource, TradeEvent, TradeLog};
use eth_btc_strategy::state::{PositionLeg, PositionSnapshot, StrategyState, StrategyStatus};

#[derive(Default)]
struct RecordingExecutor {
    submitted: std::sync::Mutex<Vec<OrderRequest>>,
}

#[async_trait::async_trait]
impl OrderExecutor for RecordingExecutor {
    async fn submit(
        &self,
        order: &OrderRequest,
    ) -> Result<rust_decimal::Decimal, eth_btc_strategy::execution::ExecutionError> {
        self.submitted
            .lock()
            .expect("submit lock")
            .push(order.clone());
        Ok(order.qty)
    }

    async fn close(
        &self,
        order: &OrderRequest,
    ) -> Result<rust_decimal::Decimal, eth_btc_strategy::execution::ExecutionError> {
        self.submitted
            .lock()
            .expect("submit lock")
            .push(order.clone());
        Ok(order.qty)
    }
}

#[derive(Default)]
struct RestingEntryExecutor {
    submitted: std::sync::Mutex<Vec<OrderRequest>>,
}

#[async_trait::async_trait]
impl OrderExecutor for RestingEntryExecutor {
    async fn submit(
        &self,
        order: &OrderRequest,
    ) -> Result<rust_decimal::Decimal, eth_btc_strategy::execution::ExecutionError> {
        self.submitted
            .lock()
            .expect("submit lock")
            .push(order.clone());
        Ok(order.qty)
    }

    async fn close(
        &self,
        order: &OrderRequest,
    ) -> Result<rust_decimal::Decimal, eth_btc_strategy::execution::ExecutionError> {
        self.submitted
            .lock()
            .expect("submit lock")
            .push(order.clone());
        Ok(order.qty)
    }

    async fn submit_result(
        &self,
        order: &OrderRequest,
    ) -> Result<OrderSubmitResult, eth_btc_strategy::execution::ExecutionError> {
        self.submitted
            .lock()
            .expect("submit lock")
            .push(order.clone());
        let oid = if order.symbol == Symbol::EthPerp {
            11
        } else {
            22
        };
        Ok(OrderSubmitResult::Resting { oid })
    }
}

#[derive(Default)]
struct CancelTrackingExecutor {
    cancelled: std::sync::Mutex<Vec<(Symbol, u64)>>,
}

#[derive(Default)]
struct PostOnlyWouldTakeExecutor;

struct DetailedFillExecutor;

#[derive(Clone)]
struct StaticFillSource {
    fills: std::sync::Arc<Vec<ExchangeFill>>,
}

#[async_trait::async_trait]
impl OrderExecutor for CancelTrackingExecutor {
    async fn submit(
        &self,
        _order: &OrderRequest,
    ) -> Result<rust_decimal::Decimal, eth_btc_strategy::execution::ExecutionError> {
        Ok(dec!(0))
    }

    async fn close(
        &self,
        _order: &OrderRequest,
    ) -> Result<rust_decimal::Decimal, eth_btc_strategy::execution::ExecutionError> {
        Ok(dec!(0))
    }

    async fn cancel(
        &self,
        symbol: Symbol,
        oid: u64,
    ) -> Result<(), eth_btc_strategy::execution::ExecutionError> {
        self.cancelled
            .lock()
            .expect("cancel lock")
            .push((symbol, oid));
        Ok(())
    }
}

#[async_trait::async_trait]
impl OrderExecutor for PostOnlyWouldTakeExecutor {
    async fn submit(&self, _order: &OrderRequest) -> Result<rust_decimal::Decimal, ExecutionError> {
        Err(ExecutionError::Fatal(
            "Post only order would have immediately matched".to_string(),
        ))
    }

    async fn close(&self, _order: &OrderRequest) -> Result<rust_decimal::Decimal, ExecutionError> {
        Ok(dec!(0))
    }

    async fn submit_result(
        &self,
        _order: &OrderRequest,
    ) -> Result<OrderSubmitResult, ExecutionError> {
        Err(ExecutionError::Fatal(
            "Post only order would have immediately matched".to_string(),
        ))
    }
}

#[async_trait::async_trait]
impl OrderExecutor for DetailedFillExecutor {
    async fn submit(&self, order: &OrderRequest) -> Result<rust_decimal::Decimal, ExecutionError> {
        Ok(order.qty)
    }

    async fn close(&self, order: &OrderRequest) -> Result<rust_decimal::Decimal, ExecutionError> {
        Ok(order.qty)
    }

    async fn submit_result(
        &self,
        order: &OrderRequest,
    ) -> Result<OrderSubmitResult, ExecutionError> {
        let (avg_price, oid) = match order.symbol {
            Symbol::EthPerp => (dec!(101), 1001),
            Symbol::BtcPerp => (dec!(99), 1002),
        };
        Ok(OrderSubmitResult::Filled(OrderFill {
            qty: order.qty,
            avg_price: Some(avg_price),
            oid: Some(oid),
        }))
    }

    async fn close_result(
        &self,
        order: &OrderRequest,
    ) -> Result<OrderSubmitResult, ExecutionError> {
        let (avg_price, oid) = match order.symbol {
            Symbol::EthPerp => (dec!(110), 2001),
            Symbol::BtcPerp => (dec!(90), 2002),
        };
        Ok(OrderSubmitResult::Filled(OrderFill {
            qty: order.qty,
            avg_price: Some(avg_price),
            oid: Some(oid),
        }))
    }
}

#[async_trait::async_trait]
impl AccountFillSource for StaticFillSource {
    async fn fetch_user_fills_by_time(
        &self,
        _start: chrono::DateTime<Utc>,
        _end: chrono::DateTime<Utc>,
    ) -> Result<Vec<ExchangeFill>, eth_btc_strategy::account::AccountError> {
        Ok((*self.fills).clone())
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
        pending_entry: None,
        cooldown_until: None,
        cumulative_realized_pnl: dec!(0),
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
            equity: None,
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
        equity: None,
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
async fn strategy_engine_uses_bar_equity_for_equity_ratio() {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.sl_z = dec!(2.0);
    config.position.c_mode = CapitalMode::EquityRatio;
    config.position.equity_ratio_k = Some(dec!(0.1));
    config.position.c_value = Some(dec!(100));
    config.position.equity_value = Some(dec!(10000));

    let execution =
        ExecutionEngine::new(std::sync::Arc::new(PaperOrderExecutor), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config.clone(), execution).unwrap();

    for offset in [0, 900, 1800] {
        let bar = eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(offset, 0).unwrap(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            equity: None,
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
        equity: Some(dec!(1000)),
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: None,
    };

    let outcome = engine.process_bar(bar).await.unwrap();
    assert_eq!(outcome.bar_log.notional_eth, Some(dec!(50)));
    assert_eq!(outcome.bar_log.notional_btc, Some(dec!(50)));
}

#[tokio::test]
async fn strategy_engine_repairs_residual_leg_and_flats_state() {
    let mut config = Config::default();
    config.strategy.n_z = 1;
    config.position.n_vol = 1;

    let executor = std::sync::Arc::new(RecordingExecutor::default());
    let execution = ExecutionEngine::new(executor.clone(), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config, execution).unwrap();

    let position = PositionSnapshot {
        direction: TradeDirection::LongEthShortBtc,
        entry_time: Utc.timestamp_opt(0, 0).unwrap(),
        eth: PositionLeg {
            qty: dec!(0),
            avg_price: dec!(2000),
            notional: dec!(0),
        },
        btc: PositionLeg {
            qty: dec!(1),
            avg_price: dec!(30000),
            notional: dec!(30000),
        },
    };

    let state = StrategyState {
        status: StrategyStatus::InPosition,
        position: Some(position),
        pending_entry: None,
        cooldown_until: None,
        cumulative_realized_pnl: dec!(0),
    };
    engine.apply_state(state).unwrap();

    let bar = eth_btc_strategy::core::strategy::StrategyBar {
        timestamp: Utc.timestamp_opt(900, 0).unwrap(),
        eth_price: dec!(2000),
        btc_price: dec!(30000),
        equity: None,
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: None,
    };

    let outcome = engine.process_bar(bar).await.unwrap();
    assert!(outcome.events.contains(&LogEvent::ResidualRepair));
    assert_eq!(outcome.trade_logs.len(), 1);
    assert!(matches!(
        outcome.trade_logs[0].event,
        TradeEvent::ResidualRepair
    ));
    assert_eq!(engine.state().state().status, StrategyStatus::Flat);

    let submitted = executor.submitted.lock().unwrap();
    assert_eq!(submitted.len(), 1);
    assert_eq!(submitted[0].symbol, Symbol::BtcPerp);
}

#[tokio::test]
async fn strategy_engine_treats_post_only_would_take_as_blocked_entry() {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.sl_z = dec!(2.0);
    config.position.c_value = Some(dec!(100));
    config.execution.order_type = OrderType::PostOnly;

    let execution = ExecutionEngine::new(
        std::sync::Arc::new(PostOnlyWouldTakeExecutor),
        RetryConfig::fast(),
    );
    let mut engine = StrategyEngine::new(config, execution).unwrap();

    for offset in [0, 900, 1800] {
        let bar = eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(offset, 0).unwrap(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            equity: None,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        };
        engine.process_bar(bar).await.unwrap();
    }

    let outcome = engine
        .process_bar(eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(2700, 0).unwrap(),
            eth_price: dec!(271.8281828),
            btc_price: dec!(100),
            equity: None,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        })
        .await
        .unwrap();

    assert_eq!(outcome.state, StrategyStatus::Flat);
    assert_eq!(
        outcome.bar_log.entry_block_reason,
        Some(EntryBlockReason::PostOnlyWouldTake)
    );
    assert!(outcome.events.contains(&LogEvent::EntryCancelled));
    assert!(outcome.trade_logs.is_empty());
}

#[tokio::test]
async fn strategy_engine_blocks_entry_when_cost_gate_fails() {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.tp_z = dec!(0.1);
    config.strategy.sl_z = dec!(10.0);
    config.position.c_value = Some(dec!(100));
    config.cost_gate.enabled = true;
    config.cost_gate.enforce = true;
    config.cost_gate.min_net_edge_bps = dec!(10000);
    config.cost_gate.entry_fee_bps = dec!(3);
    config.cost_gate.exit_fee_bps = dec!(4);
    config.cost_gate.slippage_bps = dec!(1);
    config.cost_gate.spread_bps = dec!(1);

    let recorder = std::sync::Arc::new(RecordingExecutor::default());
    let execution = ExecutionEngine::new(recorder.clone(), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config, execution).unwrap();

    for offset in [0, 900, 1800] {
        let bar = eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(offset, 0).unwrap(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            equity: None,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        };
        engine.process_bar(bar).await.unwrap();
    }

    let outcome = engine
        .process_bar(eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(2700, 0).unwrap(),
            eth_price: dec!(271.8281828),
            btc_price: dec!(100),
            equity: None,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        })
        .await
        .unwrap();

    assert_eq!(outcome.state, StrategyStatus::Flat);
    assert_eq!(
        outcome.bar_log.entry_block_reason,
        Some(EntryBlockReason::CostGate)
    );
    assert_eq!(outcome.bar_log.cost_gate_pass, Some(false));
    assert!(outcome.bar_log.expected_edge_bps.is_some());
    assert!(outcome.bar_log.estimated_cost_bps.is_some());
    assert!(outcome.bar_log.estimated_net_edge_bps.is_some());
    assert!(outcome.events.is_empty());
    assert!(outcome.trade_logs.is_empty());
    assert!(recorder.submitted.lock().expect("submit lock").is_empty());
}

#[tokio::test]
async fn strategy_engine_records_shadow_cost_gate_without_blocking() {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.tp_z = dec!(0.1);
    config.strategy.sl_z = dec!(10.0);
    config.position.c_value = Some(dec!(100));
    config.cost_gate.enabled = true;
    config.cost_gate.enforce = false;
    config.cost_gate.min_net_edge_bps = dec!(10000);

    let recorder = std::sync::Arc::new(RecordingExecutor::default());
    let execution = ExecutionEngine::new(recorder.clone(), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config, execution).unwrap();

    for offset in [0, 900, 1800] {
        let bar = eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(offset, 0).unwrap(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            equity: None,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        };
        engine.process_bar(bar).await.unwrap();
    }

    let outcome = engine
        .process_bar(eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(2700, 0).unwrap(),
            eth_price: dec!(271.8281828),
            btc_price: dec!(100),
            equity: None,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        })
        .await
        .unwrap();

    assert_eq!(outcome.state, StrategyStatus::InPosition);
    assert_eq!(outcome.bar_log.cost_gate_pass, Some(false));
    assert!(outcome.events.contains(&LogEvent::Entry));
    assert_eq!(recorder.submitted.lock().expect("submit lock").len(), 2);
}

#[tokio::test]
async fn strategy_engine_applies_direction_specific_cost_buffer() {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.tp_z = dec!(0.1);
    config.strategy.sl_z = dec!(10.0);
    config.position.c_value = Some(dec!(100));
    config.cost_gate.enabled = true;
    config.cost_gate.enforce = true;
    config.cost_gate.min_net_edge_bps = dec!(0);
    config.cost_gate.short_eth_long_btc_extra_bps = dec!(10000);

    let recorder = std::sync::Arc::new(RecordingExecutor::default());
    let execution = ExecutionEngine::new(recorder.clone(), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config, execution).unwrap();

    for offset in [0, 900, 1800] {
        let bar = eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(offset, 0).unwrap(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            equity: None,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        };
        engine.process_bar(bar).await.unwrap();
    }

    let outcome = engine
        .process_bar(eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(2700, 0).unwrap(),
            eth_price: dec!(271.8281828),
            btc_price: dec!(100),
            equity: None,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        })
        .await
        .unwrap();

    assert_eq!(outcome.state, StrategyStatus::Flat);
    assert_eq!(
        outcome.bar_log.entry_block_reason,
        Some(EntryBlockReason::CostGate)
    );
    assert_eq!(
        outcome.bar_log.cost_gate_required_net_edge_bps,
        Some(dec!(10000))
    );
    assert_eq!(outcome.bar_log.cost_gate_pass, Some(false));
    assert!(recorder.submitted.lock().expect("submit lock").is_empty());
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
            equity: None,
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
        equity: None,
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: None,
    };

    let err = engine.process_bar(bar).await.unwrap_err();
    assert!(matches!(
        err,
        eth_btc_strategy::core::strategy::StrategyError::Position(_)
    ));
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
            equity: None,
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
        equity: None,
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
async fn strategy_engine_post_only_entry_waits_for_exchange_fill() {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.sl_z = dec!(2.0);
    config.position.c_value = Some(dec!(100));
    config.execution.order_type = OrderType::PostOnly;
    config.execution.post_only_bps = 2;
    config.execution.post_only_ttl_secs = 900;

    let recorder = std::sync::Arc::new(RestingEntryExecutor::default());
    let execution = ExecutionEngine::new(recorder.clone(), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config.clone(), execution).unwrap();

    for offset in [0, 900, 1800] {
        let bar = eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(offset, 0).unwrap(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            equity: None,
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
        equity: None,
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: None,
    };
    let pending = engine.process_bar(entry_bar).await.unwrap();

    assert_eq!(pending.state, StrategyStatus::PendingEntry);
    assert!(pending.trade_logs.is_empty());
    assert_eq!(pending.events, vec![LogEvent::EntrySubmitted]);

    let exposure = PairExposure {
        eth: Some(ExchangePosition {
            qty: dec!(0.184),
            entry_price: dec!(271.7),
            notional: dec!(50.0),
        }),
        btc: Some(ExchangePosition {
            qty: dec!(-0.0008),
            entry_price: dec!(100.1),
            notional: dec!(50.0),
        }),
    };
    engine
        .reconcile_exchange_position(
            &exposure,
            Utc.timestamp_opt(3600, 0).unwrap(),
            dec!(271.7),
            dec!(100.1),
        )
        .await
        .unwrap();

    let fill_bar = eth_btc_strategy::core::strategy::StrategyBar {
        timestamp: Utc.timestamp_opt(3600, 0).unwrap(),
        eth_price: dec!(271.7),
        btc_price: dec!(100.1),
        equity: None,
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: None,
    };
    let filled = engine.process_bar(fill_bar).await.unwrap();

    assert_eq!(filled.state, StrategyStatus::InPosition);
    assert!(filled.events.contains(&LogEvent::Entry));
    assert_eq!(filled.trade_logs.len(), 1);
    assert_eq!(filled.trade_logs[0].eth_price, dec!(271.7));
    assert_eq!(filled.trade_logs[0].btc_price, dec!(100.1));
}

#[tokio::test]
async fn strategy_engine_forces_market_exit_when_post_only_is_configured() {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.tp_z = dec!(0.45);
    config.strategy.sl_z = dec!(2.0);
    config.position.c_value = Some(dec!(100));
    config.execution.order_type = OrderType::PostOnly;
    config.execution.post_only_bps = 2;

    let recorder = std::sync::Arc::new(RecordingExecutor::default());
    let execution = ExecutionEngine::new(recorder.clone(), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config.clone(), execution).unwrap();

    let state = StrategyState {
        status: StrategyStatus::InPosition,
        position: Some(PositionSnapshot {
            direction: TradeDirection::LongEthShortBtc,
            entry_time: Utc.timestamp_opt(0, 0).unwrap(),
            eth: PositionLeg {
                qty: dec!(1),
                avg_price: dec!(100),
                notional: dec!(100),
            },
            btc: PositionLeg {
                qty: dec!(-1),
                avg_price: dec!(100),
                notional: dec!(100),
            },
        }),
        pending_entry: None,
        cooldown_until: None,
        cumulative_realized_pnl: dec!(0),
    };
    engine.apply_state(state).unwrap();

    for offset in [0, 900, 1800] {
        let bar = eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(offset, 0).unwrap(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            equity: None,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        };
        engine.process_bar(bar).await.unwrap();
    }

    let exit_bar = eth_btc_strategy::core::strategy::StrategyBar {
        timestamp: Utc.timestamp_opt(2700, 0).unwrap(),
        eth_price: dec!(100),
        btc_price: dec!(100),
        equity: None,
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: None,
    };
    let _ = engine.process_bar(exit_bar).await.unwrap();

    let submitted = recorder.submitted.lock().expect("submit lock");
    assert!(
        submitted
            .iter()
            .all(|order| order.order_type == OrderType::Market)
    );
}

#[tokio::test]
async fn strategy_engine_cancels_expired_pending_entry_before_flattening() {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    let executor = std::sync::Arc::new(CancelTrackingExecutor::default());
    let execution = ExecutionEngine::new(executor.clone(), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config, execution).unwrap();

    engine
        .apply_state(StrategyState {
            status: StrategyStatus::PendingEntry,
            position: None,
            pending_entry: Some(eth_btc_strategy::state::PendingEntrySnapshot {
                direction: TradeDirection::LongEthShortBtc,
                eth_qty: dec!(0.01),
                btc_qty: dec!(-0.001),
                eth_order_id: 11,
                btc_order_id: 22,
                submitted_at: Utc.timestamp_opt(100, 0).unwrap(),
                expires_at: Utc.timestamp_opt(200, 0).unwrap(),
            }),
            cooldown_until: None,
            cumulative_realized_pnl: dec!(0),
        })
        .unwrap();

    let outcome = engine
        .process_bar(eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(200, 0).unwrap(),
            eth_price: dec!(2000),
            btc_price: dec!(60000),
            equity: None,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        })
        .await
        .unwrap();

    let cancelled = executor.cancelled.lock().expect("cancel lock").clone();
    assert_eq!(
        cancelled,
        vec![(Symbol::EthPerp, 11), (Symbol::BtcPerp, 22)]
    );
    assert_eq!(outcome.state, StrategyStatus::Flat);
    assert!(outcome.events.contains(&LogEvent::EntryCancelled));
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
            equity: None,
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
        equity: None,
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: None,
    };

    let entry_outcome = engine.process_bar(entry_bar).await.unwrap();
    assert!(entry_outcome.events.contains(&LogEvent::Entry));
    assert_eq!(entry_outcome.trade_logs.len(), 1);
    match &entry_outcome.trade_logs[0] {
        TradeLog {
            event: TradeEvent::Entry,
            realized_pnl,
            cumulative_realized_pnl,
            ..
        } => {
            assert_eq!(*realized_pnl, dec!(0));
            assert_eq!(*cumulative_realized_pnl, dec!(0));
        }
        other => panic!("unexpected entry trade log {other:?}"),
    }

    let exit_bar = eth_btc_strategy::core::strategy::StrategyBar {
        timestamp: Utc.timestamp_opt(3600, 0).unwrap(),
        eth_price: dec!(164.872127),
        btc_price: dec!(100),
        equity: None,
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: None,
    };

    let exit_outcome = engine.process_bar(exit_bar).await.unwrap();
    assert_eq!(exit_outcome.trade_logs.len(), 1);
    match &exit_outcome.trade_logs[0] {
        TradeLog {
            event: TradeEvent::Exit(_),
            realized_pnl,
            cumulative_realized_pnl,
            ..
        } => {
            assert!(*realized_pnl > dec!(0));
            assert_eq!(*cumulative_realized_pnl, *realized_pnl);
        }
        other => panic!("unexpected exit trade log {other:?}"),
    }
}

#[tokio::test]
async fn strategy_engine_uses_exchange_fills_for_trade_log_pnl() {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.tp_z = dec!(0.45);
    config.strategy.sl_z = dec!(2.0);
    config.position.c_value = Some(dec!(100));

    let execution = ExecutionEngine::new(
        std::sync::Arc::new(DetailedFillExecutor),
        RetryConfig::fast(),
    );
    let fill_source = StaticFillSource {
        fills: std::sync::Arc::new(vec![
            ExchangeFill {
                coin: Symbol::EthPerp,
                price: dec!(101),
                size: dec!(1),
                fee: dec!(0.01),
                closed_pnl: dec!(0),
                timestamp: Utc.timestamp_millis_opt(3_000_000).unwrap(),
                oid: Some(1001),
                tid: Some(1),
            },
            ExchangeFill {
                coin: Symbol::BtcPerp,
                price: dec!(99),
                size: dec!(1),
                fee: dec!(0.02),
                closed_pnl: dec!(0),
                timestamp: Utc.timestamp_millis_opt(3_000_001).unwrap(),
                oid: Some(1002),
                tid: Some(2),
            },
            ExchangeFill {
                coin: Symbol::EthPerp,
                price: dec!(110),
                size: dec!(1),
                fee: dec!(0.03),
                closed_pnl: dec!(9),
                timestamp: Utc.timestamp_millis_opt(4_000_000).unwrap(),
                oid: Some(2001),
                tid: Some(3),
            },
            ExchangeFill {
                coin: Symbol::BtcPerp,
                price: dec!(90),
                size: dec!(1),
                fee: dec!(0.04),
                closed_pnl: dec!(9),
                timestamp: Utc.timestamp_millis_opt(4_000_001).unwrap(),
                oid: Some(2002),
                tid: Some(4),
            },
        ]),
    };
    let mut engine = StrategyEngine::new(config.clone(), execution)
        .unwrap()
        .with_fill_source(std::sync::Arc::new(fill_source));

    for offset in [0, 900, 1800] {
        let bar = eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(offset, 0).unwrap(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            equity: None,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        };
        engine.process_bar(bar).await.unwrap();
    }

    let entry_outcome = engine
        .process_bar(eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(2700, 0).unwrap(),
            eth_price: dec!(271.8281828),
            btc_price: dec!(100),
            equity: None,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        })
        .await
        .unwrap();

    assert_eq!(entry_outcome.trade_logs.len(), 1);
    let entry_log = &entry_outcome.trade_logs[0];
    assert_eq!(entry_log.event, TradeEvent::Entry);
    assert_eq!(entry_log.pnl_source, PnlSource::ExchangeFills);
    assert_eq!(entry_log.eth_price, dec!(101));
    assert_eq!(entry_log.btc_price, dec!(99));
    assert_eq!(entry_log.fee, dec!(0.03));
    assert_eq!(entry_log.exchange_closed_pnl, Some(dec!(0)));
    assert_eq!(entry_log.realized_pnl, dec!(-0.03));
    assert_eq!(entry_log.cumulative_realized_pnl, dec!(-0.03));
    let position = engine.state().state().position.as_ref().unwrap();
    assert_eq!(position.eth.avg_price, dec!(101));
    assert_eq!(position.btc.avg_price, dec!(99));

    let exit_outcome = engine
        .process_bar(eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(3600, 0).unwrap(),
            eth_price: dec!(164.872127),
            btc_price: dec!(100),
            equity: None,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        })
        .await
        .unwrap();

    assert_eq!(exit_outcome.trade_logs.len(), 1);
    let exit_log = &exit_outcome.trade_logs[0];
    assert!(matches!(exit_log.event, TradeEvent::Exit(_)));
    assert_eq!(exit_log.pnl_source, PnlSource::ExchangeFills);
    assert_eq!(exit_log.eth_price, dec!(110));
    assert_eq!(exit_log.btc_price, dec!(90));
    assert_eq!(exit_log.fee, dec!(0.07));
    assert_eq!(exit_log.exchange_closed_pnl, Some(dec!(18)));
    assert_eq!(exit_log.realized_pnl, dec!(17.93));
    assert_eq!(exit_log.cumulative_realized_pnl, dec!(17.90));
    assert_eq!(engine.state().state().cumulative_realized_pnl, dec!(17.90));
}

#[tokio::test]
async fn strategy_engine_populates_unrealized_pnl_while_holding_position() {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.tp_z = dec!(0);
    config.strategy.sl_z = dec!(1000);
    config.position.c_value = Some(dec!(100));

    let execution =
        ExecutionEngine::new(std::sync::Arc::new(PaperOrderExecutor), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config, execution).unwrap();

    for offset in [0, 900, 1800] {
        let bar = eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(offset, 0).unwrap(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            equity: None,
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
        equity: None,
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: None,
    };
    let entry_outcome = engine.process_bar(entry_bar).await.unwrap();
    assert_eq!(entry_outcome.state, StrategyStatus::InPosition);
    assert_eq!(entry_outcome.bar_log.unrealized_pnl, dec!(0));

    let holding_bar = eth_btc_strategy::core::strategy::StrategyBar {
        timestamp: Utc.timestamp_opt(3600, 0).unwrap(),
        eth_price: dec!(250),
        btc_price: dec!(105),
        equity: None,
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: None,
    };
    let holding_outcome = engine.process_bar(holding_bar).await.unwrap();
    assert_eq!(holding_outcome.state, StrategyStatus::InPosition);
    assert!(holding_outcome.bar_log.unrealized_pnl > dec!(0));
}

#[tokio::test]
async fn strategy_engine_exit_realized_pnl_deducts_funding_cost() {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.tp_z = dec!(0.45);
    config.strategy.sl_z = dec!(2.0);
    config.position.c_value = Some(dec!(100));

    let execution =
        ExecutionEngine::new(std::sync::Arc::new(PaperOrderExecutor), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config, execution).unwrap();

    for offset in [0, 900, 1800] {
        let bar = eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(offset, 0).unwrap(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            equity: None,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: Some(1),
        };
        engine.process_bar(bar).await.unwrap();
    }

    let entry_bar = eth_btc_strategy::core::strategy::StrategyBar {
        timestamp: Utc.timestamp_opt(2700, 0).unwrap(),
        eth_price: dec!(271.8281828),
        btc_price: dec!(100),
        equity: None,
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: Some(1),
    };
    let entry_outcome = engine.process_bar(entry_bar).await.unwrap();
    let notional_eth = entry_outcome.bar_log.notional_eth.unwrap();
    let notional_btc = entry_outcome.bar_log.notional_btc.unwrap();

    let exit_bar = eth_btc_strategy::core::strategy::StrategyBar {
        timestamp: Utc.timestamp_opt(6300, 0).unwrap(),
        eth_price: dec!(164.872127),
        btc_price: dec!(100),
        equity: None,
        funding_eth: Some(dec!(0)),
        funding_btc: Some(dec!(0.01)),
        funding_interval_hours: Some(1),
    };
    let exit_outcome = engine.process_bar(exit_bar.clone()).await.unwrap();
    let log = &exit_outcome.trade_logs[0];
    let gross = log.eth_qty * (log.eth_price - log.entry_eth_price)
        + log.btc_qty * (log.btc_price - log.entry_btc_price);
    let funding = estimate_funding_cost(
        log.direction,
        notional_eth,
        notional_btc,
        &FundingRate {
            symbol: Symbol::EthPerp,
            rate: dec!(0),
            timestamp: exit_bar.timestamp,
            interval_hours: 1,
        },
        &FundingRate {
            symbol: Symbol::BtcPerp,
            rate: dec!(0.01),
            timestamp: exit_bar.timestamp,
            interval_hours: 1,
        },
        1,
    )
    .unwrap()
    .cost_est;
    assert_eq!(log.realized_pnl, gross - funding);
}

#[tokio::test]
async fn strategy_engine_threshold_mode_requires_adjusted_threshold_cross() {
    let mut threshold_cfg = Config::default();
    threshold_cfg.strategy.n_z = 3;
    threshold_cfg.position.n_vol = 1;
    threshold_cfg.strategy.entry_z = dec!(0.5);
    threshold_cfg.strategy.tp_z = dec!(0.45);
    threshold_cfg.strategy.sl_z = dec!(20.0);
    threshold_cfg.position.c_value = Some(dec!(100));
    threshold_cfg.risk.max_hold_hours = 1;
    threshold_cfg.funding.modes = vec![FundingMode::Threshold];
    threshold_cfg.funding.funding_threshold_k = Some(dec!(1000));

    let execution =
        ExecutionEngine::new(std::sync::Arc::new(PaperOrderExecutor), RetryConfig::fast());
    let mut engine = StrategyEngine::new(threshold_cfg, execution).unwrap();

    for offset in [0, 900, 1800] {
        let bar = eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(offset, 0).unwrap(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            equity: None,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: Some(1),
        };
        engine.process_bar(bar).await.unwrap();
    }

    let entry_candidate = eth_btc_strategy::core::strategy::StrategyBar {
        timestamp: Utc.timestamp_opt(2700, 0).unwrap(),
        eth_price: dec!(271.8281828),
        btc_price: dec!(100),
        equity: None,
        funding_eth: Some(dec!(0)),
        funding_btc: Some(dec!(0.01)),
        funding_interval_hours: Some(1),
    };

    let first = engine.process_bar(entry_candidate).await.unwrap();
    assert_eq!(first.state, StrategyStatus::Flat);
    assert!(first.trade_logs.is_empty());
    assert!(!first.events.contains(&LogEvent::Entry));
    assert!(first.bar_log.funding_cost_est.is_some());
    assert_eq!(
        first.bar_log.entry_block_reason,
        Some(EntryBlockReason::FundingThreshold)
    );
}

#[tokio::test]
async fn strategy_engine_skips_entry_below_minimum_size() {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.sl_z = dec!(2.0);
    config.position.c_mode = CapitalMode::FixedNotional;
    config.position.c_value = Some(dec!(1));

    let recorder = std::sync::Arc::new(RecordingExecutor::default());
    let execution = ExecutionEngine::new(recorder.clone(), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config, execution).unwrap();

    for offset in [0, 900, 1800] {
        let bar = eth_btc_strategy::core::strategy::StrategyBar {
            timestamp: Utc.timestamp_opt(offset, 0).unwrap(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            equity: None,
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
        equity: None,
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: None,
    };

    let outcome = engine.process_bar(entry_bar).await.unwrap();
    assert_eq!(outcome.state, StrategyStatus::Flat);
    assert!(outcome.trade_logs.is_empty());
    assert_eq!(
        outcome.bar_log.entry_block_reason,
        Some(EntryBlockReason::BelowMinSizeEth)
    );
    let submitted = recorder.submitted.lock().expect("submit lock");
    assert!(submitted.is_empty());
}

#[tokio::test]
async fn strategy_engine_marks_unavailable_zscore_as_not_ready() {
    let mut config = Config::default();
    config.strategy.n_z = 384;
    config.position.n_vol = 1;
    config.position.c_mode = CapitalMode::FixedNotional;
    config.position.c_value = Some(dec!(100));

    let execution =
        ExecutionEngine::new(std::sync::Arc::new(PaperOrderExecutor), RetryConfig::fast());
    let mut engine = StrategyEngine::new(config, execution).unwrap();

    let first_bar = eth_btc_strategy::core::strategy::StrategyBar {
        timestamp: Utc.timestamp_opt(0, 0).unwrap(),
        eth_price: dec!(2000),
        btc_price: dec!(30000),
        equity: None,
        funding_eth: None,
        funding_btc: None,
        funding_interval_hours: None,
    };
    let outcome = engine.process_bar(first_bar).await.unwrap();
    assert_eq!(outcome.state, StrategyStatus::Flat);
    assert_eq!(
        outcome.bar_log.entry_block_reason,
        Some(EntryBlockReason::ZscoreUnavailable)
    );
}
