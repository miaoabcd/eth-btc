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
        let payload: Value = serde_json::from_str(body)
            .map_err(|err| AccountError::InvalidResponse(err.to_string()))?;
        let payload = payload.get("data").unwrap_or(&payload);
        let margin_summary = payload
            .get("marginSummary")
            .ok_or_else(|| AccountError::MissingData("marginSummary missing".to_string()))?;
        let total_raw = margin_summary
            .get("totalRawUsd")
            .ok_or_else(|| AccountError::MissingData("totalRawUsd missing".to_string()))?;
        Self::parse_decimal(total_raw)
    }
}

#[async_trait]
impl AccountBalanceSource for HyperliquidAccountSource {
    async fn fetch_available_balance(&self) -> Result<Decimal, AccountError> {
        self.rate_limiter.wait().await;
        let response = self
            .http
            .post(
                &self.endpoint_url(),
                serde_json::json!({"type": "userState", "user": self.user}),
            )
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
