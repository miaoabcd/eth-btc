use std::collections::{HashMap, VecDeque};
use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde_json::Value;
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

#[derive(Debug, Clone)]
pub struct FundingHttpResponse {
    pub status: u16,
    pub body: String,
}

#[async_trait::async_trait]
pub trait FundingHttpClient: Send + Sync {
    async fn post(&self, url: &str, body: Value) -> Result<FundingHttpResponse, FundingError>;
}

#[derive(Clone)]
pub struct ReqwestFundingClient {
    client: reqwest::Client,
}

impl ReqwestFundingClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl FundingHttpClient for ReqwestFundingClient {
    async fn post(&self, url: &str, body: Value) -> Result<FundingHttpResponse, FundingError> {
        let response = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(|err| FundingError::InvalidRate(err.to_string()))?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|err| FundingError::InvalidRate(err.to_string()))?;
        Ok(FundingHttpResponse { status, body })
    }
}

#[derive(Clone)]
pub struct HyperliquidFundingSource {
    base_url: String,
    http: Arc<dyn FundingHttpClient>,
    interval_hours: u32,
}

impl HyperliquidFundingSource {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: Arc::new(ReqwestFundingClient::new()),
            interval_hours: 1,
        }
    }

    pub fn with_client(base_url: impl Into<String>, http: Arc<dyn FundingHttpClient>) -> Self {
        Self {
            base_url: base_url.into(),
            http,
            interval_hours: 1,
        }
    }

    fn endpoint_url(&self) -> String {
        format!("{}/info", self.base_url.trim_end_matches('/'))
    }

    fn symbol_string(symbol: Symbol) -> &'static str {
        match symbol {
            Symbol::EthPerp => "ETH",
            Symbol::BtcPerp => "BTC",
        }
    }

    fn parse_decimal(value: &Value) -> Result<Decimal, FundingError> {
        let value = match value {
            Value::String(value) => value.clone(),
            Value::Number(value) => value.to_string(),
            other => {
                return Err(FundingError::InvalidRate(format!(
                    "unsupported rate value: {other}"
                )));
            }
        };
        Decimal::from_str(&value).map_err(|err| {
            FundingError::InvalidRate(format!("invalid funding rate {value}: {err}"))
        })
    }

    fn parse_snapshot(
        &self,
        body: &str,
        timestamp: DateTime<Utc>,
    ) -> Result<HashMap<Symbol, FundingRate>, FundingError> {
        let value: Value =
            serde_json::from_str(body).map_err(|err| FundingError::InvalidRate(err.to_string()))?;
        let payload = if let Some(array) = value.as_array() {
            array.clone()
        } else if let Some(array) = value.get("data").and_then(|data| data.as_array()).cloned() {
            array
        } else {
            return Err(FundingError::InvalidRate(
                "unexpected funding response".to_string(),
            ));
        };
        if payload.len() < 2 {
            return Err(FundingError::InvalidRate(
                "funding payload missing contexts".to_string(),
            ));
        }
        let universe = payload[0]
            .get("universe")
            .and_then(|value| value.as_array())
            .ok_or_else(|| FundingError::InvalidRate("universe missing".to_string()))?;
        let ctxs = payload[1]
            .as_array()
            .ok_or_else(|| FundingError::InvalidRate("asset contexts missing".to_string()))?;

        let mut rates = HashMap::new();
        for (index, asset) in universe.iter().enumerate() {
            let name = asset
                .get("name")
                .and_then(|value| value.as_str())
                .ok_or_else(|| FundingError::InvalidRate("asset name missing".to_string()))?;
            let symbol = match name {
                "ETH" => Symbol::EthPerp,
                "BTC" => Symbol::BtcPerp,
                _ => continue,
            };
            let ctx = ctxs.get(index).ok_or_else(|| {
                FundingError::InvalidRate("asset context missing".to_string())
            })?;
            let funding = ctx
                .get("funding")
                .ok_or_else(|| FundingError::MissingData("funding rate missing".to_string()))?;
            let rate = Self::parse_decimal(funding)?;
            rates.insert(
                symbol,
                FundingRate {
                    symbol,
                    rate,
                    timestamp,
                    interval_hours: self.interval_hours,
                },
            );
        }
        Ok(rates)
    }
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

#[async_trait::async_trait]
impl FundingSource for HyperliquidFundingSource {
    async fn fetch_rate(
        &self,
        symbol: Symbol,
        timestamp: DateTime<Utc>,
    ) -> Result<FundingRate, FundingError> {
        let rates = self.fetch_snapshot(timestamp).await?;
        rates.get(&symbol).cloned().ok_or_else(|| {
            FundingError::MissingData(format!(
                "funding rate not found for {} at {}",
                Self::symbol_string(symbol),
                timestamp.to_rfc3339()
            ))
        })
    }

    async fn fetch_history(
        &self,
        symbol: Symbol,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<FundingRate>, FundingError> {
        let rates = self.fetch_snapshot(end).await?;
        let rate = rates.get(&symbol).cloned().ok_or_else(|| {
            FundingError::MissingData(format!(
                "funding rate not found for {} at {}",
                Self::symbol_string(symbol),
                end.to_rfc3339()
            ))
        })?;
        let _ = start;
        Ok(vec![rate])
    }
}

impl HyperliquidFundingSource {
    async fn fetch_snapshot(
        &self,
        timestamp: DateTime<Utc>,
    ) -> Result<HashMap<Symbol, FundingRate>, FundingError> {
        let url = self.endpoint_url();
        let body = serde_json::json!({
            "type": "metaAndAssetCtxs"
        });
        let response = self.http.post(&url, body).await?;
        match response.status {
            200 => self.parse_snapshot(&response.body, timestamp),
            429 => Err(FundingError::RateLimited),
            status => Err(FundingError::InvalidRate(format!(
                "unexpected status {status}"
            ))),
        }
    }
}

#[deprecated(note = "use HyperliquidFundingSource")]
pub type VariationalFundingSource = HyperliquidFundingSource;

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
pub struct ZeroFundingSource {
    interval_hours: u32,
}

impl Default for ZeroFundingSource {
    fn default() -> Self {
        Self { interval_hours: 8 }
    }
}

impl ZeroFundingSource {
    pub fn new(interval_hours: u32) -> Self {
        Self { interval_hours }
    }
}

#[async_trait::async_trait]
impl FundingSource for ZeroFundingSource {
    async fn fetch_rate(
        &self,
        symbol: Symbol,
        timestamp: DateTime<Utc>,
    ) -> Result<FundingRate, FundingError> {
        let interval = if self.interval_hours == 0 {
            8
        } else {
            self.interval_hours
        };
        Ok(FundingRate {
            symbol,
            rate: Decimal::ZERO,
            timestamp,
            interval_hours: interval,
        })
    }

    async fn fetch_history(
        &self,
        _symbol: Symbol,
        _start: DateTime<Utc>,
        _end: DateTime<Utc>,
    ) -> Result<Vec<FundingRate>, FundingError> {
        Ok(Vec::new())
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
