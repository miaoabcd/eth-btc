use std::collections::VecDeque;

use rust_decimal::Decimal;
use rust_decimal::MathematicalOps;
use rust_decimal::prelude::ToPrimitive;
use thiserror::Error;

use crate::config::{SigmaFloorConfig, SigmaFloorMode};

#[derive(Debug, Error)]
pub enum IndicatorError {
    #[error("invalid price: {0}")]
    InvalidPrice(String),
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("invalid window: {0}")]
    InvalidWindow(String),
    #[error("math error: {0}")]
    Math(String),
}

#[derive(Debug, Clone)]
struct RollingWindow {
    capacity: usize,
    values: VecDeque<Decimal>,
}

impl RollingWindow {
    fn new(capacity: usize) -> Result<Self, IndicatorError> {
        if capacity == 0 {
            return Err(IndicatorError::InvalidWindow(
                "capacity must be > 0".to_string(),
            ));
        }
        Ok(Self {
            capacity,
            values: VecDeque::with_capacity(capacity),
        })
    }

    fn push(&mut self, value: Decimal) {
        if self.values.len() == self.capacity {
            self.values.pop_front();
        }
        self.values.push_back(value);
    }

    fn len(&self) -> usize {
        self.values.len()
    }

    fn as_slice(&mut self) -> &[Decimal] {
        self.values.make_contiguous()
    }

    fn to_vec(&self) -> Vec<Decimal> {
        self.values.iter().cloned().collect()
    }

    fn mean(&self) -> Option<Decimal> {
        if self.values.is_empty() {
            return None;
        }
        let sum = self
            .values
            .iter()
            .fold(Decimal::ZERO, |acc, value| acc + *value);
        Some(sum / Decimal::from(self.values.len() as u64))
    }

    fn std(&self) -> Option<Decimal> {
        let mean = self.mean()?;
        let count = Decimal::from(self.values.len() as u64);
        let mut sum = Decimal::ZERO;
        for value in &self.values {
            let diff = *value - mean;
            sum += diff * diff;
        }
        let variance = sum / count;
        variance.sqrt().or(Some(Decimal::ZERO))
    }

    fn quantile(&self, p: Decimal) -> Option<Decimal> {
        if self.values.is_empty() {
            return None;
        }
        let mut sorted = self.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let max_index = Decimal::from((sorted.len() - 1) as u64);
        let index_decimal = (max_index * p).floor();
        let index = index_decimal
            .to_u64()
            .unwrap_or(0)
            .min((sorted.len() - 1) as u64) as usize;
        sorted.get(index).cloned()
    }
}

pub fn relative_price(eth: Decimal, btc: Decimal) -> Result<Decimal, IndicatorError> {
    if eth <= Decimal::ZERO || btc <= Decimal::ZERO {
        return Err(IndicatorError::InvalidPrice(
            "prices must be > 0".to_string(),
        ));
    }
    let eth_ln = eth
        .checked_ln()
        .ok_or_else(|| IndicatorError::Math("ln unavailable for ETH price".to_string()))?;
    let btc_ln = btc
        .checked_ln()
        .ok_or_else(|| IndicatorError::Math("ln unavailable for BTC price".to_string()))?;
    Ok(eth_ln - btc_ln)
}

pub fn log_return(current: Decimal, previous: Decimal) -> Result<Decimal, IndicatorError> {
    if current <= Decimal::ZERO || previous <= Decimal::ZERO {
        return Err(IndicatorError::InvalidPrice(
            "prices must be > 0".to_string(),
        ));
    }
    let ratio = current / previous;
    ratio
        .checked_ln()
        .ok_or_else(|| IndicatorError::Math("ln unavailable for return ratio".to_string()))
}

pub fn ewma_std(values: &[Decimal], half_life: u32) -> Option<Decimal> {
    if values.len() < 2 || half_life == 0 {
        return None;
    }
    let decay = Decimal::new(5, 1).powd(Decimal::ONE / Decimal::from(half_life));
    let alpha = Decimal::ONE - decay;
    let mut mean = values[0];
    let mut variance = Decimal::ZERO;
    for value in values.iter().skip(1) {
        let delta = *value - mean;
        mean += alpha * delta;
        let diff = *value - mean;
        variance = alpha * diff * diff + (Decimal::ONE - alpha) * variance;
    }
    variance.sqrt()
}

#[derive(Debug, Clone)]
pub struct SigmaFloorCalculator {
    mode: SigmaFloorMode,
    sigma_floor_const: Decimal,
    quantile_window: usize,
    quantile_p: Decimal,
    ewma_half_life: u32,
    sigma_history: RollingWindow,
}

impl SigmaFloorCalculator {
    pub fn new(config: SigmaFloorConfig, bars_per_day: usize) -> Result<Self, IndicatorError> {
        if config.sigma_floor_const <= Decimal::ZERO {
            return Err(IndicatorError::InvalidConfig(
                "sigma_floor_const must be > 0".to_string(),
            ));
        }
        if config.sigma_floor_quantile_p <= Decimal::ZERO
            || config.sigma_floor_quantile_p > Decimal::ONE
        {
            return Err(IndicatorError::InvalidConfig(
                "sigma_floor_quantile_p must be in (0,1]".to_string(),
            ));
        }
        if config.ewma_half_life == 0 {
            return Err(IndicatorError::InvalidConfig(
                "ewma_half_life must be > 0".to_string(),
            ));
        }
        let quantile_window = config
            .sigma_floor_quantile_window
            .saturating_mul(bars_per_day as u32) as usize;
        let sigma_history = RollingWindow::new(quantile_window.max(1))?;
        Ok(Self {
            mode: config.mode,
            sigma_floor_const: config.sigma_floor_const,
            quantile_window,
            quantile_p: config.sigma_floor_quantile_p,
            ewma_half_life: config.ewma_half_life,
            sigma_history,
        })
    }

