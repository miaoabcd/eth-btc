pub mod download;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Datelike, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{Config, PriceField, Symbol};
use crate::core::pipeline::SignalPipeline;
use crate::core::{ExitReason, TradeDirection};
use crate::data::align_to_bar_close;
use crate::funding::{FundingRate, estimate_funding_cost};
use crate::logging::BarLog;
use crate::position::{
    MinSizePolicy, PositionError, SizeConverter, compute_capital, risk_parity_weights,
};
use crate::state::{PositionLeg, PositionSnapshot, StateMachine};
use crate::storage::PriceStore;

#[derive(Debug, Error)]
pub enum BacktestError {
    #[error("indicator error: {0}")]
    Indicator(String),
    #[error("position error: {0}")]
    Position(String),
    #[error("funding error: {0}")]
    Funding(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("serialization error: {0}")]
    Serialization(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub trade_count: usize,
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
            trade_count: 0,
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
        let mut pipeline = SignalPipeline::new(&self.config)
            .map_err(|err| BacktestError::Indicator(err.to_string()))?;
        let mut state_machine = StateMachine::new(self.config.risk.clone());

        let mut trades = Vec::new();
        let mut equity_curve = Vec::new();
        let mut bar_logs = Vec::new();
        let mut equity = match self.config.position.c_mode {
            crate::config::CapitalMode::FixedNotional => self
                .config
                .position
                .c_value
                .unwrap_or(Decimal::new(100000, 0)),
            crate::config::CapitalMode::EquityRatio => {
                self.config.position.equity_value.ok_or_else(|| {
                    BacktestError::Position(
                        "equity_value required for equity ratio mode".to_string(),
                    )
                })?
            }
        };

        let mut open_trade: Option<(PositionSnapshot, Decimal, Decimal)> = None;

        for bar in bars {
            let output = pipeline
                .update(
                    bar.timestamp,
                    bar.eth_price,
                    bar.btc_price,
                    state_machine.state().status,
                    state_machine.state().position.as_ref(),
                )
                .map_err(|err| BacktestError::Indicator(err.to_string()))?;
            let r = output.r;
            let z_snapshot = output.z_snapshot;
            let vol_snapshot = output.vol_snapshot;
            let entry_signal = output.entry_signal;
            let exit_signal = output.exit_signal;
            if let Some(signal) = entry_signal
                && vol_snapshot.vol_eth.is_some()
                && vol_snapshot.vol_btc.is_some()
            {
                let capital = compute_capital(&self.config.position, equity)
                    .map_err(|err| BacktestError::Position(err.to_string()))?;
                if let Some(max_notional) = self.config.position.max_notional
                    && capital > max_notional
                {
                    return Err(BacktestError::Position(format!(
                        "capital {capital} exceeds max_notional {max_notional}"
                    )));
                }
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
                let eth_order = match eth_converter.convert_notional(notional_eth, bar.eth_price) {
                    Ok(order) => Some(order),
                    Err(PositionError::BelowMinimum(_)) => None,
                    Err(err) => return Err(BacktestError::Position(err.to_string())),
                };
                let btc_order = match btc_converter.convert_notional(notional_btc, bar.btc_price) {
                    Ok(order) => Some(order),
                    Err(PositionError::BelowMinimum(_)) => None,
                    Err(err) => return Err(BacktestError::Position(err.to_string())),
                };

                if let (Some(eth_order), Some(btc_order)) = (eth_order, btc_order) {
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
            }

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
                        holding_hours: (bar.timestamp - position.entry_time).num_hours().max(0)
                            as u32,
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
    holding_hours: u32,
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
    let mut cost = Decimal::ZERO;
    if config.backtest.include_fees {
        cost += total_notional * fee_bps;
    }
    if config.backtest.include_slippage {
        cost += total_notional * slippage_bps;
    }
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
            input.holding_hours,
        )
        .map_err(|err| BacktestError::Funding(err.to_string()))?;
        pnl -= estimate.cost_est;
    }

    Ok(pnl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rust_decimal_macros::dec;

    #[test]
    fn funding_cost_uses_holding_hours() {
        let mut config = Config::default();
        config.backtest.include_fees = false;
        config.backtest.include_slippage = false;
        config.backtest.include_funding = true;
        config.backtest.fee_bps = 0;
        config.backtest.slippage_bps = 0;

        let bar = BacktestBar {
            timestamp: Utc::now(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            funding_eth: Some(dec!(0.001)),
            funding_btc: Some(dec!(0.01)),
        };

        let input = TradeInput {
            direction: TradeDirection::ShortEthLongBtc,
            entry_eth: dec!(100),
            entry_btc: dec!(100),
            exit_eth: dec!(100),
            exit_btc: dec!(100),
            notional_eth: dec!(50),
            notional_btc: dec!(50),
            bar: &bar,
            holding_hours: 2,
        };

        let expected = estimate_funding_cost(
            input.direction,
            input.notional_eth,
            input.notional_btc,
            &FundingRate {
                symbol: Symbol::EthPerp,
                rate: dec!(0.001),
                timestamp: bar.timestamp,
                interval_hours: 8,
            },
            &FundingRate {
                symbol: Symbol::BtcPerp,
                rate: dec!(0.01),
                timestamp: bar.timestamp,
                interval_hours: 8,
            },
            input.holding_hours,
        )
        .unwrap();

        let pnl = compute_trade_pnl(input, &config).unwrap();
        assert_eq!(pnl, -expected.cost_est);
    }

    #[test]
    fn trade_pnl_respects_fee_and_slippage_flags() {
        let mut config = Config::default();
        config.backtest.include_fees = false;
        config.backtest.include_slippage = false;
        config.backtest.include_funding = false;
        config.backtest.fee_bps = 100;
        config.backtest.slippage_bps = 100;

        let bar = BacktestBar {
            timestamp: Utc::now(),
            eth_price: dec!(100),
            btc_price: dec!(100),
            funding_eth: None,
            funding_btc: None,
        };

        let input = TradeInput {
            direction: TradeDirection::LongEthShortBtc,
            entry_eth: dec!(100),
            entry_btc: dec!(100),
            exit_eth: dec!(100),
            exit_btc: dec!(100),
            notional_eth: dec!(50),
            notional_btc: dec!(50),
            bar: &bar,
            holding_hours: 1,
        };

        let pnl = compute_trade_pnl(input, &config).unwrap();
        assert_eq!(pnl, Decimal::ZERO);
    }
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

    let start = equity_curve.first().expect("checked len");
    let end = equity_curve.last().expect("checked len");
    let duration_secs = (end.timestamp - start.timestamp).num_seconds();
    let years = if duration_secs > 0 {
        duration_secs as f64 / 31_536_000.0
    } else {
        0.0
    };
    let annualized_return =
        if start.equity > Decimal::ZERO && end.equity > Decimal::ZERO && years > 0.0 {
            let total_return = end.equity / start.equity - Decimal::ONE;
            let base = Decimal::ONE + total_return;
            if let Some(base_f64) = base.to_f64() {
                let ann = base_f64.powf(1.0 / years) - 1.0;
                Decimal::from_f64(ann).unwrap_or(Decimal::ZERO)
            } else {
                Decimal::ZERO
            }
        } else {
            Decimal::ZERO
        };

    let mut peak = Decimal::ZERO;
    let mut max_drawdown = Decimal::ZERO;
    for point in equity_curve {
        if point.equity > peak {
            peak = point.equity;
        }
        if peak > Decimal::ZERO {
            let drawdown = (peak - point.equity) / peak;
            if drawdown > max_drawdown {
                max_drawdown = drawdown;
            }
        }
    }

    let mut returns = Vec::with_capacity(equity_curve.len().saturating_sub(1));
    let mut total_delta = 0i64;
    for window in equity_curve.windows(2) {
        if let [prev, next] = window
            && prev.equity > Decimal::ZERO
        {
            returns.push((next.equity / prev.equity - Decimal::ONE).to_f64());
            total_delta += (next.timestamp - prev.timestamp).num_seconds();
        }
    }
    let sharpe_ratio = if returns.len() >= 2 && total_delta > 0 {
        let valid_returns: Vec<f64> = returns.into_iter().flatten().collect();
        if valid_returns.len() >= 2 {
            let mean = valid_returns.iter().copied().sum::<f64>() / valid_returns.len() as f64;
            let denom = valid_returns.len() as f64 - 1.0;
            let variance = if denom > 0.0 {
                valid_returns
                    .iter()
                    .map(|value| {
                        let diff = value - mean;
                        diff * diff
                    })
                    .sum::<f64>()
                    / denom
            } else {
                0.0
            };
            let std = variance.sqrt();
            let avg_delta = total_delta as f64 / valid_returns.len() as f64;
            let periods_per_year = if avg_delta > 0.0 {
                31_536_000.0 / avg_delta
            } else {
                0.0
            };
            let risk_free_rate = _risk_free_rate.to_f64().unwrap_or(0.0);
            let rf_per_period = if periods_per_year > 0.0 {
                (1.0 + risk_free_rate).powf(1.0 / periods_per_year) - 1.0
            } else {
                0.0
            };
            if std > 0.0 {
                let sharpe = (mean - rf_per_period) / std * periods_per_year.sqrt();
                Decimal::from_f64(sharpe).unwrap_or(Decimal::ZERO)
            } else {
                Decimal::ZERO
            }
        } else {
            Decimal::ZERO
        }
    } else {
        Decimal::ZERO
    };

    Ok(Metrics {
        win_rate,
        profit_factor,
        stop_loss_rate,
        annualized_return,
        sharpe_ratio,
        max_drawdown,
        trade_count: trades.len(),
    })
}

pub fn export_metrics_json(path: &Path, metrics: &Metrics) -> Result<(), BacktestError> {
    let payload = serde_json::to_string_pretty(metrics)
        .map_err(|err| BacktestError::Serialization(err.to_string()))?;
    fs::write(path, payload).map_err(|err| BacktestError::Io(err.to_string()))
}

pub fn load_backtest_bars(path: &Path) -> Result<Vec<BacktestBar>, BacktestError> {
    let payload = fs::read_to_string(path).map_err(|err| BacktestError::Io(err.to_string()))?;
    serde_json::from_str(&payload).map_err(|err| BacktestError::Serialization(err.to_string()))
}

pub fn load_backtest_bars_from_db(
    path: &Path,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    price_field: PriceField,
) -> Result<Vec<BacktestBar>, BacktestError> {
    let start = align_to_bar_close(start).map_err(|err| BacktestError::Storage(err.to_string()))?;
    let end = align_to_bar_close(end).map_err(|err| BacktestError::Storage(err.to_string()))?;
    let store = PriceStore::new(path.to_string_lossy().as_ref())
        .map_err(|err| BacktestError::Storage(err.to_string()))?;
    let records = store
        .load_range(start, end)
        .map_err(|err| BacktestError::Storage(err.to_string()))?;
    let first = records.first().map(|record| record.timestamp);
    let last = records.last().map(|record| record.timestamp);
    if first.map(|ts| ts > start).unwrap_or(true) || last.map(|ts| ts < end).unwrap_or(true) {
        return Err(BacktestError::Storage(format!(
            "incomplete coverage: start {start} end {end} first {first:?} last {last:?}"
        )));
    }
    let mut bars = Vec::new();
    for record in records {
        let eth_price = effective_price(
            price_field,
            record.eth_mid,
            record.eth_mark,
            record.eth_close,
        )
        .ok_or_else(|| BacktestError::Storage("missing ETH price".to_string()))?;
        let btc_price = effective_price(
            price_field,
            record.btc_mid,
            record.btc_mark,
            record.btc_close,
        )
        .ok_or_else(|| BacktestError::Storage("missing BTC price".to_string()))?;
        bars.push(BacktestBar {
            timestamp: record.timestamp,
            eth_price,
            btc_price,
            funding_eth: record.funding_eth,
            funding_btc: record.funding_btc,
        });
    }
    Ok(bars)
}

fn effective_price(
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
