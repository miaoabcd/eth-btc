use std::collections::{HashMap, VecDeque};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use alloy_primitives::{Address, B256, keccak256};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::{SolStruct, eip712_domain};
use rust_decimal::Decimal;
use rust_decimal::RoundingStrategy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::sleep;

use crate::config::{OrderType, Symbol};
use crate::state::PositionSnapshot;
use crate::util::rate_limiter::{FixedRateLimiter, RateLimiter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderSide {
    Buy,
    Sell,
}

impl OrderSide {
    pub fn close_for_qty(qty: Decimal) -> OrderSide {
        if qty > Decimal::ZERO {
            OrderSide::Sell
        } else {
            OrderSide::Buy
        }
    }
}

#[derive(Debug, Clone)]
pub struct OrderRequest {
    pub symbol: Symbol,
    pub side: OrderSide,
    pub qty: Decimal,
    pub order_type: OrderType,
    pub limit_price: Option<Decimal>,
}

#[derive(Debug, Clone)]
pub struct OrderHttpResponse {
    pub status: u16,
    pub body: String,
}

#[async_trait::async_trait]
pub trait OrderHttpClient: Send + Sync {
    async fn post(&self, url: &str, body: Value) -> Result<OrderHttpResponse, ExecutionError>;
}

pub trait NonceProvider: Send + Sync {
    fn next_nonce(&self) -> u64;
}

#[derive(Debug, Default)]
pub struct TimeNonceProvider {
    last: Mutex<u64>,
}

impl TimeNonceProvider {
    pub fn new() -> Self {
        Self::default()
    }
}

impl NonceProvider for TimeNonceProvider {
    fn next_nonce(&self) -> u64 {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let mut last = self.last.lock().expect("nonce lock");
        if now <= *last {
            *last += 1;
        } else {
            *last = now;
        }
        *last
    }
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum ExecutionError {
    #[error("transient error: {0}")]
    Transient(String),
    #[error("fatal error: {0}")]
    Fatal(String),
    #[error("partial fill: {0}")]
    PartialFill(String),
}

impl ExecutionError {
    fn is_transient(&self) -> bool {
        matches!(self, ExecutionError::Transient(_))
    }
}

#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts: usize,
    pub base_delay_ms: u64,
}

impl RetryConfig {
    pub fn fast() -> Self {
        Self {
            max_attempts: 2,
            base_delay_ms: 1,
        }
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self::fast()
    }
}

#[async_trait::async_trait]
pub trait OrderExecutor: Send + Sync {
    async fn submit(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError>;
    async fn close(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError>;
}

#[derive(Clone)]
pub struct ReqwestOrderClient {
    client: reqwest::Client,
    api_key: Option<String>,
}

impl ReqwestOrderClient {
    pub fn new(api_key: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
        }
    }
}

#[async_trait::async_trait]
impl OrderHttpClient for ReqwestOrderClient {
    async fn post(&self, url: &str, body: Value) -> Result<OrderHttpResponse, ExecutionError> {
        let mut request = self.client.post(url).json(&body);
        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }
        let response = request
            .send()
            .await
            .map_err(|err| ExecutionError::Transient(err.to_string()))?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|err| ExecutionError::Transient(err.to_string()))?;
        Ok(OrderHttpResponse { status, body })
    }
}

#[derive(Debug, Copy, Clone)]
struct AssetSpec {
    asset_id: u32,
    sz_decimals: u32,
}

#[derive(Debug, Deserialize)]
struct HyperliquidMetaResponse {
    #[serde(default)]
    universe: Vec<HyperliquidAssetInfo>,
}

#[derive(Debug, Deserialize)]
struct HyperliquidAssetInfo {
    name: String,
    #[serde(rename = "szDecimals")]
    sz_decimals: u32,
}

#[derive(Debug, Serialize)]
struct HyperliquidExchangeRequest {
    action: HyperliquidExchangeAction,
    nonce: u64,
    signature: HyperliquidSignature,
    #[serde(rename = "vaultAddress", skip_serializing_if = "Option::is_none")]
    vault_address: Option<String>,
    #[serde(rename = "expiresAfter", skip_serializing_if = "Option::is_none")]
    expires_after: Option<u64>,
}


