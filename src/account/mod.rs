use std::collections::VecDeque;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde_json::Value;
use thiserror::Error;

use crate::util::rate_limiter::{FixedRateLimiter, RateLimiter};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AccountError {
    #[error("missing data: {0}")]
    MissingData(String),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("rate limited")]
    RateLimited,
    #[error("http error: {0}")]
    Http(String),
}

#[derive(Debug, Clone)]
pub struct AccountHttpResponse {
    pub status: u16,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExchangePosition {
    pub qty: Decimal,
    pub entry_price: Decimal,
    pub notional: Decimal,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PairExposure {
    pub eth: Option<ExchangePosition>,
    pub btc: Option<ExchangePosition>,
}

impl PairExposure {
    pub fn eth_qty(&self) -> Decimal {
        self.eth
            .as_ref()
            .map(|position| position.qty)
            .unwrap_or(Decimal::ZERO)
    }

    pub fn btc_qty(&self) -> Decimal {
        self.btc
            .as_ref()
            .map(|position| position.qty)
            .unwrap_or(Decimal::ZERO)
    }

    pub fn is_flat(&self) -> bool {
        self.eth_qty() == Decimal::ZERO && self.btc_qty() == Decimal::ZERO
    }

    pub fn has_residual(&self) -> bool {
        let eth_zero = self.eth_qty() == Decimal::ZERO;
        let btc_zero = self.btc_qty() == Decimal::ZERO;
        (eth_zero && !btc_zero) || (!eth_zero && btc_zero)
    }
}

#[async_trait]
pub trait AccountHttpClient: Send + Sync {
    async fn post(&self, url: &str, body: Value) -> Result<AccountHttpResponse, AccountError>;
}

#[derive(Clone)]
pub struct ReqwestAccountClient {
    client: reqwest::Client,
}

impl ReqwestAccountClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl AccountHttpClient for ReqwestAccountClient {
    async fn post(&self, url: &str, body: Value) -> Result<AccountHttpResponse, AccountError> {
        let response = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(|err| AccountError::Http(err.to_string()))?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|err| AccountError::Http(err.to_string()))?;
        Ok(AccountHttpResponse { status, body })
    }
}

#[async_trait]
pub trait AccountBalanceSource: Send + Sync {
    async fn fetch_available_balance(&self) -> Result<Decimal, AccountError>;
}

#[async_trait]
pub trait AccountPositionSource: Send + Sync {
    async fn fetch_pair_exposure(&self) -> Result<PairExposure, AccountError>;
}

#[derive(Clone)]
pub struct HyperliquidAccountSource {
    base_url: String,
    user: String,
    http: Arc<dyn AccountHttpClient>,
    rate_limiter: Arc<dyn RateLimiter>,
}

impl HyperliquidAccountSource {
    pub fn new(base_url: impl Into<String>, user: impl Into<String>) -> Self {
        Self::with_client_and_rate_limiter(
            base_url,
            user,
            Arc::new(ReqwestAccountClient::new()),
            Arc::new(FixedRateLimiter::new(Duration::from_millis(200))),
        )
    }

    pub fn with_client_and_rate_limiter(
        base_url: impl Into<String>,
        user: impl Into<String>,
        http: Arc<dyn AccountHttpClient>,
        rate_limiter: Arc<dyn RateLimiter>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            user: user.into(),
            http,
            rate_limiter,
        }
    }

    fn endpoint_url(&self) -> String {
        format!("{}/info", self.base_url.trim_end_matches('/'))
    }

    fn request_body(&self) -> Value {
        serde_json::json!({"type": "clearinghouseState", "user": self.user})
    }

    fn parse_payload(body: &str) -> Result<Value, AccountError> {
        let payload: Value = serde_json::from_str(body)
            .map_err(|err| AccountError::InvalidResponse(err.to_string()))?;
        Ok(payload.get("data").cloned().unwrap_or(payload))
    }

    fn parse_decimal(value: &Value) -> Result<Decimal, AccountError> {
        let value = match value {
            Value::String(value) => value.clone(),
            Value::Number(value) => value.to_string(),
            other => {
                return Err(AccountError::InvalidResponse(format!(
                    "unsupported numeric value: {other}"
                )));
            }
        };
        Decimal::from_str(&value)
            .map_err(|err| AccountError::InvalidResponse(format!("invalid decimal {value}: {err}")))
    }

