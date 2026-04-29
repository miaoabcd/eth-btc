use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use thiserror::Error;

use crate::account::{AccountFillSource, ExchangeFill, PairExposure};
use crate::config::{CapitalMode, Config, FundingMode, OrderType, PriceField, Symbol};
use crate::core::TradeDirection;
use crate::core::pipeline::SignalPipeline;
use crate::execution::{
    ExecutionEngine, ExecutionError, OrderFill, OrderRequest, OrderSide, PairFill, PairOpenOutcome,
};
use crate::funding::{FundingRate, apply_funding_controls, estimate_funding_cost};
use crate::logging::{BarLog, EntryBlockReason, LogEvent, PnlSource, TradeEvent, TradeLog};
use crate::position::{PositionError, SizeConverter, compute_capital, risk_parity_weights};
use crate::state::{
    PendingEntrySnapshot, PositionLeg, PositionSnapshot, StateMachine, StrategyState,
    StrategyStatus,
};
use crate::storage::PriceBarRecord;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct StrategyBar {
    pub timestamp: DateTime<Utc>,
    pub eth_price: Decimal,
    pub btc_price: Decimal,
    pub equity: Option<Decimal>,
    pub funding_eth: Option<Decimal>,
    pub funding_btc: Option<Decimal>,
    pub funding_interval_hours: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct StrategyOutcome {
    pub state: StrategyStatus,
    pub events: Vec<LogEvent>,
    pub bar_log: BarLog,
    pub trade_logs: Vec<TradeLog>,
}

#[derive(Debug, Error)]
pub enum StrategyError {
    #[error("indicator error: {0}")]
    Indicator(String),
    #[error("data error: {0}")]
    Data(String),
    #[error("execution error: {0}")]
    Execution(String),
    #[error("position error: {0}")]
    Position(String),
    #[error("funding error: {0}")]
    Funding(String),
}

pub struct StrategyEngine {
    config: Config,
    pipeline: SignalPipeline,
    state_machine: StateMachine,
    execution: ExecutionEngine,
    fill_source: Option<Arc<dyn AccountFillSource>>,
    regime_tracker: SpreadHalfLifeTracker,
    cumulative_realized_pnl: Decimal,
    pending_events: Vec<LogEvent>,
    pending_trade_logs: Vec<TradeLog>,
}

#[derive(Debug, Clone)]
struct FillAccounting {
    eth_price: Decimal,
    btc_price: Decimal,
    fee: Decimal,
    exchange_closed_pnl: Option<Decimal>,
    realized_pnl: Decimal,
    source: PnlSource,
}

#[derive(Debug, Clone, Copy)]
struct CostGateDecision {
    expected_edge_bps: Decimal,
    estimated_cost_bps: Decimal,
    estimated_net_edge_bps: Decimal,
    required_net_edge_bps: Decimal,
    pass: bool,
}

#[derive(Debug, Clone)]
struct RegimeGateSnapshot {
    half_life_bars: Option<f64>,
    pass: Option<bool>,
}

#[derive(Debug, Clone)]
struct SpreadHalfLifeTracker {
    lookback_bars: usize,
    values: VecDeque<Decimal>,
}

impl SpreadHalfLifeTracker {
    fn new(lookback_bars: usize) -> Self {
        Self {
            lookback_bars: lookback_bars.max(3),
            values: VecDeque::with_capacity(lookback_bars.max(3)),
        }
    }

    fn push(
        &mut self,
        value: Decimal,
        max_half_life_bars: f64,
        enabled: bool,
    ) -> RegimeGateSnapshot {
        if self.values.len() == self.lookback_bars {
            self.values.pop_front();
        }
        self.values.push_back(value);
        let half_life_bars = if self.values.len() >= self.lookback_bars {
            estimate_half_life_bars_decimal(&self.values)
        } else {
            None
        };
        let pass = if enabled {
            Some(half_life_bars.is_some_and(|value| value <= max_half_life_bars))
        } else {
            None
        };
        RegimeGateSnapshot {
            half_life_bars,
            pass,
        }
    }
}

impl std::fmt::Debug for StrategyEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StrategyEngine")
            .field("config", &self.config)
            .field("pipeline", &self.pipeline)
            .field("state_machine", &self.state_machine)
            .finish()
    }
}

impl StrategyEngine {
    pub fn new(config: Config, execution: ExecutionEngine) -> Result<Self, StrategyError> {
        let pipeline = SignalPipeline::new(&config)
            .map_err(|err| StrategyError::Indicator(err.to_string()))?;
        Ok(Self {
            pipeline,
            state_machine: StateMachine::new(config.risk.clone()),
            regime_tracker: SpreadHalfLifeTracker::new(config.regime_gate.lookback_bars),
            config,
            execution,
            fill_source: None,
            cumulative_realized_pnl: Decimal::ZERO,
            pending_events: Vec::new(),
            pending_trade_logs: Vec::new(),
        })
    }

    pub fn with_fill_source(mut self, source: Arc<dyn AccountFillSource>) -> Self {
        self.fill_source = Some(source);
        self
    }

    pub fn state(&self) -> &StateMachine {
        &self.state_machine
    }

    pub fn apply_state(&mut self, state: StrategyState) -> Result<(), StrategyError> {
        let cumulative_realized_pnl = state.cumulative_realized_pnl;
        self.state_machine
            .hydrate(state)
            .map_err(|err| StrategyError::Position(err.to_string()))?;
        self.cumulative_realized_pnl = cumulative_realized_pnl;
        Ok(())
    }

