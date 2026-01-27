use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use thiserror::Error;

use crate::config::{FundingConfig, FundingMode, Symbol};
use crate::core::TradeDirection;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FundingError {
    #[error("missing data: {0}")]
    MissingData(String),
    #[error("invalid rate: {0}")]
    InvalidRate(String),
    #[error("rate limited")]
    RateLimited,
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FundingRate {
    pub symbol: Symbol,
    pub rate: Decimal,
    pub timestamp: DateTime<Utc>,
    pub interval_hours: u32,
}

impl FundingRate {
    pub fn validate(&self) -> Result<(), FundingError> {
        if self.interval_hours == 0 {
            return Err(FundingError::InvalidRate(
                "interval_hours must be > 0".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FundingSnapshot {
    pub eth: FundingRate,
    pub btc: FundingRate,
    pub interval_hours: u32,
}

#[async_trait::async_trait]
pub trait FundingSource: Send + Sync {
    async fn fetch_rate(
        &self,
        symbol: Symbol,
        timestamp: DateTime<Utc>,
    ) -> Result<FundingRate, FundingError>;
    async fn fetch_history(
        &self,
        symbol: Symbol,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<FundingRate>, FundingError>;
}

#[derive(Clone)]
pub struct FundingFetcher {
    source: Arc<dyn FundingSource>,
}

impl FundingFetcher {
    pub fn new(source: Arc<dyn FundingSource>) -> Self {
        Self { source }
    }

    pub async fn fetch_pair_rates(
        &self,
        timestamp: DateTime<Utc>,
    ) -> Result<FundingSnapshot, FundingError> {
        let eth = self.source.fetch_rate(Symbol::EthPerp, timestamp).await?;
        let btc = self.source.fetch_rate(Symbol::BtcPerp, timestamp).await?;
        eth.validate()?;
        btc.validate()?;
        if eth.interval_hours != btc.interval_hours {
            return Err(FundingError::InvalidRate(
                "funding intervals must match".to_string(),
            ));
        }
        Ok(FundingSnapshot {
            interval_hours: eth.interval_hours,
            eth,
            btc,
        })
    }
}

#[derive(Debug, Default, Clone)]
pub struct MockFundingSource {
    rates: HashMap<(Symbol, DateTime<Utc>), FundingRate>,
    history: HashMap<Symbol, Vec<FundingRate>>,
    errors: HashMap<(Symbol, DateTime<Utc>), FundingError>,
}

impl MockFundingSource {
    pub fn insert_rate(&mut self, rate: FundingRate) {
        self.rates.insert((rate.symbol, rate.timestamp), rate);
    }

    pub fn insert_history(&mut self, symbol: Symbol, history: Vec<FundingRate>) {
        self.history.insert(symbol, history);
    }

    pub fn insert_error(&mut self, symbol: Symbol, timestamp: DateTime<Utc>, error: FundingError) {
        self.errors.insert((symbol, timestamp), error);
    }

    fn read_rate(
        &self,
        symbol: Symbol,
        timestamp: DateTime<Utc>,
    ) -> Result<FundingRate, FundingError> {
        if let Some(error) = self.errors.get(&(symbol, timestamp)) {
            return Err(error.clone());
        }
        self.rates
            .get(&(symbol, timestamp))
            .cloned()
            .ok_or_else(|| FundingError::MissingData("funding rate not found".to_string()))
    }
}

#[async_trait::async_trait]
impl FundingSource for MockFundingSource {
    async fn fetch_rate(
        &self,
        symbol: Symbol,
        timestamp: DateTime<Utc>,
    ) -> Result<FundingRate, FundingError> {
        self.read_rate(symbol, timestamp)
    }

    async fn fetch_history(
        &self,
        symbol: Symbol,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<FundingRate>, FundingError> {
        let entries = self
            .history
            .get(&symbol)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|rate| rate.timestamp >= start && rate.timestamp <= end)
            .collect::<Vec<_>>();
        Ok(entries)
    }
}

#[derive(Debug, Clone)]
pub struct FundingHistory {
    capacity: usize,
    entries: HashMap<Symbol, VecDeque<FundingRate>>,
}

impl FundingHistory {
    pub fn new(capacity: usize) -> Result<Self, FundingError> {
        if capacity == 0 {
            return Err(FundingError::InvalidRate(
                "capacity must be > 0".to_string(),
            ));
        }
        Ok(Self {
            capacity,
            entries: HashMap::new(),
        })
    }

    pub fn push(&mut self, rate: FundingRate) {
        let queue = self.entries.entry(rate.symbol).or_default();
        if queue.len() == self.capacity {
            queue.pop_front();
        }
        queue.push_back(rate);
    }

    pub fn window(&self, symbol: Symbol) -> Vec<FundingRate> {
        self.entries
            .get(&symbol)
            .map(|queue| queue.iter().cloned().collect())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FundingCostEstimate {
    pub cost_est: Decimal,
    pub normalized: Decimal,
    pub interval_hours: u32,
}

pub fn estimate_funding_cost(
    direction: TradeDirection,
    notional_eth: Decimal,
    notional_btc: Decimal,
    eth_rate: &FundingRate,
    btc_rate: &FundingRate,
    max_hold_hours: u32,
) -> Result<FundingCostEstimate, FundingError> {
    if eth_rate.interval_hours == 0 {
        return Err(FundingError::InvalidRate(
            "interval_hours must be > 0".to_string(),
        ));
    }
    let intervals = max_hold_hours.div_ceil(eth_rate.interval_hours);
    let per_interval = match direction {
        TradeDirection::LongEthShortBtc => {
            eth_rate.rate * notional_eth - btc_rate.rate * notional_btc
        }
        TradeDirection::ShortEthLongBtc => {
            -eth_rate.rate * notional_eth + btc_rate.rate * notional_btc
        }
    };
    let total_cost = per_interval * Decimal::from(intervals as u64);
    let cost_est = if total_cost > Decimal::ZERO {
        total_cost
    } else {
        Decimal::ZERO
    };
    let total_notional = notional_eth + notional_btc;
    let normalized = if total_notional > Decimal::ZERO {
        cost_est / total_notional
    } else {
        Decimal::ZERO
    };
    Ok(FundingCostEstimate {
        cost_est,
        normalized,
        interval_hours: eth_rate.interval_hours,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct FundingDecision {
    pub should_skip: bool,
    pub adjusted_entry_z: Decimal,
    pub adjusted_capital: Decimal,
}

pub fn apply_funding_controls(
    config: &FundingConfig,
    entry_z: Decimal,
    capital: Decimal,
    estimate: &FundingCostEstimate,
) -> Result<FundingDecision, FundingError> {
    let mut should_skip = false;
    let mut adjusted_entry_z = entry_z;
    let mut adjusted_capital = capital;

    for mode in &config.modes {
        match mode {
            FundingMode::Filter => {
                let threshold = config.funding_cost_threshold.ok_or_else(|| {
                    FundingError::InvalidConfig("funding_cost_threshold missing".to_string())
                })?;
                if estimate.cost_est > threshold {
                    should_skip = true;
                }
            }
            FundingMode::Threshold => {
                let k = config.funding_threshold_k.ok_or_else(|| {
                    FundingError::InvalidConfig("funding_threshold_k missing".to_string())
                })?;
                adjusted_entry_z += k * estimate.normalized;
            }
            FundingMode::Size => {
                let alpha = config.funding_size_alpha.ok_or_else(|| {
                    FundingError::InvalidConfig("funding_size_alpha missing".to_string())
                })?;
                let c_min_ratio = config.c_min_ratio.ok_or_else(|| {
                    FundingError::InvalidConfig("c_min_ratio missing".to_string())
                })?;
                let mut ratio = Decimal::ONE - alpha * estimate.normalized;
                if ratio < c_min_ratio {
                    ratio = c_min_ratio;
                }
                if ratio > Decimal::ONE {
                    ratio = Decimal::ONE;
                }
                adjusted_capital = capital * ratio;
            }
        }
    }

    Ok(FundingDecision {
        should_skip,
        adjusted_entry_z,
        adjusted_capital,
    })
}
