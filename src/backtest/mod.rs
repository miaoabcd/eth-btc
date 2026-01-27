use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Datelike, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{Config, Symbol};
use crate::core::{ExitReason, TradeDirection};
use crate::funding::{FundingRate, estimate_funding_cost};
use crate::indicators::{VolatilityCalculator, ZScoreCalculator, relative_price};
use crate::logging::BarLog;
use crate::position::{MinSizePolicy, SizeConverter, compute_capital, risk_parity_weights};
use crate::signals::{EntrySignalDetector, ExitSignalDetector};
use crate::state::{PositionLeg, PositionSnapshot, StateMachine};

#[derive(Debug, Error)]
pub enum BacktestError {
    #[error("indicator error: {0}")]
    Indicator(String),
    #[error("position error: {0}")]
    Position(String),
    #[error("funding error: {0}")]
    Funding(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("serialization error: {0}")]
    Serialization(String),
}

#[derive(Debug, Clone)]
pub struct BacktestBar {
    pub timestamp: DateTime<Utc>,
    pub eth_price: Decimal,
    pub btc_price: Decimal,
    pub funding_eth: Option<Decimal>,
    pub funding_btc: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TradeExitReason {
    TakeProfit,
    StopLoss,
    TimeStop,
}

impl From<ExitReason> for TradeExitReason {
    fn from(reason: ExitReason) -> Self {
        match reason {
            ExitReason::TakeProfit => TradeExitReason::TakeProfit,
            ExitReason::StopLoss => TradeExitReason::StopLoss,
            ExitReason::TimeStop => TradeExitReason::TimeStop,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trade {
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub pnl: Decimal,
    pub exit_reason: TradeExitReason,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EquityPoint {
    pub timestamp: DateTime<Utc>,
    pub equity: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub annualized_return: Decimal,
    pub sharpe_ratio: Decimal,
    pub max_drawdown: Decimal,
    pub win_rate: Decimal,
    pub profit_factor: Decimal,
    pub stop_loss_rate: Decimal,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            annualized_return: Decimal::ZERO,
            sharpe_ratio: Decimal::ZERO,
            max_drawdown: Decimal::ZERO,
            win_rate: Decimal::ZERO,
            profit_factor: Decimal::ZERO,
            stop_loss_rate: Decimal::ZERO,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BacktestResult {
    pub trades: Vec<Trade>,
    pub equity_curve: Vec<EquityPoint>,
    pub bar_logs: Vec<BarLog>,
    pub metrics: Metrics,
}

#[derive(Debug, Clone)]
pub struct BacktestEngine {
    config: Config,
}

impl BacktestEngine {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn run(&self, bars: &[BacktestBar]) -> Result<BacktestResult, BacktestError> {
        let mut zcalc = ZScoreCalculator::new(
            self.config.strategy.n_z,
            self.config.sigma_floor.clone(),
            96,
        )
        .map_err(|err| BacktestError::Indicator(err.to_string()))?;
        let mut volcalc = VolatilityCalculator::new(self.config.position.n_vol)
            .map_err(|err| BacktestError::Indicator(err.to_string()))?;
        let mut entry_detector = EntrySignalDetector::new(self.config.strategy.clone());
        let mut exit_detector =
            ExitSignalDetector::new(self.config.strategy.clone(), self.config.risk.clone());
        let mut state_machine = StateMachine::new(self.config.risk.clone());

        let mut trades = Vec::new();
        let mut equity_curve = Vec::new();
        let mut bar_logs = Vec::new();
        let mut equity = self
            .config
            .position
            .c_value
            .unwrap_or(Decimal::new(100000, 0));

        let mut open_trade: Option<(PositionSnapshot, Decimal, Decimal)> = None;

        for bar in bars {
            let r = relative_price(bar.eth_price, bar.btc_price)
                .map_err(|err| BacktestError::Indicator(err.to_string()))?;
            let z_snapshot = zcalc
                .update(r)
                .map_err(|err| BacktestError::Indicator(err.to_string()))?;
            let vol_snapshot = volcalc
                .update(bar.eth_price, bar.btc_price)
                .map_err(|err| BacktestError::Indicator(err.to_string()))?;

            let entry_signal =
                entry_detector.update(z_snapshot.zscore, state_machine.state().status);
            if let Some(signal) = entry_signal
                && vol_snapshot.vol_eth.is_some()
                && vol_snapshot.vol_btc.is_some()
            {
                let capital = compute_capital(&self.config.position, equity)
                    .map_err(|err| BacktestError::Position(err.to_string()))?;
                let weights = risk_parity_weights(
                    vol_snapshot.vol_eth.unwrap(),
                    vol_snapshot.vol_btc.unwrap(),
                )
                .map_err(|err| BacktestError::Position(err.to_string()))?;
                let notional_eth = capital * weights.w_eth;
                let notional_btc = capital * weights.w_btc;

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
                    .map_err(|err| BacktestError::Position(err.to_string()))?;
                let btc_order = btc_converter
                    .convert_notional(notional_btc, bar.btc_price)
                    .map_err(|err| BacktestError::Position(err.to_string()))?;

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
                state_machine
                    .enter(position.clone(), bar.timestamp)
                    .map_err(|err| BacktestError::Position(err.to_string()))?;
                open_trade = Some((position, bar.eth_price, bar.btc_price));
            }

            let exit_signal = exit_detector.evaluate(
                z_snapshot.zscore,
                state_machine.state().status,
                state_machine.state().position.as_ref(),
                bar.timestamp,
            );
            if let Some(exit_signal) = exit_signal
                && let Some((position, entry_eth, entry_btc)) = open_trade.take()
            {
                let pnl = compute_trade_pnl(
                    TradeInput {
                        direction: position.direction,
                        entry_eth,
                        entry_btc,
                        exit_eth: bar.eth_price,
                        exit_btc: bar.btc_price,
                        notional_eth: position.eth.notional,
                        notional_btc: position.btc.notional,
                        bar,
                    },
                    &self.config,
                )?;
                equity += pnl;
                trades.push(Trade {
                    entry_time: position.entry_time,
                    exit_time: bar.timestamp,
                    pnl,
                    exit_reason: exit_signal.reason.into(),
                });
                state_machine
                    .exit(exit_signal.reason, bar.timestamp)
                    .map_err(|err| BacktestError::Position(err.to_string()))?;
            }

            equity_curve.push(EquityPoint {
                timestamp: bar.timestamp,
                equity,
            });

            bar_logs.push(BarLog {
                timestamp: bar.timestamp,
                eth_price: Some(bar.eth_price),
                btc_price: Some(bar.btc_price),
                r: Some(r),
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
                state: state_machine.state().status,
                position: state_machine.state().position.clone(),
                events: Vec::new(),
            });
        }

        let metrics = compute_metrics(&trades, &equity_curve, Decimal::ZERO)?;

        Ok(BacktestResult {
            trades,
            equity_curve,
            bar_logs,
            metrics,
        })
    }
}

struct TradeInput<'a> {
    direction: TradeDirection,
    entry_eth: Decimal,
    entry_btc: Decimal,
    exit_eth: Decimal,
    exit_btc: Decimal,
    notional_eth: Decimal,
    notional_btc: Decimal,
    bar: &'a BacktestBar,
}

fn compute_trade_pnl(input: TradeInput<'_>, config: &Config) -> Result<Decimal, BacktestError> {
    let pnl_eth = match input.direction {
        TradeDirection::LongEthShortBtc => {
            (input.exit_eth - input.entry_eth) / input.entry_eth * input.notional_eth
        }
        TradeDirection::ShortEthLongBtc => {
            (input.entry_eth - input.exit_eth) / input.entry_eth * input.notional_eth
        }
    };
    let pnl_btc = match input.direction {
        TradeDirection::LongEthShortBtc => {
            (input.entry_btc - input.exit_btc) / input.entry_btc * input.notional_btc
        }
        TradeDirection::ShortEthLongBtc => {
            (input.exit_btc - input.entry_btc) / input.entry_btc * input.notional_btc
        }
    };
    let mut pnl = pnl_eth + pnl_btc;

    let total_notional = input.notional_eth + input.notional_btc;
    let fee_bps = Decimal::from(config.backtest.fee_bps) / Decimal::new(10000, 0);
    let slippage_bps = Decimal::from(config.backtest.slippage_bps) / Decimal::new(10000, 0);
    let cost = total_notional * (fee_bps + slippage_bps);
    pnl -= cost;

    if config.backtest.include_funding
        && let (Some(funding_eth), Some(funding_btc)) =
            (input.bar.funding_eth, input.bar.funding_btc)
    {
        let eth_rate = FundingRate {
            symbol: Symbol::EthPerp,
            rate: funding_eth,
            timestamp: input.bar.timestamp,
            interval_hours: 8,
        };
        let btc_rate = FundingRate {
            symbol: Symbol::BtcPerp,
            rate: funding_btc,
            timestamp: input.bar.timestamp,
            interval_hours: 8,
        };
        let estimate = estimate_funding_cost(
            input.direction,
            input.notional_eth,
            input.notional_btc,
            &eth_rate,
            &btc_rate,
            config.risk.max_hold_hours,
        )
        .map_err(|err| BacktestError::Funding(err.to_string()))?;
        pnl -= estimate.cost_est;
    }

    Ok(pnl)
}

pub fn compute_metrics(
    trades: &[Trade],
    equity_curve: &[EquityPoint],
    _risk_free_rate: Decimal,
) -> Result<Metrics, BacktestError> {
    if trades.is_empty() || equity_curve.len() < 2 {
        return Ok(Metrics::default());
    }
    let wins = trades
        .iter()
        .filter(|trade| trade.pnl > Decimal::ZERO)
        .count();
    let win_rate = Decimal::from(wins as u64) / Decimal::from(trades.len() as u64);

    let profit = trades
        .iter()
        .filter(|trade| trade.pnl > Decimal::ZERO)
        .fold(Decimal::ZERO, |acc, trade| acc + trade.pnl);
    let loss = trades
        .iter()
        .filter(|trade| trade.pnl < Decimal::ZERO)
        .fold(Decimal::ZERO, |acc, trade| acc + trade.pnl.abs());
    let profit_factor = if loss == Decimal::ZERO {
        Decimal::ZERO
    } else {
        profit / loss
    };

    let stop_loss = trades
        .iter()
        .filter(|trade| trade.exit_reason == TradeExitReason::StopLoss)
        .count();
    let stop_loss_rate = Decimal::from(stop_loss as u64) / Decimal::from(trades.len() as u64);

    Ok(Metrics {
        win_rate,
        profit_factor,
        stop_loss_rate,
        ..Metrics::default()
    })
}

pub fn export_metrics_json(path: &Path, metrics: &Metrics) -> Result<(), BacktestError> {
    let payload = serde_json::to_string_pretty(metrics)
        .map_err(|err| BacktestError::Serialization(err.to_string()))?;
    fs::write(path, payload).map_err(|err| BacktestError::Io(err.to_string()))
}

pub fn export_trades_csv(path: &Path, trades: &[Trade]) -> Result<(), BacktestError> {
    let mut contents = String::from("entry_time,exit_time,pnl,exit_reason\n");
    for trade in trades {
        contents.push_str(&format!(
            "{},{},{},{:?}\n",
            trade.entry_time.to_rfc3339(),
            trade.exit_time.to_rfc3339(),
            trade.pnl,
            trade.exit_reason
        ));
    }
    fs::write(path, contents).map_err(|err| BacktestError::Io(err.to_string()))
}

pub fn export_equity_csv(path: &Path, equity: &[EquityPoint]) -> Result<(), BacktestError> {
    let mut contents = String::from("timestamp,equity\n");
    for point in equity {
        contents.push_str(&format!(
            "{},{}\n",
            point.timestamp.to_rfc3339(),
            point.equity
        ));
    }
    fs::write(path, contents).map_err(|err| BacktestError::Io(err.to_string()))
}

pub fn run_sensitivity(
    configs: &[Config],
    bars: &[BacktestBar],
) -> Result<Vec<BacktestResult>, BacktestError> {
    let mut results = Vec::new();
    for config in configs {
        let engine = BacktestEngine::new(config.clone());
        results.push(engine.run(bars)?);
    }
    Ok(results)
}

#[derive(Debug, Clone, PartialEq)]
pub struct BreakdownRow {
    pub year: i32,
    pub month: u32,
    pub pnl: Decimal,
}

pub fn breakdown_monthly(trades: &[Trade]) -> Vec<BreakdownRow> {
    let mut grouped: BTreeMap<(i32, u32), Decimal> = BTreeMap::new();
    for trade in trades {
        let key = (trade.exit_time.year(), trade.exit_time.month());
        let entry = grouped.entry(key).or_insert(Decimal::ZERO);
        *entry += trade.pnl;
    }
    grouped
        .into_iter()
        .map(|((year, month), pnl)| BreakdownRow { year, month, pnl })
        .collect()
}

pub fn verify_reproducibility(config: &Config, bars: &[BacktestBar]) -> Result<(), BacktestError> {
    let engine = BacktestEngine::new(config.clone());
    let first = engine.run(bars)?;
    let second = engine.run(bars)?;
    if first.trades.len() != second.trades.len() {
        return Err(BacktestError::Position("trade count mismatch".to_string()));
    }
    Ok(())
}