    pub async fn reconcile_exchange_position(
        &mut self,
        exposure: &PairExposure,
        timestamp: DateTime<Utc>,
        eth_price: Decimal,
        btc_price: Decimal,
    ) -> Result<(), StrategyError> {
        let local_state = self.state_machine.state().clone();

        if exposure.is_flat() {
            if local_state.position.is_some() {
                warn!(
                    local_status = ?local_state.status,
                    "exchange is flat while local state still holds a position; forcing flat"
                );
                self.state_machine.force_flat();
            }
            return Ok(());
        }

        if exposure.has_residual() {
            if local_state.status == StrategyStatus::PendingEntry
                && let Some(pending) = local_state.pending_entry.as_ref()
            {
                self.cancel_pending_entry_orders(pending).await;
            }
            let position = self.exposure_to_position(exposure, timestamp)?;
            warn!(
                eth_qty = %position.eth.qty,
                btc_qty = %position.btc.qty,
                "exchange residual leg detected; attempting repair"
            );
            let repair_fill = self
                .execution
                .repair_residual(&position)
                .await
                .map_err(|err| StrategyError::Execution(err.to_string()))?;
            let trade_log = self
                .residual_repair_trade_log(
                    &position,
                    timestamp,
                    eth_price,
                    btc_price,
                    repair_fill.as_ref(),
                )
                .await;
            self.state_machine.force_flat();
            self.pending_events.push(LogEvent::ResidualRepair);
            self.pending_trade_logs.push(trade_log);
            return Ok(());
        }

        match local_state.status {
            StrategyStatus::PendingEntry => {
                let mut position = self.exposure_to_position(exposure, timestamp)?;
                let pending = local_state.pending_entry.as_ref().ok_or_else(|| {
                    StrategyError::Position(
                        "pending-entry state missing pending snapshot".to_string(),
                    )
                })?;
                let accounting = self
                    .fill_accounting_for_order_ids(
                        &[Some(pending.eth_order_id), Some(pending.btc_order_id)],
                        &[Symbol::EthPerp, Symbol::BtcPerp],
                        pending.submitted_at,
                        timestamp,
                        position.eth.avg_price,
                        position.btc.avg_price,
                        Decimal::ZERO,
                    )
                    .await
                    .unwrap_or_else(|| {
                        Self::model_accounting(
                            position.eth.avg_price,
                            position.btc.avg_price,
                            Decimal::ZERO,
                        )
                    });
                position.eth.avg_price = accounting.eth_price;
                position.btc.avg_price = accounting.btc_price;
                self.add_realized_pnl(accounting.realized_pnl);
                self.state_machine
                    .confirm_pending_entry(position.clone())
                    .map_err(|err| StrategyError::Position(err.to_string()))?;
                self.pending_events.push(LogEvent::Entry);
                self.pending_trade_logs.push(TradeLog {
                    timestamp,
                    event: TradeEvent::Entry,
                    direction: position.direction,
                    eth_qty: position.eth.qty,
                    btc_qty: position.btc.qty,
                    eth_price: accounting.eth_price,
                    btc_price: accounting.btc_price,
                    entry_time: timestamp,
                    entry_eth_price: accounting.eth_price,
                    entry_btc_price: accounting.btc_price,
                    realized_pnl: accounting.realized_pnl,
                    cumulative_realized_pnl: self.cumulative_realized_pnl,
                    fee: accounting.fee,
                    exchange_closed_pnl: accounting.exchange_closed_pnl,
                    pnl_source: accounting.source,
                    eth_ref_price: Some(eth_price),
                    btc_ref_price: Some(btc_price),
                    eth_slippage_bps: Some(slippage_bps_for_side(
                        eth_price,
                        accounting.eth_price,
                        entry_eth_side(position.direction),
                    )),
                    btc_slippage_bps: Some(slippage_bps_for_side(
                        btc_price,
                        accounting.btc_price,
                        entry_btc_side(position.direction),
                    )),
                });
                Ok(())
            }
            StrategyStatus::InPosition => {
                let Some(position) = local_state.position.as_ref() else {
                    return Err(StrategyError::Position(
                        "in-position state missing local snapshot".to_string(),
                    ));
                };
                if !Self::exposure_matches_position(exposure, position) {
                    return Err(StrategyError::Execution(format!(
                        "exchange position mismatch with local state: local eth_qty={} btc_qty={}, remote eth_qty={} btc_qty={}",
                        position.eth.qty,
                        position.btc.qty,
                        exposure.eth_qty(),
                        exposure.btc_qty(),
                    )));
                }
                Ok(())
            }
            StrategyStatus::Flat | StrategyStatus::Cooldown => {
                Err(StrategyError::Execution(format!(
                    "exchange position exists while local state is {:?}: remote eth_qty={} btc_qty={}",
                    local_state.status,
                    exposure.eth_qty(),
                    exposure.btc_qty(),
                )))
            }
        }
    }

    pub fn warm_up_with_records(
        &mut self,
        records: &[PriceBarRecord],
    ) -> Result<(), StrategyError> {
        let mut sorted = records.to_vec();
        sorted.sort_by_key(|r| r.timestamp);
        for record in sorted {
            self.state_machine.update(record.timestamp);
            let eth_price = select_price(
                self.config.data.price_field,
                record.eth_mid,
                record.eth_mark,
                record.eth_close,
            )
            .ok_or_else(|| {
                StrategyError::Data(format!(
                    "missing eth price in warmup at {}",
                    record.timestamp
                ))
            })?;
            let btc_price = select_price(
                self.config.data.price_field,
                record.btc_mid,
                record.btc_mark,
                record.btc_close,
            )
            .ok_or_else(|| {
                StrategyError::Data(format!(
                    "missing btc price in warmup at {}",
                    record.timestamp
                ))
            })?;
            let output = self
                .pipeline
                .update(
                    record.timestamp,
                    eth_price,
                    btc_price,
                    self.state_machine.state().status,
                    self.state_machine.state().position.as_ref(),
                )
                .map_err(|err| StrategyError::Indicator(err.to_string()))?;
            self.regime_tracker.push(
                output.r,
                self.config.regime_gate.max_half_life_bars,
                self.config.regime_gate.enabled,
            );
        }
        Ok(())
    }