#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum HyperliquidExchangeAction {
    #[serde(rename = "order")]
    Order {
        orders: Vec<HyperliquidOrderRequest>,
        #[serde(default)]
        grouping: HyperliquidOrderGrouping,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum HyperliquidOrderGrouping {
    #[serde(rename = "na")]
    Na,
}

#[derive(Debug, Serialize)]
struct HyperliquidOrderRequest {
    #[serde(rename = "a")]
    asset: u32,
    #[serde(rename = "b")]
    is_buy: bool,
    #[serde(rename = "p")]
    price: Decimal,
    #[serde(rename = "s")]
    size: Decimal,
    #[serde(rename = "r")]
    reduce_only: bool,
    #[serde(rename = "t")]
    kind: HyperliquidOrderType,
}

#[derive(Debug, Serialize)]
struct HyperliquidOrderType {
    #[serde(rename = "limit")]
    limit: HyperliquidLimitParams,
}

impl HyperliquidOrderType {
    fn ioc_limit() -> Self {
        Self {
            limit: HyperliquidLimitParams { tif: "Ioc" },
        }
    }
}

#[derive(Debug, Serialize)]
struct HyperliquidLimitParams {
    #[serde(rename = "tif")]
    tif: &'static str,
}

#[derive(Debug, Serialize)]
struct HyperliquidSignature {
    r: String,
    s: String,
    v: u64,
}

#[derive(Debug, Deserialize)]
struct HyperliquidExecResponse {
    status: String,
    response: HyperliquidExecResponseData,
}

impl HyperliquidExecResponse {
    fn filled_qty(self) -> Result<Decimal, ExecutionError> {
        if self.status != "ok" {
            return Err(ExecutionError::Fatal(format!(
                "exchange response status {}",
                self.status
            )));
        }
        match self.response {
            HyperliquidExecResponseData::Order { data } => data.filled_qty(),
            _ => Err(ExecutionError::Fatal(
                "unexpected exchange response".to_string(),
            )),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum HyperliquidExecResponseData {
    #[serde(rename = "order")]
    Order { data: HyperliquidExecOrderResponseData },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
struct HyperliquidExecOrderResponseData {
    statuses: Vec<HyperliquidExecOrderStatus>,
}

impl HyperliquidExecOrderResponseData {
    fn filled_qty(self) -> Result<Decimal, ExecutionError> {
        let status = self
            .statuses
            .into_iter()
            .next()
            .ok_or_else(|| ExecutionError::Fatal("empty order status".to_string()))?;
        match status {
            HyperliquidExecOrderStatus::Filled { filled } => Ok(filled.total_sz),
            HyperliquidExecOrderStatus::Error { error } => Err(ExecutionError::Fatal(error)),
            HyperliquidExecOrderStatus::Resting { .. } => Err(ExecutionError::Fatal(
                "order resting on book".to_string(),
            )),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum HyperliquidExecOrderStatus {
    Filled { filled: HyperliquidFilledInfo },
    Resting {
        #[allow(dead_code)]
        resting: serde_json::Value,
    },
    Error { error: String },
}

#[derive(Debug, Deserialize)]
struct HyperliquidFilledInfo {
    #[serde(rename = "totalSz")]
    total_sz: Decimal,
}

#[derive(Clone)]
pub struct HyperliquidSigner {
    private_key: String,
}

impl HyperliquidSigner {
    pub fn new(private_key: String) -> Self {
        Self { private_key }
    }

    fn signer(&self) -> Result<PrivateKeySigner, ExecutionError> {
        let key_hex = self.private_key.trim_start_matches("0x");
        PrivateKeySigner::from_str(key_hex)
            .map_err(|err| ExecutionError::Fatal(format!("invalid private key: {err}")))
    }

    fn connection_id(
        action: &HyperliquidExchangeAction,
        nonce: u64,
        vault_address: Option<&str>,
    ) -> Result<B256, ExecutionError> {
        let mut bytes = rmp_serde::to_vec_named(action)
            .map_err(|err| ExecutionError::Fatal(format!("serialize action: {err}")))?;
        bytes.extend_from_slice(&nonce.to_be_bytes());
        if let Some(vault) = vault_address {
            bytes.push(1);
            let vault_hex = vault.trim_start_matches("0x");
            let vault_bytes = hex::decode(vault_hex)
                .map_err(|err| ExecutionError::Fatal(format!("invalid vault address: {err}")))?;
            bytes.extend_from_slice(&vault_bytes);
        } else {
            bytes.push(0);
        }
        Ok(keccak256(&bytes))
    }
}

alloy_sol_types::sol! {
    struct Agent {
        string source;
        bytes32 connectionId;
    }
}

impl HyperliquidSigner {
    fn sign(
        &self,
        action: &HyperliquidExchangeAction,
        nonce: u64,
        is_testnet: bool,
        vault_address: Option<&str>,
    ) -> Result<HyperliquidSignature, ExecutionError> {
        let connection_id = Self::connection_id(action, nonce, vault_address)?;
        let agent = Agent {
            source: if is_testnet { "b".to_string() } else { "a".to_string() },
            connectionId: connection_id,
        };
        let domain = eip712_domain! {
            name: "Exchange",
            version: "1",
            chain_id: 1337,
            verifying_contract: Address::ZERO,
        };
        let signing_hash = agent.eip712_signing_hash(&domain);
        let signer = self.signer()?;
        let signature = signer
            .sign_hash_sync(&B256::from(signing_hash.0))
            .map_err(|err| ExecutionError::Fatal(format!("signing failed: {err}")))?;
        let v = if signature.v() { 28u64 } else { 27u64 };
        Ok(HyperliquidSignature {
            r: format!("0x{:064x}", signature.r()),
            s: format!("0x{:064x}", signature.s()),
            v,
        })
    }
}

#[derive(Clone)]
pub struct LiveOrderExecutor {
    base_url: String,
    client: Arc<dyn OrderHttpClient>,
    rate_limiter: Arc<dyn RateLimiter>,
    asset_specs: Arc<AsyncMutex<Option<HashMap<Symbol, AssetSpec>>>>,
    signer: Option<HyperliquidSigner>,
    nonce_provider: Arc<dyn NonceProvider>,
    vault_address: Option<String>,
    is_testnet: bool,
}

impl LiveOrderExecutor {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_client_and_rate_limiter(
            base_url,
            Arc::new(ReqwestOrderClient::new(None)),
            Arc::new(FixedRateLimiter::new(Duration::from_millis(200))),
        )
    }

    pub fn with_api_key(base_url: impl Into<String>, api_key: String) -> Self {
        Self::with_client_and_rate_limiter(
            base_url,
            Arc::new(ReqwestOrderClient::new(Some(api_key))),
            Arc::new(FixedRateLimiter::new(Duration::from_millis(200))),
        )
    }

    pub fn with_client(base_url: impl Into<String>, client: Arc<dyn OrderHttpClient>) -> Self {
        Self::with_client_and_rate_limiter(
            base_url,
            client,
            Arc::new(FixedRateLimiter::new(Duration::from_millis(200))),
        )
    }

    pub fn with_client_and_rate_limiter(
        base_url: impl Into<String>,
        client: Arc<dyn OrderHttpClient>,
        rate_limiter: Arc<dyn RateLimiter>,
    ) -> Self {
        let base_url = base_url.into();
        let is_testnet = Self::infer_testnet(&base_url);
        Self {
            base_url,
            client,
            rate_limiter,
            asset_specs: Arc::new(AsyncMutex::new(None)),
            signer: None,
            nonce_provider: Arc::new(TimeNonceProvider::new()),
            vault_address: None,
            is_testnet,
        }
    }

    pub fn with_private_key(base_url: impl Into<String>, private_key: String) -> Self {
        let mut executor = Self::new(base_url);
        executor.signer = Some(HyperliquidSigner::new(private_key));
        executor
    }

    pub fn with_signer(mut self, signer: HyperliquidSigner) -> Self {
        self.signer = Some(signer);
        self
    }

    pub fn with_nonce_provider(mut self, nonce_provider: Arc<dyn NonceProvider>) -> Self {
        self.nonce_provider = nonce_provider;
        self
    }

    pub fn with_vault_address(mut self, vault_address: String) -> Self {
        self.vault_address = Some(vault_address);
        self
    }

    pub fn with_testnet(mut self, is_testnet: bool) -> Self {
        self.is_testnet = is_testnet;
        self
    }

    fn infer_testnet(base_url: &str) -> bool {
        base_url.contains("testnet")
    }

    fn exchange_url(&self) -> String {
        format!("{}/exchange", self.base_url.trim_end_matches('/'))
    }

    fn info_url(&self) -> String {
        format!("{}/info", self.base_url.trim_end_matches('/'))
    }

    async fn load_asset_specs(&self) -> Result<HashMap<Symbol, AssetSpec>, ExecutionError> {
        let response = self
            .client
            .post(&self.info_url(), serde_json::json!({"type":"meta"}))
            .await?;
        match response.status {
            200 => {
                let meta: HyperliquidMetaResponse = serde_json::from_str(&response.body)
                    .map_err(|err| ExecutionError::Fatal(err.to_string()))?;
                let mut specs = HashMap::new();
                for (index, asset) in meta.universe.into_iter().enumerate() {
                    let symbol = match asset.name.as_str() {
                        "ETH" => Symbol::EthPerp,
                        "BTC" => Symbol::BtcPerp,
                        _ => continue,
                    };
                    specs.insert(
                        symbol,
                        AssetSpec {
                            asset_id: index as u32,
                            sz_decimals: asset.sz_decimals,
                        },
                    );
                }
                if !specs.contains_key(&Symbol::EthPerp) || !specs.contains_key(&Symbol::BtcPerp) {
                    return Err(ExecutionError::Fatal(
                        "meta response missing ETH/BTC assets".to_string(),
                    ));
                }
                Ok(specs)
            }
            429 => Err(ExecutionError::Transient("rate limited".to_string())),
            status if status >= 500 => Err(ExecutionError::Transient(format!(
                "server error {status}"
            ))),
            status => Err(ExecutionError::Fatal(format!(
                "client error {status}"
            ))),
        }
    }

    async fn asset_spec(&self, symbol: Symbol) -> Result<AssetSpec, ExecutionError> {
        let mut guard = self.asset_specs.lock().await;
        if let Some(specs) = guard.as_ref() {
            if let Some(spec) = specs.get(&symbol) {
                return Ok(*spec);
            }
        }
        let specs = self.load_asset_specs().await?;
        let spec = specs
            .get(&symbol)
            .copied()
            .ok_or_else(|| ExecutionError::Fatal("asset spec missing".to_string()))?;
        *guard = Some(specs);
        Ok(spec)
    }

    fn align_size(qty: Decimal, decimals: u32) -> Decimal {
        qty.round_dp_with_strategy(decimals, RoundingStrategy::ToZero)
    }

    async fn post_order(
        &self,
        order: &OrderRequest,
        reduce_only: bool,
    ) -> Result<Decimal, ExecutionError> {
        self.rate_limiter.wait().await;
        let spec = self.asset_spec(order.symbol).await?;
        let price = order.limit_price.ok_or_else(|| {
            ExecutionError::Fatal("limit_price required for Hyperliquid orders".to_string())
        })?;
        let size = Self::align_size(order.qty, spec.sz_decimals);
        if size <= Decimal::ZERO {
            return Err(ExecutionError::Fatal(
                "order size rounds to zero".to_string(),
            ));
        }
        let action = HyperliquidExchangeAction::Order {
            orders: vec![HyperliquidOrderRequest {
                asset: spec.asset_id,
                is_buy: matches!(order.side, OrderSide::Buy),
                price,
                size,
                reduce_only,
                kind: HyperliquidOrderType::ioc_limit(),
            }],
            grouping: HyperliquidOrderGrouping::Na,
        };
        let signer = self.signer.as_ref().ok_or_else(|| {
            ExecutionError::Fatal("missing Hyperliquid private key".to_string())
        })?;
        let nonce = self.nonce_provider.next_nonce();
        let signature =
            signer.sign(&action, nonce, self.is_testnet, self.vault_address.as_deref())?;
        let payload = HyperliquidExchangeRequest {
            action,
            nonce,
            signature,
            vault_address: self.vault_address.clone(),
            expires_after: None,
        };
        let body =
            serde_json::to_value(payload).map_err(|err| ExecutionError::Fatal(err.to_string()))?;
        let response = self.client.post(&self.exchange_url(), body).await?;
        if response.status >= 500 {
            return Err(ExecutionError::Transient(format!(
                "server error {status}",
                status = response.status
            )));
        }
        if response.status >= 400 {
            return Err(ExecutionError::Fatal(format!(
                "client error {status}",
                status = response.status
            )));
        }
        let parsed: HyperliquidExecResponse = serde_json::from_str(&response.body)
            .map_err(|err| ExecutionError::Fatal(err.to_string()))?;
        parsed.filled_qty()
    }
}

#[async_trait::async_trait]
impl OrderExecutor for LiveOrderExecutor {
    async fn submit(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError> {
        self.post_order(order, false).await
    }

    async fn close(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError> {
        self.post_order(order, true).await
    }
}

#[derive(Clone)]
pub struct ExecutionEngine {
    executor: Arc<dyn OrderExecutor>,
    retry: RetryConfig,
}

impl ExecutionEngine {
    pub fn new(executor: Arc<dyn OrderExecutor>, retry: RetryConfig) -> Self {
        Self { executor, retry }
    }

    pub async fn open_pair(
        &self,
        eth_order: OrderRequest,
        btc_order: OrderRequest,
    ) -> Result<(), ExecutionError> {
        let eth_fill = self.retry_submit(&eth_order).await;
        let eth_fill = match eth_fill {
            Ok(fill) => fill,
            Err(err) => return Err(err),
        };

        match self.retry_submit(&btc_order).await {
            Ok(_) => Ok(()),
            Err(err) => {
                let _ = self
                    .retry_close(&OrderRequest {
                        symbol: eth_order.symbol,
                        side: OrderSide::close_for_qty(eth_fill),
                        qty: eth_fill.abs(),
                        order_type: OrderType::Market,
                        limit_price: eth_order.limit_price,
                    })
                    .await;
                Err(ExecutionError::PartialFill(err.to_string()))
            }
        }
    }

    pub async fn close_pair(
        &self,
        eth_order: OrderRequest,
        btc_order: OrderRequest,
    ) -> Result<(), ExecutionError> {
        let eth_result = self.retry_close(&eth_order).await;
        if eth_result.is_err() {
            return eth_result.map(|_| ());
        }
        let btc_result = self.retry_close(&btc_order).await;
        if let Err(err) = btc_result {
            let rollback_order = OrderRequest {
                symbol: eth_order.symbol,
                side: match eth_order.side {
                    OrderSide::Buy => OrderSide::Sell,
                    OrderSide::Sell => OrderSide::Buy,
                },
                qty: eth_order.qty,
                order_type: eth_order.order_type,
                limit_price: eth_order.limit_price,
            };
            let rollback = self.retry_submit(&rollback_order).await;
            return match rollback {
                Ok(_) => Err(ExecutionError::PartialFill(format!(
                    "close second leg failed: {err}; rollback executed"
                ))),
                Err(rollback_err) => Err(ExecutionError::PartialFill(format!(
                    "close second leg failed: {err}; rollback failed: {rollback_err}"
                ))),
            };
        }
        Ok(())
    }

    pub async fn repair_residual(&self, position: &PositionSnapshot) -> Result<(), ExecutionError> {
        if position.eth.qty != Decimal::ZERO && position.btc.qty == Decimal::ZERO {
            let order = OrderRequest {
                symbol: Symbol::EthPerp,
                side: OrderSide::close_for_qty(position.eth.qty),
                qty: position.eth.qty.abs(),
                order_type: OrderType::Market,
                limit_price: Some(position.eth.avg_price),
            };
            return self.retry_close(&order).await.map(|_| ());
        }
        if position.btc.qty != Decimal::ZERO && position.eth.qty == Decimal::ZERO {
            let order = OrderRequest {
                symbol: Symbol::BtcPerp,
                side: OrderSide::close_for_qty(position.btc.qty),
                qty: position.btc.qty.abs(),
                order_type: OrderType::Market,
                limit_price: Some(position.btc.avg_price),
            };
            return self.retry_close(&order).await.map(|_| ());
        }
        Ok(())
    }

    async fn retry_submit(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError> {
        self.retry_with(|| self.executor.submit(order)).await
    }

    async fn retry_close(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError> {
        self.retry_with(|| self.executor.close(order)).await
    }

    async fn retry_with<F, Fut>(&self, mut action: F) -> Result<Decimal, ExecutionError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<Decimal, ExecutionError>>,
    {
        let mut delay = self.retry.base_delay_ms;
        let attempts = self.retry.max_attempts.max(1);
        for attempt in 0..attempts {
            match action().await {
                Ok(value) => return Ok(value),
                Err(err) => {
                    if err.is_transient() && attempt + 1 < attempts {
                        sleep(Duration::from_millis(delay)).await;
                        delay = delay.saturating_mul(2);
                        continue;
                    }
                    return Err(err);
                }
            }
        }
        Err(ExecutionError::Transient(
            "retry attempts exhausted".to_string(),
        ))
    }
}

#[derive(Default)]
pub struct MockOrderExecutor {
    submit_responses: Mutex<HashMap<Symbol, VecDeque<Result<Decimal, ExecutionError>>>>,
    close_responses: Mutex<HashMap<Symbol, VecDeque<Result<Decimal, ExecutionError>>>>,
}

impl MockOrderExecutor {
    pub fn push_submit_response(
        &mut self,
        symbol: Symbol,
        response: Result<Decimal, ExecutionError>,
    ) {
        let queue = self
            .submit_responses
            .get_mut()
            .expect("mock submit lock poisoned")
            .entry(symbol)
            .or_default();
        queue.push_back(response);
    }

    pub fn push_close_response(
        &mut self,
        symbol: Symbol,
        response: Result<Decimal, ExecutionError>,
    ) {
        let queue = self
            .close_responses
            .get_mut()
            .expect("mock close lock poisoned")
            .entry(symbol)
            .or_default();
        queue.push_back(response);
    }

    fn pop_response(
        store: &Mutex<HashMap<Symbol, VecDeque<Result<Decimal, ExecutionError>>>>,
        symbol: Symbol,
    ) -> Result<Decimal, ExecutionError> {
        let mut guard = store.lock().expect("mock lock poisoned");
        let queue = guard.entry(symbol).or_default();
        queue
            .pop_front()
            .unwrap_or_else(|| Err(ExecutionError::Fatal("no mock response".to_string())))
    }
}

#[async_trait::async_trait]
impl OrderExecutor for MockOrderExecutor {
    async fn submit(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError> {
        Self::pop_response(&self.submit_responses, order.symbol)
    }

    async fn close(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError> {
        Self::pop_response(&self.close_responses, order.symbol)
    }
}

#[derive(Debug, Default, Clone)]
pub struct PaperOrderExecutor;

#[async_trait::async_trait]
impl OrderExecutor for PaperOrderExecutor {
    async fn submit(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError> {
        Ok(order.qty)
    }

    async fn close(&self, order: &OrderRequest) -> Result<Decimal, ExecutionError> {
        Ok(order.qty)
    }
}
