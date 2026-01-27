use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use thiserror::Error;

use crate::config::{Config, Symbol};
use crate::core::TradeDirection;
use crate::execution::{ExecutionEngine, OrderRequest, OrderSide};
use crate::funding::{FundingRate, apply_funding_controls, estimate_funding_cost};
use crate::indicators::{VolatilityCalculator, ZScoreCalculator, relative_price};
use crate::logging::{BarLog, LogEvent};
use crate::position::{MinSizePolicy, SizeConverter, compute_capital, risk_parity_weights};
use crate::signals::{EntrySignalDetector, ExitSignalDetector};
use crate::state::{PositionLeg, PositionSnapshot, StateMachine, StrategyStatus};

#[derive(Debug, Clone)]
pub struct StrategyBar {
    pub timestamp: DateTime<Utc>,
    pub eth_price: Decimal,
    pub btc_price: Decimal,
    pub funding_eth: Option<Decimal>,
    pub funding_btc: Option<Decimal>,
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
    zcalc: ZScoreCalculator,
    volcalc: VolatilityCalculator,
    entry_detector: EntrySignalDetector,
    exit_detector: ExitSignalDetector,
    state_machine: StateMachine,
    execution: ExecutionEngine,
}

impl StrategyEngine {
    pub fn new(config: Config, execution: ExecutionEngine) -> Result<Self, StrategyError> {
        let zcalc = ZScoreCalculator::new(config.strategy.n_z, config.sigma_floor.clone(), 96)
            .map_err(|err| StrategyError::Indicator(err.to_string()))?;
        let volcalc = VolatilityCalculator::new(config.position.n_vol)
            .map_err(|err| StrategyError::Indicator(err.to_string()))?;
        Ok(Self {
            zcalc,
            volcalc,
            entry_detector: EntrySignalDetector::new(config.strategy.clone()),
            exit_detector: ExitSignalDetector::new(config.strategy.clone(), config.risk.clone()),
            state_machine: StateMachine::new(config.risk.clone()),
            config,
            execution,
        })
    }

    pub fn state(&self) -> &StateMachine {
        &self.state_machine
    }

    pub async fn process_bar(
        &mut self,
        bar: StrategyBar,
    ) -> Result<StrategyOutcome, StrategyError> {
        self.state_machine.update(bar.timestamp);
        let r = relative_price(bar.eth_price, bar.btc_price)
            .map_err(|err| StrategyError::Indicator(err.to_string()))?;
        let z_snapshot = self
            .zcalc
            .update(r)
            .map_err(|err| StrategyError::Indicator(err.to_string()))?;
        let vol_snapshot = self
            .volcalc
            .update(bar.eth_price, bar.btc_price)
            .map_err(|err| StrategyError::Indicator(err.to_string()))?;

        let mut events = Vec::new();

        if let Some(signal) = self
            .entry_detector
            .update(z_snapshot.zscore, self.state_machine.state().status)
            && let Some(vol_eth) = vol_snapshot.vol_eth
            && let Some(vol_btc) = vol_snapshot.vol_btc
        {
            let capital = compute_capital(
                &self.config.position,
                self.config.position.c_value.unwrap_or(Decimal::ZERO),
            )
            .map_err(|err| StrategyError::Position(err.to_string()))?;
            let weights = risk_parity_weights(vol_eth, vol_btc)
                .map_err(|err| StrategyError::Position(err.to_string()))?;
            let notional_eth = capital * weights.w_eth;
            let notional_btc = capital * weights.w_btc;

            if let (Some(funding_eth), Some(funding_btc)) = (bar.funding_eth, bar.funding_btc) {
                let eth_rate = FundingRate {
                    symbol: Symbol::EthPerp,
                    rate: funding_eth,
                    timestamp: bar.timestamp,
                    interval_hours: 8,
                };
                let btc_rate = FundingRate {
                    symbol: Symbol::BtcPerp,
                    rate: funding_btc,
                    timestamp: bar.timestamp,
                    interval_hours: 8,
                };
                let estimate = estimate_funding_cost(
                    signal.direction,
                    notional_eth,
                    notional_btc,
                    &eth_rate,
                    &btc_rate,
                    self.config.risk.max_hold_hours,
                )
                .map_err(|err| StrategyError::Funding(err.to_string()))?;
                let decision = apply_funding_controls(
                    &self.config.funding,
                    self.config.strategy.entry_z,
                    capital,
                    &estimate,
                )
                .map_err(|err| StrategyError::Funding(err.to_string()))?;
                if decision.should_skip {
                    return Ok(self.build_outcome(bar, z_snapshot, vol_snapshot, events));
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
                .convert_notional(notional_eth, bar.eth_price)
                .map_err(|err| StrategyError::Position(err.to_string()))?;
            let btc_order = btc_converter
                .convert_notional(notional_btc, bar.btc_price)
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
                        limit_price: None,
                    },
                    OrderRequest {
                        symbol: Symbol::BtcPerp,
                        side: btc_side,
                        qty: btc_order.qty,
                        order_type: self.config.execution.order_type,
                        limit_price: None,
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
                    notional: notional_eth,
                },
                btc: PositionLeg {
                    qty: if signal.direction == TradeDirection::LongEthShortBtc {
                        -btc_order.qty
                    } else {
                        btc_order.qty
                    },
                    avg_price: bar.btc_price,
                    notional: notional_btc,
                },
            };
            self.state_machine
                .enter(position, bar.timestamp)
                .map_err(|err| StrategyError::Position(err.to_string()))?;
            events.push(LogEvent::Entry);
        }

        if let Some(exit_signal) = self.exit_detector.evaluate(
            z_snapshot.zscore,
            self.state_machine.state().status,
            self.state_machine.state().position.as_ref(),
            bar.timestamp,
        ) && let Some(position) = self.state_machine.state().position.clone()
        {
            let eth_order = OrderRequest {
                symbol: Symbol::EthPerp,
                side: OrderSide::close_for_qty(position.eth.qty),
                qty: position.eth.qty.abs(),
                order_type: self.config.execution.order_type,
                limit_price: None,
            };
            let btc_order = OrderRequest {
                symbol: Symbol::BtcPerp,
                side: OrderSide::close_for_qty(position.btc.qty),
                qty: position.btc.qty.abs(),
                order_type: self.config.execution.order_type,
                limit_price: None,
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

        Ok(self.build_outcome(bar, z_snapshot, vol_snapshot, events))
    }

    fn build_outcome(
        &self,
        bar: StrategyBar,
        z_snapshot: crate::indicators::ZScoreSnapshot,
        vol_snapshot: crate::indicators::VolatilitySnapshot,
        events: Vec<LogEvent>,
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
                w_eth: None,
                w_btc: None,
                notional_eth: None,
                notional_btc: None,
                funding_eth: bar.funding_eth,
                funding_btc: bar.funding_btc,
                funding_cost_est: None,
                funding_skip: None,
                state: self.state_machine.state().status,
                position: self.state_machine.state().position.clone(),
                events,
            },
        }
    }
}
