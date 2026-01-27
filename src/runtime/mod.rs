use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use thiserror::Error;
use tokio::sync::watch;
use tokio::time::interval;
use tracing::{info, warn};

use crate::core::strategy::{StrategyBar, StrategyEngine, StrategyError, StrategyOutcome};
use crate::data::{DataError, PriceFetcher};
use crate::funding::FundingFetcher;
use crate::state::{StateError, StateStore, StrategyState};

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("data error: {0}")]
    Data(String),
    #[error("strategy error: {0}")]
    Strategy(String),
    #[error("state error: {0}")]
    State(String),
}

pub trait StateWriter: Send + Sync {
    fn save(&self, state: &StrategyState) -> Result<(), StateError>;
}

pub struct StateStoreWriter {
    store: std::sync::Mutex<StateStore>,
}

impl StateStoreWriter {
    pub fn new(store: StateStore) -> Self {
        Self {
            store: std::sync::Mutex::new(store),
        }
    }
}

impl StateWriter for StateStoreWriter {
    fn save(&self, state: &StrategyState) -> Result<(), StateError> {
        let store = self
            .store
            .lock()
            .map_err(|_| StateError::Persistence("state store mutex poisoned".to_string()))?;
        store.save(state)
    }
}

pub struct LiveRunner {
    engine: StrategyEngine,
    price_fetcher: PriceFetcher,
    funding_fetcher: Option<FundingFetcher>,
    state_writer: Option<Arc<dyn StateWriter>>,
    now: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
}

impl LiveRunner {
    pub fn new(
        engine: StrategyEngine,
        price_fetcher: PriceFetcher,
        funding_fetcher: Option<FundingFetcher>,
    ) -> Self {
        Self {
            engine,
            price_fetcher,
            funding_fetcher,
            state_writer: None,
            now: Arc::new(Utc::now),
        }
    }

    pub fn with_clock(mut self, now: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>) -> Self {
        self.now = now;
        self
    }

    pub fn with_state_writer(mut self, writer: Arc<dyn StateWriter>) -> Self {
        self.state_writer = Some(writer);
        self
    }

    pub async fn run_once(&mut self) -> Result<StrategyOutcome, RunnerError> {
        let now = (self.now)();
        self.run_once_at(now).await
    }

    pub async fn run_once_at(
        &mut self,
        timestamp: DateTime<Utc>,
    ) -> Result<StrategyOutcome, RunnerError> {
        let snapshot = self
            .price_fetcher
            .fetch_pair_prices(timestamp)
            .await
            .map_err(|err| RunnerError::Data(err.to_string()))?;

        let funding = if let Some(fetcher) = &self.funding_fetcher {
            match fetcher.fetch_pair_rates(snapshot.timestamp).await {
                Ok(snapshot) => Some(snapshot),
                Err(err) => {
                    warn!(error = ?err, "funding fetch failed; proceeding without funding");
                    None
                }
            }
        } else {
            None
        };

        let bar = StrategyBar {
            timestamp: snapshot.timestamp,
            eth_price: snapshot.eth,
            btc_price: snapshot.btc,
            funding_eth: funding.as_ref().map(|value| value.eth.rate),
            funding_btc: funding.as_ref().map(|value| value.btc.rate),
        };

        let outcome = self
            .engine
            .process_bar(bar)
            .await
            .map_err(|err| RunnerError::Strategy(err.to_string()))?;
        if let Some(writer) = &self.state_writer {
            writer
                .save(self.engine.state().state())
                .map_err(|err| RunnerError::State(err.to_string()))?;
        }
        info!(events = ?outcome.events, "processed bar");
        Ok(outcome)
    }

    pub async fn run_loop(
        &mut self,
        tick_interval: Duration,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), RunnerError> {
        let mut ticker = interval(tick_interval);
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let _ = self.run_once().await?;
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("shutdown signal received");
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}

impl From<DataError> for RunnerError {
    fn from(err: DataError) -> Self {
        RunnerError::Data(err.to_string())
    }
}

impl From<StrategyError> for RunnerError {
    fn from(err: StrategyError) -> Self {
        RunnerError::Strategy(err.to_string())
    }
}

impl From<StateError> for RunnerError {
    fn from(err: StateError) -> Self {
        RunnerError::State(err.to_string())
    }
}