    pub fn update(&mut self, sigma_t: Decimal, r_values: &[Decimal]) -> Option<Decimal> {
        self.sigma_history.push(sigma_t);
        match self.mode {
            SigmaFloorMode::Const => Some(self.sigma_floor_const),
            SigmaFloorMode::Quantile => {
                if self.sigma_history.len() < self.quantile_window {
                    return None;
                }
                self.sigma_history.quantile(self.quantile_p)
            }
            SigmaFloorMode::EwmaMix => {
                if self.sigma_history.len() < self.quantile_window {
                    return None;
                }
                let quantile = self.sigma_history.quantile(self.quantile_p)?;
                let ewma = ewma_std(r_values, self.ewma_half_life)?;
                Some(if ewma > quantile { ewma } else { quantile })
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ZScoreSnapshot {
    pub r: Decimal,
    pub mean: Option<Decimal>,
    pub sigma: Option<Decimal>,
    pub sigma_floor: Option<Decimal>,
    pub sigma_eff: Option<Decimal>,
    pub zscore: Option<Decimal>,
}

#[derive(Debug, Clone)]
pub struct ZScoreCalculator {
    n_z: usize,
    window: RollingWindow,
    sigma_floor: SigmaFloorCalculator,
}

impl ZScoreCalculator {
    pub fn new(
        n_z: usize,
        sigma_floor_config: SigmaFloorConfig,
        bars_per_day: usize,
    ) -> Result<Self, IndicatorError> {
        if n_z == 0 {
            return Err(IndicatorError::InvalidConfig("n_z must be > 0".to_string()));
        }
        Ok(Self {
            n_z,
            window: RollingWindow::new(n_z)?,
            sigma_floor: SigmaFloorCalculator::new(sigma_floor_config, bars_per_day)?,
        })
    }

    pub fn update(&mut self, r: Decimal) -> Result<ZScoreSnapshot, IndicatorError> {
        self.window.push(r);
        if self.window.len() < self.n_z {
            return Ok(ZScoreSnapshot {
                r,
                mean: None,
                sigma: None,
                sigma_floor: None,
                sigma_eff: None,
                zscore: None,
            });
        }
        let mean = self
            .window
            .mean()
            .ok_or_else(|| IndicatorError::Math("mean unavailable".to_string()))?;
        let sigma = self
            .window
            .std()
            .ok_or_else(|| IndicatorError::Math("sigma unavailable".to_string()))?;
        let floor = self.sigma_floor.update(sigma, self.window.as_slice());
        let sigma_eff = floor.map(|floor| if sigma > floor { sigma } else { floor });
        let zscore = sigma_eff.map(|sigma_eff| {
            if sigma_eff == Decimal::ZERO {
                Decimal::ZERO
            } else {
                (r - mean) / sigma_eff
            }
        });
        Ok(ZScoreSnapshot {
            r,
            mean: Some(mean),
            sigma: Some(sigma),
            sigma_floor: floor,
            sigma_eff,
            zscore,
        })
    }
}

#[derive(Debug, Clone)]
pub struct VolatilitySnapshot {
    pub vol_eth: Option<Decimal>,
    pub vol_btc: Option<Decimal>,
}

#[derive(Debug, Clone)]
pub struct VolatilityCalculator {
    n_vol: usize,
    eth_returns: RollingWindow,
    btc_returns: RollingWindow,
    last_eth: Option<Decimal>,
    last_btc: Option<Decimal>,
}

impl VolatilityCalculator {
    pub fn new(n_vol: usize) -> Result<Self, IndicatorError> {
        if n_vol == 0 {
            return Err(IndicatorError::InvalidConfig(
                "n_vol must be > 0".to_string(),
            ));
        }
        Ok(Self {
            n_vol,
            eth_returns: RollingWindow::new(n_vol)?,
            btc_returns: RollingWindow::new(n_vol)?,
            last_eth: None,
            last_btc: None,
        })
    }

    pub fn update(
        &mut self,
        eth_price: Decimal,
        btc_price: Decimal,
    ) -> Result<VolatilitySnapshot, IndicatorError> {
        if eth_price <= Decimal::ZERO || btc_price <= Decimal::ZERO {
            return Err(IndicatorError::InvalidPrice(
                "prices must be > 0".to_string(),
            ));
        }
        if let Some(last) = self.last_eth {
            let ret = log_return(eth_price, last)?;
            self.eth_returns.push(ret);
        }
        if let Some(last) = self.last_btc {
            let ret = log_return(btc_price, last)?;
            self.btc_returns.push(ret);
        }
        self.last_eth = Some(eth_price);
        self.last_btc = Some(btc_price);

        let vol_eth = if self.eth_returns.len() >= self.n_vol {
            self.eth_returns.std()
        } else {
            None
        };
        let vol_btc = if self.btc_returns.len() >= self.n_vol {
            self.btc_returns.std()
        } else {
            None
        };
        Ok(VolatilitySnapshot { vol_eth, vol_btc })
    }
}
