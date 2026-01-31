use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use thiserror::Error;

use crate::config::{CapitalMode, Config, Symbol};
use crate::core::TradeDirection;
use crate::core::pipeline::SignalPipeline;
use crate::execution::{ExecutionEngine, OrderRequest, OrderSide};
use crate::funding::{FundingRate, apply_funding_controls, estimate_funding_cost};
use crate::logging::{BarLog, LogEvent};
use crate::position::{MinSizePolicy, SizeConverter, compute_capital, risk_parity_weights};
use crate::state::{PositionLeg, PositionSnapshot, StateMachine, StrategyState, StrategyStatus};

#[derive(Debug, Clone)]
pub struct StrategyBar {
    pub timestamp: DateTime<Utc>,
    pub eth_price: Decimal,
    pub btc_price: Decimal,
    pub funding_eth: Option<Decimal>,
    pub funding_btc: Option<Decimal>,
    pub funding_interval_hours: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct StrategyOutcome {
    pub state: StrategyStatus,
    pub events: Vec<LogEvent>,
    pub bar_log: BarLog,
}

#[derive(Debug, Error)]
pub enum StrategyError {
    #[error("indicator error: {0}")]
    Indicator(String),
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
            config,
            execution,
        })
    }

    pub fn state(&self) -> &StateMachine {
        &self.state_machine
    }

    pub fn apply_state(&mut self, state: StrategyState) -> Result<(), StrategyError> {
        self.state_machine
            .hydrate(state)
            .map_err(|err| StrategyError::Position(err.to_string()))
    }

    pub async fn process_bar(
        &mut self,
        bar: StrategyBar,
    ) -> Result<StrategyOutcome, StrategyError> {
        self.state_machine.update(bar.timestamp);
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

        let mut events = Vec::new();
        let mut w_eth = None;
        let mut w_btc = None;
        let mut notional_eth = None;
        let mut notional_btc = None;
        let mut funding_cost_est = None;
        let mut funding_skip = None;

        if let Some(vol_eth) = vol_snapshot.vol_eth
            && let Some(vol_btc) = vol_snapshot.vol_btc
        {
            let weights = risk_parity_weights(vol_eth, vol_btc)
                .map_err(|err| StrategyError::Position(err.to_string()))?;
            w_eth = Some(weights.w_eth);
            w_btc = Some(weights.w_btc);
        }

        if let Some(signal) = entry_signal
            && let Some(vol_eth) = vol_snapshot.vol_eth
            && let Some(vol_btc) = vol_snapshot.vol_btc
        {
            let equity = match self.config.position.c_mode {
                CapitalMode::FixedNotional => self.config.position.c_value.unwrap_or(Decimal::ZERO),
                CapitalMode::EquityRatio => self.config.position.equity_value.ok_or_else(|| {
                    StrategyError::Position(
                        "position.equity_value required for equity ratio mode".to_string(),
                    )
                })?,
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
                    ));
                }
            }

            let eth_converter = SizeConverter::new(
                self.config
                    .instrument_constraints
                    .get(&Symbol::EthPerp)
                    .cloned()
                    .unwrap_or_default(),
                MinSizePolicy::Skip,
            );
            let btc_converter = SizeConverter::new(
                self.config
                    .instrument_constraints
                    .get(&Symbol::BtcPerp)
                    .cloned()
                    .unwrap_or_default(),
                MinSizePolicy::Skip,
            );
            let eth_order = eth_converter
                .convert_notional(notional_eth_value, bar.eth_price)
                .map_err(|err| StrategyError::Position(err.to_string()))?;
            let btc_order = btc_converter
                .convert_notional(notional_btc_value, bar.btc_price)
                .map_err(|err| StrategyError::Position(err.to_string()))?;

            let (eth_side, btc_side) = match signal.direction {
                TradeDirection::LongEthShortBtc => (OrderSide::Buy, OrderSide::Sell),
                TradeDirection::ShortEthLongBtc => (OrderSide::Sell, OrderSide::Buy),
            };
            self.execution
                .open_pair(
                    OrderRequest {
                        symbol: Symbol::EthPerp,
                        side: eth_side,
                        qty: eth_order.qty,
                        order_type: self.config.execution.order_type,
                        limit_price: Some(self.limit_price(eth_side, bar.eth_price)),
                    },
                    OrderRequest {
                        symbol: Symbol::BtcPerp,
                        side: btc_side,
                        qty: btc_order.qty,
                        order_type: self.config.execution.order_type,
                        limit_price: Some(self.limit_price(btc_side, bar.btc_price)),
                    },
                )
                .await
                .map_err(|err| StrategyError::Execution(err.to_string()))?;

            let position = PositionSnapshot {
                direction: signal.direction,
                entry_time: bar.timestamp,
                eth: PositionLeg {
                    qty: if signal.direction == TradeDirection::LongEthShortBtc {
                        eth_order.qty
                    } else {
                        -eth_order.qty
                    },
                    avg_price: bar.eth_price,
                    notional: notional_eth_value,
                },
                btc: PositionLeg {
                    qty: if signal.direction == TradeDirection::LongEthShortBtc {
                        -btc_order.qty
                    } else {
                        btc_order.qty
                    },
                    avg_price: bar.btc_price,
                    notional: notional_btc_value,
                },
            };
            self.state_machine
                .enter(position, bar.timestamp)
                .map_err(|err| StrategyError::Position(err.to_string()))?;
            events.push(LogEvent::Entry);
        }

        if let Some(exit_signal) = exit_signal
            && let Some(position) = self.state_machine.state().position.clone()
        {
            let eth_side = OrderSide::close_for_qty(position.eth.qty);
            let btc_side = OrderSide::close_for_qty(position.btc.qty);
            let eth_order = OrderRequest {
                symbol: Symbol::EthPerp,
                side: eth_side,
                qty: position.eth.qty.abs(),
                order_type: self.config.execution.order_type,
                limit_price: Some(self.limit_price(eth_side, bar.eth_price)),
            };
            let btc_order = OrderRequest {
                symbol: Symbol::BtcPerp,
                side: btc_side,
                qty: position.btc.qty.abs(),
                order_type: self.config.execution.order_type,
                limit_price: Some(self.limit_price(btc_side, bar.btc_price)),
            };
            self.execution
                .close_pair(eth_order, btc_order)
                .await
                .map_err(|err| StrategyError::Execution(err.to_string()))?;
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
        ))
    }

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
    ) -> StrategyOutcome {
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
                state: self.state_machine.state().status,
                position: self.state_machine.state().position.clone(),
                events,
            },
        }
    }

    fn limit_price(&self, side: OrderSide, price: Decimal) -> Decimal {
        let slippage = Decimal::from(self.config.execution.slippage_bps)
            / Decimal::new(10000, 0);
        match side {
            OrderSide::Buy => price * (Decimal::ONE + slippage),
            OrderSide::Sell => price * (Decimal::ONE - slippage),
        }
    }
}