    fn parse_available_balance(&self, body: &str) -> Result<Decimal, AccountError> {
        let payload = Self::parse_payload(body)?;
        let margin_summary = payload
            .get("marginSummary")
            .ok_or_else(|| AccountError::MissingData("marginSummary missing".to_string()))?;
        let total_raw = margin_summary
            .get("totalRawUsd")
            .ok_or_else(|| AccountError::MissingData("totalRawUsd missing".to_string()))?;
        Self::parse_decimal(total_raw)
    }

    fn parse_position(position: &Value) -> Result<Option<(&str, ExchangePosition)>, AccountError> {
        let position = position.get("position").unwrap_or(position);
        let coin = position
            .get("coin")
            .and_then(Value::as_str)
            .ok_or_else(|| AccountError::MissingData("position.coin missing".to_string()))?;
        let qty = position
            .get("szi")
            .ok_or_else(|| AccountError::MissingData("position.szi missing".to_string()))
            .and_then(Self::parse_decimal)?;
        if qty == Decimal::ZERO {
            return Ok(None);
        }
        let entry_price = position
            .get("entryPx")
            .map(Self::parse_decimal)
            .transpose()?
            .unwrap_or(Decimal::ZERO);
        let notional = position
            .get("positionValue")
            .map(Self::parse_decimal)
            .transpose()?
            .unwrap_or_else(|| qty.abs() * entry_price);
        Ok(Some((
            coin,
            ExchangePosition {
                qty,
                entry_price,
                notional,
            },
        )))
    }

    fn parse_pair_exposure(&self, body: &str) -> Result<PairExposure, AccountError> {
        let payload = Self::parse_payload(body)?;
        let asset_positions = payload
            .get("assetPositions")
            .and_then(Value::as_array)
            .ok_or_else(|| AccountError::MissingData("assetPositions missing".to_string()))?;
        let mut exposure = PairExposure::default();
        for asset_position in asset_positions {
            let Some((coin, position)) = Self::parse_position(asset_position)? else {
                continue;
            };
            match coin {
                "ETH" => exposure.eth = Some(position),
                "BTC" => exposure.btc = Some(position),
                _ => {}
            }
        }
        Ok(exposure)
    }
}

#[async_trait]
impl AccountBalanceSource for HyperliquidAccountSource {
    async fn fetch_available_balance(&self) -> Result<Decimal, AccountError> {
        self.rate_limiter.wait().await;
        let response = self
            .http
            .post(&self.endpoint_url(), self.request_body())
            .await?;
        match response.status {
            200 => self.parse_available_balance(&response.body),
            429 => Err(AccountError::RateLimited),
            status if status >= 500 => Err(AccountError::Http(format!("server error {status}"))),
            status => Err(AccountError::InvalidResponse(format!(
                "client error {status}"
            ))),
        }
    }
}

#[async_trait]
impl AccountPositionSource for HyperliquidAccountSource {
    async fn fetch_pair_exposure(&self) -> Result<PairExposure, AccountError> {
        self.rate_limiter.wait().await;
        let response = self
            .http
            .post(&self.endpoint_url(), self.request_body())
            .await?;
        match response.status {
            200 => self.parse_pair_exposure(&response.body),
            429 => Err(AccountError::RateLimited),
            status if status >= 500 => Err(AccountError::Http(format!("server error {status}"))),
            status => Err(AccountError::InvalidResponse(format!(
                "client error {status}"
            ))),
        }
    }
}

#[derive(Default)]
pub struct MockAccountSource {
    responses: Mutex<VecDeque<Result<Decimal, AccountError>>>,
}

impl MockAccountSource {
    pub fn push_response(&mut self, response: Result<Decimal, AccountError>) {
        let queue = self.responses.get_mut().expect("mock response lock");
        queue.push_back(response);
    }

    fn pop_response(&self) -> Result<Decimal, AccountError> {
        let mut guard = self.responses.lock().expect("mock response lock");
        guard
            .pop_front()
            .unwrap_or_else(|| Err(AccountError::MissingData("no mock response".to_string())))
    }
}

#[async_trait]
impl AccountBalanceSource for MockAccountSource {
    async fn fetch_available_balance(&self) -> Result<Decimal, AccountError> {
        self.pop_response()
    }
}
