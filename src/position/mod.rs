use rust_decimal::Decimal;
use thiserror::Error;

use crate::config::{CapitalMode, InstrumentConstraints, PositionConfig, RoundingMode};

#[derive(Debug, Error)]
pub enum RiskParityError {
    #[error("volatility must be positive")]
    InvalidVolatility,
}

#[derive(Debug, Error)]
pub enum CapitalError {
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
}

#[derive(Debug, Error)]
pub enum PositionError {
    #[error("invalid price: {0}")]
    InvalidPrice(String),
    #[error("below minimum: {0}")]
    BelowMinimum(String),
    #[error("invalid constraints: {0}")]
    InvalidConstraints(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RiskParityWeights {
    pub w_eth: Decimal,
    pub w_btc: Decimal,
}

pub fn risk_parity_weights(
    vol_eth: Decimal,
    vol_btc: Decimal,
) -> Result<RiskParityWeights, RiskParityError> {
    if vol_eth <= Decimal::ZERO || vol_btc <= Decimal::ZERO {
        return Ok(RiskParityWeights {
            w_eth: Decimal::new(5, 1),
            w_btc: Decimal::new(5, 1),
        });
    }
    let inv_eth = Decimal::ONE / vol_eth;
    let inv_btc = Decimal::ONE / vol_btc;
    let total = inv_eth + inv_btc;
    if total == Decimal::ZERO {
        return Err(RiskParityError::InvalidVolatility);
    }
    let w_eth = inv_eth / total;
    let w_btc = Decimal::ONE - w_eth;
    Ok(RiskParityWeights { w_eth, w_btc })
}

pub fn compute_capital(config: &PositionConfig, equity: Decimal) -> Result<Decimal, CapitalError> {
    match config.c_mode {
        CapitalMode::FixedNotional => config.c_value.ok_or_else(|| {
            CapitalError::InvalidConfig("c_value required for fixed notional".to_string())
        }),
        CapitalMode::EquityRatio => {
            let k = config.equity_ratio_k.ok_or_else(|| {
                CapitalError::InvalidConfig("equity_ratio_k required for equity ratio".to_string())
            })?;
            Ok(equity * k)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinSizePolicy {
    Skip,
    Adjust,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderSize {
    pub qty: Decimal,
    pub notional: Decimal,
    pub price: Decimal,
}

#[derive(Debug, Clone)]
pub struct SizeConverter {
    constraints: InstrumentConstraints,
    policy: MinSizePolicy,
}

impl SizeConverter {
    pub fn new(constraints: InstrumentConstraints, policy: MinSizePolicy) -> Self {
        Self {
            constraints,
            policy,
        }
    }

    pub fn convert_notional(
        &self,
        notional: Decimal,
        price: Decimal,
    ) -> Result<OrderSize, PositionError> {
        if price <= Decimal::ZERO {
            return Err(PositionError::InvalidPrice("price must be > 0".to_string()));
        }
        let raw_qty = notional / price;
        let qty = self.round_qty(raw_qty, self.constraints.rounding_mode)?;
        let notional = qty * price;

        if qty < self.constraints.min_qty || notional < self.constraints.min_notional {
            return match self.policy {
                MinSizePolicy::Skip => Err(PositionError::BelowMinimum(
                    "order below minimum constraints".to_string(),
                )),
                MinSizePolicy::Adjust => {
                    let min_qty = self
                        .constraints
                        .min_notional
                        .checked_div(price)
                        .unwrap_or(self.constraints.min_qty);
                    let target = if min_qty > self.constraints.min_qty {
                        min_qty
                    } else {
                        self.constraints.min_qty
                    };
                    let adjusted = self.round_qty(target, RoundingMode::Ceil)?;
                    Ok(OrderSize {
                        qty: adjusted,
                        notional: adjusted * price,
                        price,
                    })
                }
            };
        }

        Ok(OrderSize {
            qty,
            notional,
            price,
        })
    }

    fn round_qty(&self, qty: Decimal, mode: RoundingMode) -> Result<Decimal, PositionError> {
        if self.constraints.step_size <= Decimal::ZERO {
            return Err(PositionError::InvalidConstraints(
                "step_size must be > 0".to_string(),
            ));
        }
        let steps = qty / self.constraints.step_size;
        let rounded_steps = match mode {
            RoundingMode::Floor => steps.floor(),
            RoundingMode::Ceil => steps.ceil(),
            RoundingMode::Round => steps.round(),
        };
        let rounded = rounded_steps * self.constraints.step_size;
        Ok(rounded.round_dp(self.constraints.qty_precision))
    }
}
