use std::collections::{HashMap, VecDeque};
use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use crate::config::{PriceField, Symbol};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DataError {
    #[error("missing data: {0}")]
    MissingData(String),
    #[error("invalid price: {0}")]
    InvalidPrice(String),
    #[error("rate limited")]
    RateLimited,
    #[error("timeout")]
    Timeout,
    #[error("inconsistent data: {0}")]
    InconsistentData(String),
    #[error("invalid timestamp: {0}")]
    InvalidTimestamp(String),
    #[error("http error: {0}")]
    Http(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("invalid window: {0}")]
    InvalidWindow(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PriceBar {
    pub symbol: Symbol,
    pub timestamp: DateTime<Utc>,
    pub mid: Option<Decimal>,
    pub mark: Option<Decimal>,
    pub close: Option<Decimal>,
}

impl PriceBar {
    pub fn new(
        symbol: Symbol,
        timestamp: DateTime<Utc>,
        mid: Option<Decimal>,
        mark: Option<Decimal>,
        close: Option<Decimal>,
    ) -> Self {
        Self {
            symbol,
            timestamp,
            mid,
            mark,
            close,
        }
    }

    pub fn effective_price(&self, preferred: PriceField) -> Option<Decimal> {
        match preferred {
            PriceField::Mid => self.mid.or(self.mark).or(self.close),
            PriceField::Mark => self.mark.or(self.mid).or(self.close),
            PriceField::Close => self.close.or(self.mid).or(self.mark),
        }
    }

    pub fn validate(&self) -> Result<(), DataError> {
        for (label, value) in [
            ("mid", self.mid),
            ("mark", self.mark),
            ("close", self.close),
        ] {
            if let Some(price) = value
                && price <= Decimal::ZERO
            {
                return Err(DataError::InvalidPrice(format!("{} must be > 0", label)));
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
pub trait PriceSource: Send + Sync {
    async fn fetch_bar(
        &self,
        symbol: Symbol,
        timestamp: DateTime<Utc>,
    ) -> Result<PriceBar, DataError>;
    async fn fetch_history(
        &self,
        symbol: Symbol,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<PriceBar>, DataError>;
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

#[async_trait::async_trait]
pub trait HttpClient: Send + Sync {
    async fn get(&self, url: &str, query: &[(&str, String)]) -> Result<HttpResponse, DataError>;
}

#[derive(Debug, Clone)]
pub struct ReqwestHttpClient {
    client: reqwest::Client,
}

impl ReqwestHttpClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for ReqwestHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl HttpClient for ReqwestHttpClient {
    async fn get(&self, url: &str, query: &[(&str, String)]) -> Result<HttpResponse, DataError> {
        let response = self
            .client
            .get(url)
            .query(&query)
            .send()
            .await
            .map_err(|err| {
                if err.is_timeout() {
                    DataError::Timeout
                } else {
                    DataError::Http(err.to_string())
                }
            })?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|err| DataError::Http(err.to_string()))?;
        Ok(HttpResponse { status, body })
    }
}

#[derive(Clone)]
pub struct VariationalPriceSource {
    base_url: String,
    http: Arc<dyn HttpClient>,
}

impl VariationalPriceSource {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: Arc::new(ReqwestHttpClient::new()),
        }
    }

    pub fn with_client(base_url: impl Into<String>, http: Arc<dyn HttpClient>) -> Self {
        Self {
            base_url: base_url.into(),
            http,
        }
    }

    fn endpoint_url(&self) -> String {
        format!("{}/v1/marketdata/bars", self.base_url.trim_end_matches('/'))
    }

    fn symbol_string(symbol: Symbol) -> &'static str {
        match symbol {
            Symbol::EthPerp => "ETH-PERP",
            Symbol::BtcPerp => "BTC-PERP",
        }
    }

    fn normalize_range(
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<(DateTime<Utc>, DateTime<Utc>), DataError> {
        let start = align_to_bar_close(start);
        let end = align_to_bar_close(end);
        if end < start {
            return Err(DataError::InvalidTimestamp(
                "end must be >= start".to_string(),
            ));
        }
        Ok((start, end))
    }

    fn parse_decimal(value: Option<Value>) -> Result<Option<Decimal>, DataError> {
        let Some(value) = value else {
            return Ok(None);
        };
        let value = match value {
            Value::String(value) => value,
            Value::Number(value) => value.to_string(),
            other => {
                return Err(DataError::Parse(format!(
                    "unsupported decimal value: {other}"
                )));
            }
        };
        Decimal::from_str(&value)
            .map(Some)
            .map_err(|err| DataError::Parse(format!("invalid decimal {value}: {err}")))
    }

    fn parse_bars(&self, symbol: Symbol, body: &str) -> Result<Vec<PriceBar>, DataError> {
        let response: BarsResponse =
            serde_json::from_str(body).map_err(|err| DataError::Parse(err.to_string()))?;
        let mut bars = Vec::new();
        for record in response.bars {
            let timestamp = DateTime::parse_from_rfc3339(&record.timestamp)
                .map_err(|err| DataError::Parse(err.to_string()))?
                .with_timezone(&Utc);
            let bar = PriceBar::new(
                symbol,
                timestamp,
                Self::parse_decimal(record.mid)?,
                Self::parse_decimal(record.mark)?,
                Self::parse_decimal(record.close)?,
            );
            bar.validate()?;
            bars.push(bar);
        }
        Ok(bars)
    }
}

#[async_trait::async_trait]
impl PriceSource for VariationalPriceSource {
    async fn fetch_bar(
        &self,
        symbol: Symbol,
        timestamp: DateTime<Utc>,
    ) -> Result<PriceBar, DataError> {
        let aligned = align_to_bar_close(timestamp);
        let mut bars = self.fetch_history(symbol, aligned, aligned).await?;
        let bar = bars
            .iter()
            .position(|bar| bar.timestamp == aligned)
            .map(|index| bars.swap_remove(index))
            .ok_or_else(|| {
                DataError::MissingData(format!(
                    "bar not found for {} at {}",
                    Self::symbol_string(symbol),
                    aligned.to_rfc3339()
                ))
            })?;
        Ok(bar)
    }

    async fn fetch_history(
        &self,
        symbol: Symbol,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<PriceBar>, DataError> {
        let (start, end) = Self::normalize_range(start, end)?;
        let url = self.endpoint_url();
        let query = vec![
            ("symbol", Self::symbol_string(symbol).to_string()),
            ("start", start.to_rfc3339()),
            ("end", end.to_rfc3339()),
            ("interval", "15m".to_string()),
        ];
        let response = self.http.get(&url, &query).await?;
        match response.status {
            200 => {
                let bars = self.parse_bars(symbol, &response.body)?;
                Ok(bars
                    .into_iter()
                    .filter(|bar| bar.timestamp >= start && bar.timestamp <= end)
                    .collect())
            }
            429 => Err(DataError::RateLimited),
            status => Err(DataError::Http(format!("unexpected status {status}"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PriceSnapshot {
    pub timestamp: DateTime<Utc>,
    pub eth: Decimal,
    pub btc: Decimal,
    pub field: PriceField,
}

#[derive(Clone)]
pub struct PriceFetcher {
    source: Arc<dyn PriceSource>,
    price_field: PriceField,
}

impl PriceFetcher {
    pub fn new(source: Arc<dyn PriceSource>, price_field: PriceField) -> Self {
        Self {
            source,
            price_field,
        }
    }

    pub async fn fetch_pair_prices(
        &self,
        timestamp: DateTime<Utc>,
    ) -> Result<PriceSnapshot, DataError> {
        let aligned = align_to_bar_close(timestamp);
        let eth_bar = self.source.fetch_bar(Symbol::EthPerp, aligned).await?;
        let btc_bar = self.source.fetch_bar(Symbol::BtcPerp, aligned).await?;
        eth_bar.validate()?;
        btc_bar.validate()?;

        if eth_bar.symbol != Symbol::EthPerp || btc_bar.symbol != Symbol::BtcPerp {
            return Err(DataError::InconsistentData(
                "unexpected symbols in price bars".to_string(),
            ));
        }
        if eth_bar.timestamp != btc_bar.timestamp {
            return Err(DataError::InconsistentData(
                "timestamp mismatch between ETH and BTC".to_string(),
            ));
        }
        if eth_bar.timestamp != aligned {
            return Err(DataError::InconsistentData(
                "bar timestamp does not match requested close".to_string(),
            ));
        }

        let eth_price = eth_bar
            .effective_price(self.price_field)
            .ok_or_else(|| DataError::MissingData("ETH price missing".to_string()))?;
        let btc_price = btc_bar
            .effective_price(self.price_field)
            .ok_or_else(|| DataError::MissingData("BTC price missing".to_string()))?;

        Ok(PriceSnapshot {
            timestamp: aligned,
            eth: eth_price,
            btc: btc_price,
            field: self.price_field,
        })
    }
}

#[derive(Debug, Default, Clone)]
pub struct MockPriceSource {
    bars: HashMap<(Symbol, DateTime<Utc>), PriceBar>,
    history: HashMap<Symbol, Vec<PriceBar>>,
    errors: HashMap<(Symbol, DateTime<Utc>), DataError>,
    history_errors: HashMap<Symbol, DataError>,
}

impl MockPriceSource {
    pub fn insert_bar(&mut self, bar: PriceBar) {
        self.bars.insert((bar.symbol, bar.timestamp), bar);
    }

    pub fn insert_history(&mut self, symbol: Symbol, bars: Vec<PriceBar>) {
        self.history.insert(symbol, bars);
    }

    pub fn insert_error(&mut self, symbol: Symbol, timestamp: DateTime<Utc>, error: DataError) {
        self.errors.insert((symbol, timestamp), error);
    }

    pub fn insert_history_error(&mut self, symbol: Symbol, error: DataError) {
        self.history_errors.insert(symbol, error);
    }

    fn read_bar(&self, symbol: Symbol, timestamp: DateTime<Utc>) -> Result<PriceBar, DataError> {
        if let Some(error) = self.errors.get(&(symbol, timestamp)) {
            return Err(error.clone());
        }
        let bar = self
            .bars
            .get(&(symbol, timestamp))
            .cloned()
            .ok_or_else(|| DataError::MissingData("bar not found".to_string()))?;
        bar.validate()?;
        Ok(bar)
    }

    fn read_history(
        &self,
        symbol: Symbol,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<PriceBar>, DataError> {
        if let Some(error) = self.history_errors.get(&symbol) {
            return Err(error.clone());
        }
        let bars = self
            .history
            .get(&symbol)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|bar| bar.timestamp >= start && bar.timestamp <= end)
            .collect::<Vec<_>>();
        Ok(bars)
    }
}

#[async_trait::async_trait]
impl PriceSource for MockPriceSource {
    async fn fetch_bar(
        &self,
        symbol: Symbol,
        timestamp: DateTime<Utc>,
    ) -> Result<PriceBar, DataError> {
        self.read_bar(symbol, timestamp)
    }
    async fn fetch_history(
        &self,
        symbol: Symbol,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<PriceBar>, DataError> {
        self.read_history(symbol, start, end)
    }
}

#[derive(Debug, Clone)]
pub struct PriceHistory {
    capacity: usize,
    bars: VecDeque<PriceBar>,
}

impl PriceHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            bars: VecDeque::with_capacity(capacity),
        }
    }

    pub fn push(&mut self, bar: PriceBar) {
        if self.bars.len() == self.capacity {
            self.bars.pop_front();
        }
        self.bars.push_back(bar);
    }

    pub fn get(&self, offset: usize) -> Option<&PriceBar> {
        let len = self.bars.len();
        if offset >= len {
            return None;
        }
        self.bars.get(len - 1 - offset)
    }

    pub fn len(&self) -> usize {
        self.bars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bars.is_empty()
    }

    pub fn is_warmed_up(&self, required: usize) -> bool {
        self.len() >= required
    }

    pub fn to_vec(&self) -> Vec<PriceBar> {
        self.bars.iter().cloned().collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceWindow {
    ZScore,
    Volatility,
    SigmaQuantile,
}

#[derive(Debug, Clone)]
struct SymbolHistory {
    zscore: PriceHistory,
    volatility: PriceHistory,
    sigma: PriceHistory,
}

impl SymbolHistory {
    fn new(z_capacity: usize, vol_capacity: usize, sigma_capacity: usize) -> Self {
        Self {
            zscore: PriceHistory::new(z_capacity),
            volatility: PriceHistory::new(vol_capacity),
            sigma: PriceHistory::new(sigma_capacity),
        }
    }

    fn push(&mut self, bar: PriceBar) {
        self.zscore.push(bar.clone());
        self.volatility.push(bar.clone());
        self.sigma.push(bar);
    }
}

#[derive(Debug, Clone)]
pub struct PriceHistorySet {
    z_capacity: usize,
    vol_capacity: usize,
    sigma_capacity: usize,
    eth: SymbolHistory,
    btc: SymbolHistory,
    last_timestamp: Option<DateTime<Utc>>,
}

impl PriceHistorySet {
    pub fn new(
        z_capacity: usize,
        vol_capacity: usize,
        sigma_capacity: usize,
    ) -> Result<Self, DataError> {
        if z_capacity == 0 || vol_capacity == 0 || sigma_capacity == 0 {
            return Err(DataError::InvalidWindow(
                "window sizes must be > 0".to_string(),
            ));
        }
        Ok(Self {
            z_capacity,
            vol_capacity,
            sigma_capacity,
            eth: SymbolHistory::new(z_capacity, vol_capacity, sigma_capacity),
            btc: SymbolHistory::new(z_capacity, vol_capacity, sigma_capacity),
            last_timestamp: None,
        })
    }

    pub fn push_pair(&mut self, eth_bar: PriceBar, btc_bar: PriceBar) -> Result<(), DataError> {
        if eth_bar.symbol != Symbol::EthPerp || btc_bar.symbol != Symbol::BtcPerp {
            return Err(DataError::InconsistentData(
                "expected ETH and BTC bars".to_string(),
            ));
        }
        if eth_bar.timestamp != btc_bar.timestamp {
            return Err(DataError::InconsistentData(
                "timestamp mismatch between ETH and BTC".to_string(),
            ));
        }
        if let Some(last) = self.last_timestamp
            && eth_bar.timestamp <= last
        {
            return Err(DataError::InvalidTimestamp(
                "timestamp must be strictly increasing".to_string(),
            ));
        }
        eth_bar.validate()?;
        btc_bar.validate()?;

        self.eth.push(eth_bar.clone());
        self.btc.push(btc_bar.clone());
        self.last_timestamp = Some(eth_bar.timestamp);
        Ok(())
    }

    pub fn is_warmed_up(&self, window: PriceWindow) -> bool {
        match window {
            PriceWindow::ZScore => {
                self.eth.zscore.len() >= self.z_capacity && self.btc.zscore.len() >= self.z_capacity
            }
            PriceWindow::Volatility => {
                self.eth.volatility.len() >= self.vol_capacity
                    && self.btc.volatility.len() >= self.vol_capacity
            }
            PriceWindow::SigmaQuantile => {
                self.eth.sigma.len() >= self.sigma_capacity
                    && self.btc.sigma.len() >= self.sigma_capacity
            }
        }
    }

    pub fn window(&self, symbol: Symbol, window: PriceWindow) -> Vec<PriceBar> {
        let history = match symbol {
            Symbol::EthPerp => &self.eth,
            Symbol::BtcPerp => &self.btc,
        };
        match window {
            PriceWindow::ZScore => history.zscore.to_vec(),
            PriceWindow::Volatility => history.volatility.to_vec(),
            PriceWindow::SigmaQuantile => history.sigma.to_vec(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct BarsResponse {
    #[serde(default)]
    bars: Vec<BarRecord>,
}

#[derive(Debug, Deserialize)]
struct BarRecord {
    timestamp: String,
    #[serde(default)]
    mid: Option<Value>,
    #[serde(default)]
    mark: Option<Value>,
    #[serde(default)]
    close: Option<Value>,
}

pub fn align_to_bar_close(timestamp: DateTime<Utc>) -> DateTime<Utc> {
    let seconds = timestamp.timestamp();
    let aligned = seconds - seconds.rem_euclid(900);
    Utc.timestamp_opt(aligned, 0)
        .single()
        .expect("aligned timestamp must be valid")
}
