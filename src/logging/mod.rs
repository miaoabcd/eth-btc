use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::time::sleep;

use crate::config::LogFormat;
use crate::core::{ExitReason, TradeDirection};
use crate::state::{PositionSnapshot, StrategyStatus};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogEvent {
    Entry,
    Exit(ExitReason),
    CooldownStart,
    CooldownEnd,
    ResidualRepair,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeEvent {
    Entry,
    Exit(ExitReason),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeLog {
    pub timestamp: DateTime<Utc>,
    pub event: TradeEvent,
    pub direction: TradeDirection,
    pub eth_qty: Decimal,
    pub btc_qty: Decimal,
    pub eth_price: Decimal,
    pub btc_price: Decimal,
    pub entry_time: DateTime<Utc>,
    pub entry_eth_price: Decimal,
    pub entry_btc_price: Decimal,
    pub realized_pnl: Decimal,
    pub cumulative_realized_pnl: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarLog {
    pub timestamp: DateTime<Utc>,
    pub eth_price: Option<Decimal>,
    pub btc_price: Option<Decimal>,
    pub r: Option<Decimal>,
    pub mu: Option<Decimal>,
    pub sigma: Option<Decimal>,
    pub sigma_eff: Option<Decimal>,
    pub zscore: Option<Decimal>,
    pub vol_eth: Option<Decimal>,
    pub vol_btc: Option<Decimal>,
    pub w_eth: Option<Decimal>,
    pub w_btc: Option<Decimal>,
    pub notional_eth: Option<Decimal>,
    pub notional_btc: Option<Decimal>,
    pub funding_eth: Option<Decimal>,
    pub funding_btc: Option<Decimal>,
    pub funding_cost_est: Option<Decimal>,
    pub funding_skip: Option<bool>,
    pub unrealized_pnl: Decimal,
    pub state: StrategyStatus,
    pub position: Option<PositionSnapshot>,
    pub events: Vec<LogEvent>,
}

impl BarLog {
    pub fn to_json_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

#[derive(Debug, Clone, Default)]
pub struct LogFormatter;

impl LogFormatter {
    pub fn format_json(&self, bar: &BarLog) -> Result<String, serde_json::Error> {
        serde_json::to_string(bar)
    }

    pub fn format_text(&self, bar: &BarLog) -> String {
        let eth = bar
            .eth_price
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NA".to_string());
        let btc = bar
            .btc_price
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NA".to_string());
        let z = bar
            .zscore
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NA".to_string());
        format!(
            "[{}] ETH={} BTC={} Z={} UPNL={} STATE={:?}",
            bar.timestamp.to_rfc3339(),
            eth,
            btc,
            z,
            bar.unrealized_pnl,
            bar.state
        )
    }
}

pub trait BarLogWriter: Send + Sync {
    fn write(&self, bar: &BarLog) -> Result<(), std::io::Error>;
}

pub struct BarLogFileWriter {
    logger: Mutex<FileLogger>,
    formatter: LogFormatter,
    format: LogFormat,
}

impl BarLogFileWriter {
    pub fn new(path: PathBuf, format: LogFormat) -> Result<Self, std::io::Error> {
        let logger = FileLogger::new(path, RotationConfig::default())?;
        Ok(Self {
            logger: Mutex::new(logger),
            formatter: LogFormatter::default(),
            format,
        })
    }
}

impl BarLogWriter for BarLogFileWriter {
    fn write(&self, bar: &BarLog) -> Result<(), std::io::Error> {
        let line = match self.format {
            LogFormat::Json => self
                .formatter
                .format_json(bar)
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?,
            LogFormat::Text => self.formatter.format_text(bar),
        };
        let mut logger = self.logger.lock().expect("stats log lock");
        logger.write_line(&line)
    }
}

#[derive(Debug, Clone, Default)]
pub struct TradeLogFormatter;

impl TradeLogFormatter {
    pub fn format_json(&self, log: &TradeLog) -> Result<String, serde_json::Error> {
        serde_json::to_string(log)
    }

    pub fn format_text(&self, log: &TradeLog) -> String {
        format!(
            "[{}] EVENT={:?} DIR={:?} ETH_QTY={} BTC_QTY={} ETH_PX={} BTC_PX={} ENTRY_TIME={} ENTRY_ETH_PX={} ENTRY_BTC_PX={} REALIZED_PNL={} CUM_REALIZED_PNL={}",
            log.timestamp.to_rfc3339(),
            log.event,
            log.direction,
            log.eth_qty,
            log.btc_qty,
            log.eth_price,
            log.btc_price,
            log.entry_time.to_rfc3339(),
            log.entry_eth_price,
            log.entry_btc_price,
            log.realized_pnl,
            log.cumulative_realized_pnl
        )
    }
}

pub trait TradeLogWriter: Send + Sync {
    fn write(&self, log: &TradeLog) -> Result<(), std::io::Error>;
}

pub struct TradeLogFileWriter {
    logger: Mutex<FileLogger>,
    formatter: TradeLogFormatter,
    format: LogFormat,
}

impl TradeLogFileWriter {
    pub fn new(path: PathBuf, format: LogFormat) -> Result<Self, std::io::Error> {
        let logger = FileLogger::new(path, RotationConfig::default())?;
        Ok(Self {
            logger: Mutex::new(logger),
            formatter: TradeLogFormatter::default(),
            format,
        })
    }
}

impl TradeLogWriter for TradeLogFileWriter {
    fn write(&self, log: &TradeLog) -> Result<(), std::io::Error> {
        let line = match self.format {
            LogFormat::Json => self
                .formatter
                .format_json(log)
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?,
            LogFormat::Text => self.formatter.format_text(log),
        };
        let mut logger = self.logger.lock().expect("trade log lock");
        logger.write_line(&line)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertLevel {
    Critical,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub level: AlertLevel,
    pub message: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Error, Clone)]
pub enum AlertError {
    #[error("transient alert error: {0}")]
    Transient(String),
    #[error("fatal alert error: {0}")]
    Fatal(String),
    #[error("throttled")]
    Throttled,
}

#[async_trait::async_trait]
pub trait AlertChannel: Send + Sync {
    async fn send(&self, alert: Alert) -> Result<(), AlertError>;
}

#[derive(Clone, Default)]
pub struct InMemoryAlertChannel {
    inner: Arc<Mutex<Vec<Alert>>>,
}

impl InMemoryAlertChannel {
    pub fn alerts(&self) -> Vec<Alert> {
        self.inner.lock().expect("alert lock poisoned").clone()
    }
}

#[async_trait::async_trait]
impl AlertChannel for InMemoryAlertChannel {
    async fn send(&self, alert: Alert) -> Result<(), AlertError> {
        self.inner.lock().expect("alert lock poisoned").push(alert);
        Ok(())
    }
}

#[derive(Clone)]
pub struct AlertDispatcher {
    channels: Vec<Arc<dyn AlertChannel>>,
}

impl AlertDispatcher {
    pub fn new(channels: Vec<Arc<dyn AlertChannel>>) -> Self {
        Self { channels }
    }

    pub async fn send(&self, alert: Alert) -> Result<(), AlertError> {
        for channel in &self.channels {
            channel.send(alert.clone()).await?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct AlertResponse {
    pub status: u16,
    pub body: String,
}

#[async_trait::async_trait]
pub trait AlertHttpClient: Send + Sync {
    async fn post(&self, url: &str, payload: &str) -> Result<AlertResponse, AlertError>;
}

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_attempts: usize,
    pub base_delay_ms: u64,
}

impl RetryPolicy {
    pub fn fast() -> Self {
        Self {
            max_attempts: 2,
            base_delay_ms: 1,
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::fast()
    }
}

pub struct WebhookChannel {
    url: String,
    retry: RetryPolicy,
    client: Box<dyn AlertHttpClient>,
}

impl WebhookChannel {
    pub fn new(url: String, retry: RetryPolicy, client: Box<dyn AlertHttpClient>) -> Self {
        Self { url, retry, client }
    }

    async fn send_with_retry(&self, payload: &str) -> Result<(), AlertError> {
        let mut delay = self.retry.base_delay_ms;
        for attempt in 0..self.retry.max_attempts {
            let response = self.client.post(&self.url, payload).await;
            match response {
                Ok(response) if response.status < 400 => return Ok(()),
                Ok(response) if response.status >= 500 => {
                    if attempt + 1 == self.retry.max_attempts {
                        return Err(AlertError::Transient(format!(
                            "webhook status {}",
                            response.status
                        )));
                    }
                }
                Ok(response) => {
                    return Err(AlertError::Fatal(format!(
                        "webhook status {}",
                        response.status
                    )));
                }
                Err(err) => {
                    if attempt + 1 == self.retry.max_attempts {
                        return Err(err);
                    }
                }
            }
            sleep(Duration::from_millis(delay)).await;
            delay = delay.saturating_mul(2);
        }
        Err(AlertError::Transient("webhook retry exhausted".to_string()))
    }
}

#[async_trait::async_trait]
impl AlertChannel for WebhookChannel {
    async fn send(&self, alert: Alert) -> Result<(), AlertError> {
        let payload =
            serde_json::to_string(&alert).map_err(|err| AlertError::Fatal(err.to_string()))?;
        self.send_with_retry(&payload).await
    }
}

#[async_trait::async_trait]
pub trait EmailTransport: Send + Sync {
    async fn send(&self, subject: &str, body: &str) -> Result<(), AlertError>;
}

#[derive(Clone, Default)]
pub struct NoopEmailTransport;

#[async_trait::async_trait]
impl EmailTransport for NoopEmailTransport {
    async fn send(&self, _subject: &str, _body: &str) -> Result<(), AlertError> {
        Ok(())
    }
}

pub struct EmailChannel<T: EmailTransport + Clone> {
    transport: T,
    throttle_seconds: i64,
    last_sent: Mutex<Option<DateTime<Utc>>>,
}

impl<T: EmailTransport + Clone> EmailChannel<T> {
    pub fn new(transport: T, throttle_seconds: i64) -> Self {
        Self {
            transport,
            throttle_seconds,
            last_sent: Mutex::new(None),
        }
    }

    async fn send_inner(&self, alert: Alert) -> Result<(), AlertError> {
        {
            let last_sent = self.last_sent.lock().expect("email lock poisoned");
            if let Some(last) = *last_sent
                && (alert.timestamp - last).num_seconds() < self.throttle_seconds
            {
                return Err(AlertError::Throttled);
            }
        }
        let subject = format!("[{:?}] Alert", alert.level);
        self.transport.send(&subject, &alert.message).await?;
        let mut last_sent = self.last_sent.lock().expect("email lock poisoned");
        *last_sent = Some(alert.timestamp);
        Ok(())
    }
}

#[async_trait::async_trait]
impl<T: EmailTransport + Clone> AlertChannel for EmailChannel<T> {
    async fn send(&self, alert: Alert) -> Result<(), AlertError> {
        self.send_inner(alert).await
    }
}

pub fn redact_json_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut redacted = serde_json::Map::new();
            for (key, val) in map {
                let lowered = key.to_lowercase();
                if lowered.contains("key")
                    || lowered.contains("secret")
                    || lowered.contains("token")
                    || lowered.contains("password")
                {
                    redacted.insert(key.clone(), Value::String("***".to_string()));
                } else {
                    redacted.insert(key.clone(), redact_json_value(val));
                }
            }
            Value::Object(redacted)
        }
        Value::Array(items) => Value::Array(items.iter().map(redact_json_value).collect()),
        other => other.clone(),
    }
}

#[derive(Debug, Clone)]
pub struct RotationConfig {
    pub max_bytes: u64,
    pub max_files: usize,
}

impl Default for RotationConfig {
    fn default() -> Self {
        Self {
            max_bytes: 10 * 1024 * 1024,
            max_files: 5,
        }
    }
}

#[derive(Debug)]
pub struct FileLogger {
    path: PathBuf,
    rotation: RotationConfig,
}

impl FileLogger {
    pub fn new(path: PathBuf, rotation: RotationConfig) -> Result<Self, std::io::Error> {
        if rotation.max_files == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "max_files must be > 0",
            ));
        }
        Ok(Self { path, rotation })
    }

    pub fn write_line(&mut self, line: &str) -> Result<(), std::io::Error> {
        let bytes = line.len() as u64 + 1;
        if self.path.exists() {
            let metadata = fs::metadata(&self.path)?;
            if metadata.len() + bytes > self.rotation.max_bytes {
                self.rotate()?;
            }
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    fn rotate(&self) -> Result<(), std::io::Error> {
        for index in (1..=self.rotation.max_files).rev() {
            let target = rotated_path(&self.path, index);
            let source = if index == 1 {
                self.path.clone()
            } else {
                rotated_path(&self.path, index - 1)
            };
            if source.exists() {
                if target.exists() {
                    fs::remove_file(&target)?;
                }
                fs::rename(&source, &target)?;
            }
        }
        Ok(())
    }
}

fn rotated_path(path: &Path, index: usize) -> PathBuf {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("log");
    let new_name = format!("{filename}.{index}");
    let mut rotated = path.to_path_buf();
    rotated.set_file_name(new_name);
    rotated
}