    pub async fn process_bar(
        &mut self,
        bar: StrategyBar,
    ) -> Result<StrategyOutcome, StrategyError> {
        let mut events = std::mem::take(&mut self.pending_events);
        let mut trade_logs = std::mem::take(&mut self.pending_trade_logs);
        if let Some(pending) = self.state_machine.state().pending_entry.clone()
            && self.state_machine.state().status == StrategyStatus::PendingEntry
            && bar.timestamp >= pending.expires_at
        {
            self.execution
                .cancel_order(Symbol::EthPerp, pending.eth_order_id)
                .await
                .map_err(|err| StrategyError::Execution(err.to_string()))?;
            self.execution
                .cancel_order(Symbol::BtcPerp, pending.btc_order_id)
                .await
                .map_err(|err| StrategyError::Execution(err.to_string()))?;
            self.state_machine.force_flat();
            events.push(LogEvent::EntryCancelled);
        }
        self.state_machine.update(bar.timestamp);

        if let Some(position) = self.state_machine.state().position.clone()
            && position.has_residual()
        {
            warn!(
                eth_qty = %position.eth.qty,
                btc_qty = %position.btc.qty,
                "residual leg detected; attempting repair"
            );
            let repair_fill = self
                .execution
                .repair_residual(&position)
                .await
                .map_err(|err| StrategyError::Execution(err.to_string()))?;
            let trade_log = self
                .residual_repair_trade_log(
                    &position,
                    bar.timestamp,
                    bar.eth_price,
                    bar.btc_price,
                    repair_fill.as_ref(),
                )
                .await;
            self.state_machine.force_flat();
            events.push(LogEvent::ResidualRepair);
            trade_logs.push(trade_log);
        }
        let output = self
            .pipeline
            .update(
                bar.timestamp,
                bar.eth_price,
                bar.btc_price,
                self.state_machine.state().status,
                self.state_machine.state().position.as_ref(),
            )
            .map_err(|err| StrategyError::Indicator(err.to_string()))?;
        let z_snapshot = output.z_snapshot;
        let vol_snapshot = output.vol_snapshot;
        let entry_signal = output.entry_signal;
        let exit_signal = output.exit_signal;
        let regime_snapshot = self.regime_tracker.push(
            output.r,
            self.config.regime_gate.max_half_life_bars,
            self.config.regime_gate.enabled,
        );

        let mut w_eth = None;
        let mut w_btc = None;
        let mut notional_eth = None;
        let mut notional_btc = None;
        let mut funding_cost_est = None;
        let mut funding_skip = None;
        let regime_half_life_bars = regime_snapshot.half_life_bars;
        let regime_gate_pass = regime_snapshot.pass;
        let mut expected_edge_bps = None;
        let mut estimated_cost_bps = None;
        let mut estimated_net_edge_bps = None;
        let mut cost_gate_required_net_edge_bps = None;
        let mut cost_gate_pass = None;
        let mut entry_block_reason = None;

        if let Some(vol_eth) = vol_snapshot.vol_eth
            && let Some(vol_btc) = vol_snapshot.vol_btc
        {
            let weights = risk_parity_weights(vol_eth, vol_btc)
                .map_err(|err| StrategyError::Position(err.to_string()))?;
            w_eth = Some(weights.w_eth);
            w_btc = Some(weights.w_btc);
        }

        if self.state_machine.state().status == StrategyStatus::Flat {
            if z_snapshot.zscore.is_none() {
                entry_block_reason = Some(EntryBlockReason::ZscoreUnavailable);
            } else if entry_signal.is_none() {
                entry_block_reason = Some(EntryBlockReason::NoCross);
            } else if vol_snapshot.vol_eth.is_none() || vol_snapshot.vol_btc.is_none() {
                entry_block_reason = Some(EntryBlockReason::VolatilityUnavailable);
            }
        }

        if let Some(signal) = entry_signal
            && let Some(vol_eth) = vol_snapshot.vol_eth
            && let Some(vol_btc) = vol_snapshot.vol_btc
        {
            let equity = match self.config.position.c_mode {
                CapitalMode::FixedNotional => self.config.position.c_value.unwrap_or(Decimal::ZERO),
                CapitalMode::EquityRatio => {
                    if let Some(value) = bar.equity {
                        value
                    } else if let Some(value) = self.config.position.equity_value {
                        value
                    } else {
                        return Err(StrategyError::Position(
                            "equity unavailable for equity ratio mode".to_string(),
                        ));
                    }
                }
            };
            let capital = compute_capital(&self.config.position, equity)
                .map_err(|err| StrategyError::Position(err.to_string()))?;
            if let Some(max_notional) = self.config.position.max_notional
                && capital > max_notional
            {
                return Err(StrategyError::Position(format!(
                    "capital {capital} exceeds max_notional {max_notional}"
                )));
            }
            let weights = risk_parity_weights(vol_eth, vol_btc)
                .map_err(|err| StrategyError::Position(err.to_string()))?;
            let notional_eth_value = capital * weights.w_eth;
            let notional_btc_value = capital * weights.w_btc;
            w_eth = Some(weights.w_eth);
            w_btc = Some(weights.w_btc);
            notional_eth = Some(notional_eth_value);
            notional_btc = Some(notional_btc_value);

            if let (Some(funding_eth), Some(funding_btc)) = (bar.funding_eth, bar.funding_btc) {
                let interval_hours = bar.funding_interval_hours.unwrap_or(8);
                let eth_rate = FundingRate {
                    symbol: Symbol::EthPerp,
                    rate: funding_eth,
                    timestamp: bar.timestamp,
                    interval_hours,
                };
                let btc_rate = FundingRate {
                    symbol: Symbol::BtcPerp,
                    rate: funding_btc,
                    timestamp: bar.timestamp,
                    interval_hours,
                };
                let estimate = estimate_funding_cost(
                    signal.direction,
                    notional_eth_value,
                    notional_btc_value,
                    &eth_rate,
                    &btc_rate,
                    self.config.risk.max_hold_hours,
                )
                .map_err(|err| StrategyError::Funding(err.to_string()))?;
                funding_cost_est = Some(estimate.cost_est);
                let decision = apply_funding_controls(
                    &self.config.funding,
                    self.config.strategy.entry_z,
                    capital,
                    &estimate,
                )
                .map_err(|err| StrategyError::Funding(err.to_string()))?;
                funding_skip = Some(decision.should_skip);
                if decision.should_skip {
                    entry_block_reason = Some(EntryBlockReason::FundingFilter);
                    return Ok(self.build_outcome(
                        bar,
                        z_snapshot,
                        vol_snapshot,
                        events,
                        w_eth,
                        w_btc,
                        Some(notional_eth_value),
                        Some(notional_btc_value),
                        funding_cost_est,
                        funding_skip,
                        regime_half_life_bars,
                        regime_gate_pass,
                        expected_edge_bps,
                        estimated_cost_bps,
                        estimated_net_edge_bps,
                        cost_gate_required_net_edge_bps,
                        cost_gate_pass,
                        entry_block_reason,
                        trade_logs,
                    ));
                }
                if self.config.funding.modes.contains(&FundingMode::Threshold)
                    && signal.zscore.abs() < decision.adjusted_entry_z
                {
                    entry_block_reason = Some(EntryBlockReason::FundingThreshold);
                    return Ok(self.build_outcome(
                        bar,
                        z_snapshot,
                        vol_snapshot,
                        events,
                        w_eth,
                        w_btc,
                        Some(notional_eth_value),
                        Some(notional_btc_value),
                        funding_cost_est,
                        funding_skip,
                        regime_half_life_bars,
                        regime_gate_pass,
                        expected_edge_bps,
                        estimated_cost_bps,
                        estimated_net_edge_bps,
                        cost_gate_required_net_edge_bps,
                        cost_gate_pass,
                        entry_block_reason,
                        trade_logs,
                    ));
                }
            }

            if self.config.regime_gate.enabled && regime_gate_pass != Some(true) {
                entry_block_reason = Some(EntryBlockReason::RegimeGate);
                return Ok(self.build_outcome(
                    bar,
                    z_snapshot,
                    vol_snapshot,
                    events,
                    w_eth,
                    w_btc,
                    Some(notional_eth_value),
                    Some(notional_btc_value),
                    funding_cost_est,
                    funding_skip,
                    regime_half_life_bars,
                    regime_gate_pass,
                    expected_edge_bps,
                    estimated_cost_bps,
                    estimated_net_edge_bps,
                    cost_gate_required_net_edge_bps,
                    cost_gate_pass,
                    entry_block_reason,
                    trade_logs,
                ));
            }

            if let Some(decision) = self.cost_gate_decision(
                signal.direction,
                signal.zscore,
                z_snapshot.sigma_eff,
                capital,
                funding_cost_est,
            ) {
                expected_edge_bps = Some(decision.expected_edge_bps);
                estimated_cost_bps = Some(decision.estimated_cost_bps);
                estimated_net_edge_bps = Some(decision.estimated_net_edge_bps);
                cost_gate_required_net_edge_bps = Some(decision.required_net_edge_bps);
                cost_gate_pass = Some(decision.pass);
                if self.config.cost_gate.enforce && !decision.pass {
                    entry_block_reason = Some(EntryBlockReason::CostGate);
                    return Ok(self.build_outcome(
                        bar,
                        z_snapshot,
                        vol_snapshot,
                        events,
                        w_eth,
                        w_btc,
                        Some(notional_eth_value),
                        Some(notional_btc_value),
                        funding_cost_est,
                        funding_skip,
                        regime_half_life_bars,
                        regime_gate_pass,
                        expected_edge_bps,
                        estimated_cost_bps,
                        estimated_net_edge_bps,
                        cost_gate_required_net_edge_bps,
                        cost_gate_pass,
                        entry_block_reason,
                        trade_logs,
                    ));
                }
            }

            let eth_converter = SizeConverter::new(
                self.config
                    .instrument_constraints
                    .get(&Symbol::EthPerp)
                    .cloned()
                    .unwrap_or_default(),
                self.config.position.min_size_policy,
            );
            let btc_converter = SizeConverter::new(
                self.config
                    .instrument_constraints
                    .get(&Symbol::BtcPerp)
                    .cloned()
                    .unwrap_or_default(),
                self.config.position.min_size_policy,
            );
            let eth_order = match eth_converter.convert_notional(notional_eth_value, bar.eth_price)
            {
                Ok(order) => order,
                Err(PositionError::BelowMinimum(_)) => {
                    entry_block_reason = Some(EntryBlockReason::BelowMinSizeEth);
                    return Ok(self.build_outcome(
                        bar,
                        z_snapshot,
                        vol_snapshot,
                        events,
                        w_eth,
                        w_btc,
                        Some(notional_eth_value),
                        Some(notional_btc_value),
                        funding_cost_est,
                        funding_skip,
                        regime_half_life_bars,
                        regime_gate_pass,
                        expected_edge_bps,
                        estimated_cost_bps,
                        estimated_net_edge_bps,
                        cost_gate_required_net_edge_bps,
                        cost_gate_pass,
                        entry_block_reason,
                        trade_logs,
                    ));
                }
                Err(err) => return Err(StrategyError::Position(err.to_string())),
            };
            let btc_order = match btc_converter.convert_notional(notional_btc_value, bar.btc_price)
            {
                Ok(order) => order,
                Err(PositionError::BelowMinimum(_)) => {
                    entry_block_reason = Some(EntryBlockReason::BelowMinSizeBtc);
                    return Ok(self.build_outcome(
                        bar,
                        z_snapshot,
                        vol_snapshot,
                        events,
                        w_eth,
                        w_btc,
                        Some(notional_eth_value),
                        Some(notional_btc_value),
                        funding_cost_est,
                        funding_skip,
                        regime_half_life_bars,
                        regime_gate_pass,
                        expected_edge_bps,
                        estimated_cost_bps,
                        estimated_net_edge_bps,
                        cost_gate_required_net_edge_bps,
                        cost_gate_pass,
                        entry_block_reason,
                        trade_logs,
                    ));
                }
                Err(err) => return Err(StrategyError::Position(err.to_string())),
            };

            let (eth_side, btc_side) = match signal.direction {
                TradeDirection::LongEthShortBtc => (OrderSide::Buy, OrderSide::Sell),
                TradeDirection::ShortEthLongBtc => (OrderSide::Sell, OrderSide::Buy),
            };
            let entry_order_type = self.entry_order_type();
            let eth_limit_price = self.limit_price(entry_order_type, eth_side, bar.eth_price);
            let btc_limit_price = self.limit_price(entry_order_type, btc_side, bar.btc_price);
            let expires_after = matches!(entry_order_type, OrderType::PostOnly).then(|| {
                (bar.timestamp.timestamp_millis() as u64)
                    + self.config.execution.post_only_ttl_secs * 1000
            });
            info!(
                timestamp = %bar.timestamp.to_rfc3339(),
                direction = ?signal.direction,
                zscore = %signal.zscore,
                eth_side = ?eth_side,
                eth_qty = %eth_order.qty,
                eth_limit_price = %eth_limit_price,
                btc_side = ?btc_side,
                btc_qty = %btc_order.qty,
                btc_limit_price = %btc_limit_price,
                "entry order attempt"
            );
            let open_outcome = match self
                .execution
                .open_pair(
                    OrderRequest {
                        symbol: Symbol::EthPerp,
                        side: eth_side,
                        qty: eth_order.qty,
                        order_type: entry_order_type,
                        limit_price: Some(eth_limit_price),
                        expires_after,
                    },
                    OrderRequest {
                        symbol: Symbol::BtcPerp,
                        side: btc_side,
                        qty: btc_order.qty,
                        order_type: entry_order_type,
                        limit_price: Some(btc_limit_price),
                        expires_after,
                    },
                )
                .await
            {
                Ok(outcome) => outcome,
                Err(err)
                    if matches!(err, ExecutionError::Fatal(_)) && err.is_post_only_would_take() =>
                {
                    entry_block_reason = Some(EntryBlockReason::PostOnlyWouldTake);
                    events.push(LogEvent::EntryCancelled);
                    return Ok(self.build_outcome(
                        bar,
                        z_snapshot,
                        vol_snapshot,
                        events,
                        w_eth,
                        w_btc,
                        notional_eth,
                        notional_btc,
                        funding_cost_est,
                        funding_skip,
                        regime_half_life_bars,
                        regime_gate_pass,
                        expected_edge_bps,
                        estimated_cost_bps,
                        estimated_net_edge_bps,
                        cost_gate_required_net_edge_bps,
                        cost_gate_pass,
                        entry_block_reason,
                        trade_logs,
                    ));
                }
                Err(err) => return Err(StrategyError::Execution(err.to_string())),
            };
            match open_outcome {
                PairOpenOutcome::Filled(pair_fill) => {
                    let accounting = self
                        .fill_accounting_for_pair_fill(
                            &pair_fill,
                            bar.timestamp,
                            pair_fill.eth.avg_price.unwrap_or(bar.eth_price),
                            pair_fill.btc.avg_price.unwrap_or(bar.btc_price),
                            Decimal::ZERO,
                        )
                        .await
                        .unwrap_or_else(|| {
                            Self::model_accounting(
                                pair_fill.eth.avg_price.unwrap_or(bar.eth_price),
                                pair_fill.btc.avg_price.unwrap_or(bar.btc_price),
                                Decimal::ZERO,
                            )
                        });
                    self.add_realized_pnl(accounting.realized_pnl);
                    let position = PositionSnapshot {
                        direction: signal.direction,
                        entry_time: bar.timestamp,
                        eth: PositionLeg {
                            qty: if signal.direction == TradeDirection::LongEthShortBtc {
                                pair_fill.eth.qty
                            } else {
                                -pair_fill.eth.qty
                            },
                            avg_price: accounting.eth_price,
                            notional: pair_fill.eth.qty.abs() * accounting.eth_price,
                        },
                        btc: PositionLeg {
                            qty: if signal.direction == TradeDirection::LongEthShortBtc {
                                -pair_fill.btc.qty
                            } else {
                                pair_fill.btc.qty
                            },
                            avg_price: accounting.btc_price,
                            notional: pair_fill.btc.qty.abs() * accounting.btc_price,
                        },
                    };
                    self.state_machine
                        .enter(position, bar.timestamp)
                        .map_err(|err| StrategyError::Position(err.to_string()))?;
                    events.push(LogEvent::Entry);
                    trade_logs.push(TradeLog {
                        timestamp: bar.timestamp,
                        event: TradeEvent::Entry,
                        direction: signal.direction,
                        eth_qty: if signal.direction == TradeDirection::LongEthShortBtc {
                            pair_fill.eth.qty
                        } else {
                            -pair_fill.eth.qty
                        },
                        btc_qty: if signal.direction == TradeDirection::LongEthShortBtc {
                            -pair_fill.btc.qty
                        } else {
                            pair_fill.btc.qty
                        },
                        eth_price: accounting.eth_price,
                        btc_price: accounting.btc_price,
                        entry_time: bar.timestamp,
                        entry_eth_price: accounting.eth_price,
                        entry_btc_price: accounting.btc_price,
                        realized_pnl: accounting.realized_pnl,
                        cumulative_realized_pnl: self.cumulative_realized_pnl,
                        fee: accounting.fee,
                        exchange_closed_pnl: accounting.exchange_closed_pnl,
                        pnl_source: accounting.source,
                        eth_ref_price: Some(bar.eth_price),
                        btc_ref_price: Some(bar.btc_price),
                        eth_slippage_bps: Some(slippage_bps_for_side(
                            bar.eth_price,
                            accounting.eth_price,
                            eth_side,
                        )),
                        btc_slippage_bps: Some(slippage_bps_for_side(
                            bar.btc_price,
                            accounting.btc_price,
                            btc_side,
                        )),
                    });
                }
                PairOpenOutcome::Resting(resting) => {
                    self.state_machine
                        .enter_pending(PendingEntrySnapshot {
                            direction: signal.direction,
                            eth_qty: if signal.direction == TradeDirection::LongEthShortBtc {
                                eth_order.qty
                            } else {
                                -eth_order.qty
                            },
                            btc_qty: if signal.direction == TradeDirection::LongEthShortBtc {
                                -btc_order.qty
                            } else {
                                btc_order.qty
                            },
                            eth_order_id: resting.eth_oid,
                            btc_order_id: resting.btc_oid,
                            submitted_at: bar.timestamp,
                            expires_at: bar.timestamp
                                + chrono::Duration::seconds(
                                    self.config.execution.post_only_ttl_secs as i64,
                                ),
                        })
                        .map_err(|err| StrategyError::Position(err.to_string()))?;
                    events.push(LogEvent::EntrySubmitted);
                }
            }
        }

        if let Some(exit_signal) = exit_signal
            && let Some(position) = self.state_machine.state().position.clone()
        {
            let eth_side = OrderSide::close_for_qty(position.eth.qty);
            let btc_side = OrderSide::close_for_qty(position.btc.qty);
            let exit_order_type = self.exit_order_type();
            let eth_order = OrderRequest {
                symbol: Symbol::EthPerp,
                side: eth_side,
                qty: position.eth.qty.abs(),
                order_type: exit_order_type,
                limit_price: Some(self.limit_price(exit_order_type, eth_side, bar.eth_price)),
                expires_after: None,
            };
            let btc_order = OrderRequest {
                symbol: Symbol::BtcPerp,
                side: btc_side,
                qty: position.btc.qty.abs(),
                order_type: exit_order_type,
                limit_price: Some(self.limit_price(exit_order_type, btc_side, bar.btc_price)),
                expires_after: None,
            };
            info!(
                timestamp = %bar.timestamp.to_rfc3339(),
                reason = ?exit_signal.reason,
                direction = ?position.direction,
                eth_side = ?eth_side,
                eth_qty = %eth_order.qty,
                eth_limit_price = %eth_order.limit_price.unwrap_or(bar.eth_price),
                btc_side = ?btc_side,
                btc_qty = %btc_order.qty,
                btc_limit_price = %btc_order.limit_price.unwrap_or(bar.btc_price),
                "exit order attempt"
            );
            let pair_fill = self
                .execution
                .close_pair(eth_order, btc_order)
                .await
                .map_err(|err| StrategyError::Execution(err.to_string()))?;
            let close_eth_price = pair_fill.eth.avg_price.unwrap_or(bar.eth_price);
            let close_btc_price = pair_fill.btc.avg_price.unwrap_or(bar.btc_price);
            let mut model_realized_pnl =
                compute_position_pnl(&position, close_eth_price, close_btc_price);
            if let (Some(funding_eth), Some(funding_btc)) = (bar.funding_eth, bar.funding_btc) {
                let interval_hours = bar
                    .funding_interval_hours
                    .filter(|value| *value > 0)
                    .unwrap_or(8);
                let holding_hours = (bar.timestamp - position.entry_time).num_hours().max(0) as u32;
                let estimate = estimate_funding_cost(
                    position.direction,
                    position.eth.notional,
                    position.btc.notional,
                    &FundingRate {
                        symbol: Symbol::EthPerp,
                        rate: funding_eth,
                        timestamp: bar.timestamp,
                        interval_hours,
                    },
                    &FundingRate {
                        symbol: Symbol::BtcPerp,
                        rate: funding_btc,
                        timestamp: bar.timestamp,
                        interval_hours,
                    },
                    holding_hours,
                )
                .map_err(|err| StrategyError::Funding(err.to_string()))?;
                model_realized_pnl -= estimate.cost_est;
            }
            let accounting = self
                .fill_accounting_for_pair_fill(
                    &pair_fill,
                    bar.timestamp,
                    close_eth_price,
                    close_btc_price,
                    model_realized_pnl,
                )
                .await
                .unwrap_or_else(|| {
                    Self::model_accounting(close_eth_price, close_btc_price, model_realized_pnl)
                });
            self.add_realized_pnl(accounting.realized_pnl);
            trade_logs.push(TradeLog {
                timestamp: bar.timestamp,
                event: TradeEvent::Exit(exit_signal.reason),
                direction: position.direction,
                eth_qty: position.eth.qty,
                btc_qty: position.btc.qty,
                eth_price: accounting.eth_price,
                btc_price: accounting.btc_price,
                entry_time: position.entry_time,
                entry_eth_price: position.eth.avg_price,
                entry_btc_price: position.btc.avg_price,
                realized_pnl: accounting.realized_pnl,
                cumulative_realized_pnl: self.cumulative_realized_pnl,
                fee: accounting.fee,
                exchange_closed_pnl: accounting.exchange_closed_pnl,
                pnl_source: accounting.source,
                eth_ref_price: Some(bar.eth_price),
                btc_ref_price: Some(bar.btc_price),
                eth_slippage_bps: Some(slippage_bps_for_side(
                    bar.eth_price,
                    accounting.eth_price,
                    eth_side,
                )),
                btc_slippage_bps: Some(slippage_bps_for_side(
                    bar.btc_price,
                    accounting.btc_price,
                    btc_side,
                )),
            });
            self.state_machine
                .exit(exit_signal.reason, bar.timestamp)
                .map_err(|err| StrategyError::Position(err.to_string()))?;
            events.push(LogEvent::Exit(exit_signal.reason));
        }

        Ok(self.build_outcome(
            bar,
            z_snapshot,
            vol_snapshot,
            events,
            w_eth,
            w_btc,
            notional_eth,
            notional_btc,
            funding_cost_est,
            funding_skip,
            regime_half_life_bars,
            regime_gate_pass,
            expected_edge_bps,
            estimated_cost_bps,
            estimated_net_edge_bps,
            cost_gate_required_net_edge_bps,
            cost_gate_pass,
            entry_block_reason,
            trade_logs,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn build_outcome(
        &self,
        bar: StrategyBar,
        z_snapshot: crate::indicators::ZScoreSnapshot,
        vol_snapshot: crate::indicators::VolatilitySnapshot,
        events: Vec<LogEvent>,
        w_eth: Option<Decimal>,
        w_btc: Option<Decimal>,
        notional_eth: Option<Decimal>,
        notional_btc: Option<Decimal>,
        funding_cost_est: Option<Decimal>,
        funding_skip: Option<bool>,
        regime_half_life_bars: Option<f64>,
        regime_gate_pass: Option<bool>,
        expected_edge_bps: Option<Decimal>,
        estimated_cost_bps: Option<Decimal>,
        estimated_net_edge_bps: Option<Decimal>,
        cost_gate_required_net_edge_bps: Option<Decimal>,
        cost_gate_pass: Option<bool>,
        entry_block_reason: Option<EntryBlockReason>,
        trade_logs: Vec<TradeLog>,
    ) -> StrategyOutcome {
        let unrealized_pnl = self
            .state_machine
            .state()
            .position
            .as_ref()
            .map(|position| compute_position_pnl(position, bar.eth_price, bar.btc_price))
            .unwrap_or(Decimal::ZERO);
        StrategyOutcome {
            state: self.state_machine.state().status,
            events: events.clone(),
            bar_log: BarLog {
                timestamp: bar.timestamp,
                eth_price: Some(bar.eth_price),
                btc_price: Some(bar.btc_price),
                r: Some(z_snapshot.r),
                mu: z_snapshot.mean,
                sigma: z_snapshot.sigma,
                sigma_eff: z_snapshot.sigma_eff,
                zscore: z_snapshot.zscore,
                vol_eth: vol_snapshot.vol_eth,
                vol_btc: vol_snapshot.vol_btc,
                w_eth,
                w_btc,
                notional_eth,
                notional_btc,
                funding_eth: bar.funding_eth,
                funding_btc: bar.funding_btc,
                funding_cost_est,
                funding_skip,
                regime_half_life_bars,
                regime_gate_pass,
                expected_edge_bps,
                estimated_cost_bps,
                estimated_net_edge_bps,
                cost_gate_required_net_edge_bps,
                cost_gate_pass,
                eth_best_bid: None,
                eth_best_ask: None,
                eth_bid_size: None,
                eth_ask_size: None,
                eth_spread_bps: None,
                btc_best_bid: None,
                btc_best_ask: None,
                btc_bid_size: None,
                btc_ask_size: None,
                btc_spread_bps: None,
                entry_block_reason,
                run_error: None,
                unrealized_pnl,
                state: self.state_machine.state().status,
                position: self.state_machine.state().position.clone(),
                events,
            },
            trade_logs,
        }
    }

    fn entry_order_type(&self) -> OrderType {
        self.config.execution.order_type
    }

    fn exit_order_type(&self) -> OrderType {
        match self.config.execution.order_type {
            OrderType::PostOnly => OrderType::Market,
            other => other,
        }
    }

    fn limit_price(&self, order_type: OrderType, side: OrderSide, price: Decimal) -> Decimal {
        match order_type {
            OrderType::Market => {
                let slippage =
                    Decimal::from(self.config.execution.slippage_bps) / Decimal::new(10000, 0);
                match side {
                    OrderSide::Buy => price * (Decimal::ONE + slippage),
                    OrderSide::Sell => price * (Decimal::ONE - slippage),
                }
            }
            OrderType::Limit => price,
            OrderType::PostOnly => {
                let offset =
                    Decimal::from(self.config.execution.post_only_bps) / Decimal::new(10000, 0);
                match side {
                    OrderSide::Buy => price * (Decimal::ONE - offset),
                    OrderSide::Sell => price * (Decimal::ONE + offset),
                }
            }
        }
    }

    fn cost_gate_decision(
        &self,
        direction: TradeDirection,
        zscore: Decimal,
        sigma_eff: Option<Decimal>,
        capital: Decimal,
        funding_cost_est: Option<Decimal>,
    ) -> Option<CostGateDecision> {
        if !self.config.cost_gate.enabled {
            return None;
        }
        let sigma_eff = sigma_eff?;
        let gross_z_edge = (zscore.abs() - self.config.strategy.tp_z).max(Decimal::ZERO);
        let expected_edge_bps = gross_z_edge * sigma_eff * Decimal::from(10_000u32);
        let funding_bps = if capital > Decimal::ZERO {
            funding_cost_est.unwrap_or(Decimal::ZERO) / capital * Decimal::from(10_000u32)
        } else {
            Decimal::ZERO
        };
        let estimated_cost_bps = self.config.cost_gate.entry_fee_bps
            + self.config.cost_gate.exit_fee_bps
            + self.config.cost_gate.slippage_bps
            + self.config.cost_gate.spread_bps
            + funding_bps;
        let estimated_net_edge_bps = expected_edge_bps - estimated_cost_bps;
        let required_net_edge_bps = self.config.cost_gate.min_net_edge_bps
            + match direction {
                TradeDirection::LongEthShortBtc => {
                    self.config.cost_gate.long_eth_short_btc_extra_bps
                }
                TradeDirection::ShortEthLongBtc => {
                    self.config.cost_gate.short_eth_long_btc_extra_bps
                }
            };
        Some(CostGateDecision {
            expected_edge_bps,
            estimated_cost_bps,
            estimated_net_edge_bps,
            required_net_edge_bps,
            pass: estimated_net_edge_bps >= required_net_edge_bps,
        })
    }

    fn exposure_to_position(
        &self,
        exposure: &PairExposure,
        timestamp: DateTime<Utc>,
    ) -> Result<PositionSnapshot, StrategyError> {
        let eth_qty = exposure.eth_qty();
        let btc_qty = exposure.btc_qty();
        let direction = if eth_qty > Decimal::ZERO || btc_qty < Decimal::ZERO {
            TradeDirection::LongEthShortBtc
        } else if eth_qty < Decimal::ZERO || btc_qty > Decimal::ZERO {
            TradeDirection::ShortEthLongBtc
        } else {
            return Err(StrategyError::Position(
                "cannot infer direction from flat exchange exposure".to_string(),
            ));
        };

        Ok(PositionSnapshot {
            direction,
            entry_time: timestamp,
            eth: PositionLeg {
                qty: eth_qty,
                avg_price: exposure
                    .eth
                    .as_ref()
                    .map(|position| position.entry_price)
                    .unwrap_or(Decimal::ZERO),
                notional: exposure
                    .eth
                    .as_ref()
                    .map(|position| position.notional)
                    .unwrap_or(Decimal::ZERO),
            },
            btc: PositionLeg {
                qty: btc_qty,
                avg_price: exposure
                    .btc
                    .as_ref()
                    .map(|position| position.entry_price)
                    .unwrap_or(Decimal::ZERO),
                notional: exposure
                    .btc
                    .as_ref()
                    .map(|position| position.notional)
                    .unwrap_or(Decimal::ZERO),
            },
        })
    }

    fn add_realized_pnl(&mut self, pnl: Decimal) {
        self.cumulative_realized_pnl += pnl;
        self.state_machine
            .set_cumulative_realized_pnl(self.cumulative_realized_pnl);
    }

    async fn cancel_pending_entry_orders(&self, pending: &PendingEntrySnapshot) {
        for (symbol, oid) in [
            (Symbol::EthPerp, pending.eth_order_id),
            (Symbol::BtcPerp, pending.btc_order_id),
        ] {
            if let Err(err) = self.execution.cancel_order(symbol, oid).await {
                warn!(
                    ?symbol,
                    oid,
                    error = %err,
                    "failed to cancel pending entry order during residual repair"
                );
            }
        }
    }

    fn model_accounting(
        eth_price: Decimal,
        btc_price: Decimal,
        realized_pnl: Decimal,
    ) -> FillAccounting {
        FillAccounting {
            eth_price,
            btc_price,
            fee: Decimal::ZERO,
            exchange_closed_pnl: None,
            realized_pnl,
            source: PnlSource::ModelEstimate,
        }
    }

    async fn fill_accounting_for_pair_fill(
        &self,
        pair_fill: &PairFill,
        timestamp: DateTime<Utc>,
        fallback_eth_price: Decimal,
        fallback_btc_price: Decimal,
        fallback_realized_pnl: Decimal,
    ) -> Option<FillAccounting> {
        self.fill_accounting_for_order_ids(
            &[pair_fill.eth.oid, pair_fill.btc.oid],
            &[Symbol::EthPerp, Symbol::BtcPerp],
            timestamp - Duration::minutes(5),
            timestamp,
            fallback_eth_price,
            fallback_btc_price,
            fallback_realized_pnl,
        )
        .await
    }

    async fn fill_accounting_for_order_ids(
        &self,
        order_ids: &[Option<u64>],
        expected_symbols: &[Symbol],
        start: DateTime<Utc>,
        end_hint: DateTime<Utc>,
        fallback_eth_price: Decimal,
        fallback_btc_price: Decimal,
        _fallback_realized_pnl: Decimal,
    ) -> Option<FillAccounting> {
        let source = self.fill_source.as_ref()?;
        let order_ids: HashSet<u64> = order_ids.iter().copied().flatten().collect();
        if order_ids.is_empty() {
            return None;
        }
        let end = std::cmp::max(
            end_hint + Duration::minutes(5),
            Utc::now() + Duration::minutes(1),
        );
        let fills = match source.fetch_user_fills_by_time(start, end).await {
            Ok(fills) => fills,
            Err(err) => {
                warn!(error = ?err, "exchange fill fetch failed; falling back to model pnl");
                return None;
            }
        };
        let matched: Vec<ExchangeFill> = fills
            .into_iter()
            .filter(|fill| fill.oid.is_some_and(|oid| order_ids.contains(&oid)))
            .collect();
        if matched.is_empty() {
            warn!(
                order_ids = ?order_ids,
                "no matching exchange fills found; falling back to model pnl"
            );
            return None;
        }
        for symbol in expected_symbols {
            if !matched.iter().any(|fill| fill.coin == *symbol) {
                warn!(
                    symbol = ?symbol,
                    order_ids = ?order_ids,
                    "missing expected exchange fill symbol; falling back to model pnl"
                );
                return None;
            }
        }
        Some(Self::summarize_exchange_fills(
            &matched,
            fallback_eth_price,
            fallback_btc_price,
        ))
    }

    fn summarize_exchange_fills(
        fills: &[ExchangeFill],
        fallback_eth_price: Decimal,
        fallback_btc_price: Decimal,
    ) -> FillAccounting {
        let mut eth_px_sz = Decimal::ZERO;
        let mut eth_sz = Decimal::ZERO;
        let mut btc_px_sz = Decimal::ZERO;
        let mut btc_sz = Decimal::ZERO;
        let mut fee = Decimal::ZERO;
        let mut closed_pnl = Decimal::ZERO;

        for fill in fills {
            fee += fill.fee;
            closed_pnl += fill.closed_pnl;
            match fill.coin {
                Symbol::EthPerp => {
                    eth_px_sz += fill.price * fill.size;
                    eth_sz += fill.size;
                }
                Symbol::BtcPerp => {
                    btc_px_sz += fill.price * fill.size;
                    btc_sz += fill.size;
                }
            }
        }

        FillAccounting {
            eth_price: if eth_sz > Decimal::ZERO {
                eth_px_sz / eth_sz
            } else {
                fallback_eth_price
            },
            btc_price: if btc_sz > Decimal::ZERO {
                btc_px_sz / btc_sz
            } else {
                fallback_btc_price
            },
            fee,
            exchange_closed_pnl: Some(closed_pnl),
            realized_pnl: closed_pnl - fee,
            source: PnlSource::ExchangeFills,
        }
    }

    fn exposure_matches_position(exposure: &PairExposure, position: &PositionSnapshot) -> bool {
        exposure.eth_qty() == position.eth.qty && exposure.btc_qty() == position.btc.qty
    }

    async fn residual_repair_trade_log(
        &mut self,
        position: &PositionSnapshot,
        timestamp: DateTime<Utc>,
        eth_price: Decimal,
        btc_price: Decimal,
        repair_fill: Option<&(Symbol, OrderFill)>,
    ) -> TradeLog {
        let fallback_eth_price = match repair_fill {
            Some((Symbol::EthPerp, fill)) => fill.avg_price.unwrap_or(eth_price),
            _ => eth_price,
        };
        let fallback_btc_price = match repair_fill {
            Some((Symbol::BtcPerp, fill)) => fill.avg_price.unwrap_or(btc_price),
            _ => btc_price,
        };
        let model_realized_pnl =
            compute_position_pnl(position, fallback_eth_price, fallback_btc_price);
        let accounting = if let Some((symbol, fill)) = repair_fill {
            self.fill_accounting_for_order_ids(
                &[fill.oid],
                &[*symbol],
                timestamp - Duration::minutes(5),
                timestamp,
                fallback_eth_price,
                fallback_btc_price,
                model_realized_pnl,
            )
            .await
            .unwrap_or_else(|| {
                Self::model_accounting(fallback_eth_price, fallback_btc_price, model_realized_pnl)
            })
        } else {
            Self::model_accounting(fallback_eth_price, fallback_btc_price, model_realized_pnl)
        };
        self.add_realized_pnl(accounting.realized_pnl);
        TradeLog {
            timestamp,
            event: TradeEvent::ResidualRepair,
            direction: position.direction,
            eth_qty: position.eth.qty,
            btc_qty: position.btc.qty,
            eth_price: accounting.eth_price,
            btc_price: accounting.btc_price,
            entry_time: position.entry_time,
            entry_eth_price: position.eth.avg_price,
            entry_btc_price: position.btc.avg_price,
            realized_pnl: accounting.realized_pnl,
            cumulative_realized_pnl: self.cumulative_realized_pnl,
            fee: accounting.fee,
            exchange_closed_pnl: accounting.exchange_closed_pnl,
            pnl_source: accounting.source,
            eth_ref_price: None,
            btc_ref_price: None,
            eth_slippage_bps: None,
            btc_slippage_bps: None,
        }
    }
}

fn select_price(
    field: PriceField,
    mid: Option<Decimal>,
    mark: Option<Decimal>,
    close: Option<Decimal>,
) -> Option<Decimal> {
    match field {
        PriceField::Mid => mid.or(mark).or(close),
        PriceField::Mark => mark.or(mid).or(close),
        PriceField::Close => close.or(mid).or(mark),
    }
}

fn estimate_half_life_bars_decimal(values: &VecDeque<Decimal>) -> Option<f64> {
    if values.len() < 3 {
        return None;
    }
    let series = values
        .iter()
        .map(|value| value.to_f64())
        .collect::<Option<Vec<_>>>()?;
    let mut x = Vec::with_capacity(series.len().saturating_sub(1));
    let mut y = Vec::with_capacity(series.len().saturating_sub(1));
    for idx in 1..series.len() {
        x.push(series[idx - 1]);
        y.push(series[idx] - series[idx - 1]);
    }
    let mean_x = x.iter().sum::<f64>() / x.len() as f64;
    let mean_y = y.iter().sum::<f64>() / y.len() as f64;
    let mut numerator = 0.0;
    let mut denominator = 0.0;
    for (xi, yi) in x.iter().zip(y.iter()) {
        numerator += (xi - mean_x) * (yi - mean_y);
        denominator += (xi - mean_x).powi(2);
    }
    if denominator <= f64::EPSILON {
        return None;
    }
    let slope = numerator / denominator;
    if slope >= 0.0 || !slope.is_finite() {
        return None;
    }
    let half_life = -std::f64::consts::LN_2 / slope;
    if half_life.is_finite() && half_life > 0.0 {
        Some(half_life)
    } else {
        None
    }
}

fn entry_eth_side(direction: TradeDirection) -> OrderSide {
    match direction {
        TradeDirection::LongEthShortBtc => OrderSide::Buy,
        TradeDirection::ShortEthLongBtc => OrderSide::Sell,
    }
}

fn entry_btc_side(direction: TradeDirection) -> OrderSide {
    match direction {
        TradeDirection::LongEthShortBtc => OrderSide::Sell,
        TradeDirection::ShortEthLongBtc => OrderSide::Buy,
    }
}

fn slippage_bps_for_side(
    reference_price: Decimal,
    fill_price: Decimal,
    side: OrderSide,
) -> Decimal {
    if reference_price <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    let signed_cost = match side {
        OrderSide::Buy => fill_price - reference_price,
        OrderSide::Sell => reference_price - fill_price,
    };
    signed_cost / reference_price * Decimal::from(10_000u32)
}

fn compute_position_pnl(
    position: &PositionSnapshot,
    eth_price: Decimal,
    btc_price: Decimal,
) -> Decimal {
    let eth_pnl = position.eth.qty * (eth_price - position.eth.avg_price);
    let btc_pnl = position.btc.qty * (btc_price - position.btc.avg_price);
    eth_pnl + btc_pnl
}
