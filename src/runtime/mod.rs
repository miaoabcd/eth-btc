use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use thiserror::Error;
use tokio::sync::{Mutex, watch};
use tokio::time::interval;
use tracing::{info, warn};

use crate::account::AccountBalanceSource;
use crate::core::strategy::{StrategyBar, StrategyEngine, StrategyError, StrategyOutcome};
use crate::data::{DataError, PriceFetcher};
use crate::funding::FundingFetcher;
use crate::logging::{BarLogWriter, TradeLogWriter};
use crate::state::{StateError, StateStore, StrategyState};
use crate::storage::{PriceBarRecord, PriceBarWriter};
pub mod backfill;

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error(transparent)]
    Data(#[from] DataError),
    #[error(transparent)]
    Strategy(#[from] StrategyError),
    #[error(transparent)]
    State(#[from] StateError),
}

#[async_trait::async_trait]
pub trait StateWriter: Send + Sync {
    async fn save(&self, state: &StrategyState) -> Result<(), StateError>;
}

pub struct StateStoreWriter {
    store: Mutex<StateStore>,
}

impl StateStoreWriter {
    pub fn new(store: StateStore) -> Self {
        Self {
            store: Mutex::new(store),
        }
    }
}

#[async_trait::async_trait]
impl StateWriter for StateStoreWriter {
    async fn save(&self, state: &StrategyState) -> Result<(), StateError> {
        let store = self.store.lock().await;
        store.save(state)
    }
}

pub struct LiveRunner {
    engine: StrategyEngine,
    price_fetcher: PriceFetcher,
    funding_fetcher: Option<FundingFetcher>,
    account_source: Option<Arc<dyn AccountBalanceSource>>,
    state_writer: Option<Arc<dyn StateWriter>>,
    stats_writer: Option<Arc<dyn BarLogWriter>>,
    trade_writer: Option<Arc<dyn TradeLogWriter>>,
    price_writer: Option<Arc<dyn PriceBarWriter>>,
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
            account_source: None,
            state_writer: None,
            stats_writer: None,
            trade_writer: None,
            price_writer: None,
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

    pub fn with_account_source(mut self, source: Arc<dyn AccountBalanceSource>) -> Self {
        self.account_source = Some(source);
        self
    }

    pub fn with_stats_writer(mut self, writer: Arc<dyn BarLogWriter>) -> Self {
        self.stats_writer = Some(writer);
        self
    }

    pub fn with_trade_writer(mut self, writer: Arc<dyn TradeLogWriter>) -> Self {
        self.trade_writer = Some(writer);
        self
    }

    pub fn with_price_writer(mut self, writer: Arc<dyn PriceBarWriter>) -> Self {
        self.price_writer = Some(writer);
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
        let bars_snapshot = self.price_fetcher.fetch_pair_bars(timestamp).await?;
        let snapshot = bars_snapshot.snapshot.clone();

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

        let equity = if let Some(source) = &self.account_source {
            match source.fetch_available_balance().await {
                Ok(value) => Some(value),
                Err(err) => {
                    warn!(error = ?err, "account balance fetch failed; proceeding without equity");
                    None
                }
            }
        } else {
            None
        };

        if let Some(writer) = &self.price_writer {
            let record = PriceBarRecord {
                timestamp: snapshot.timestamp,
                eth_mid: bars_snapshot.eth_bar.mid,
                eth_mark: bars_snapshot.eth_bar.mark,
                eth_close: bars_snapshot.eth_bar.close,
                btc_mid: bars_snapshot.btc_bar.mid,
                btc_mark: bars_snapshot.btc_bar.mark,
                btc_close: bars_snapshot.btc_bar.close,
                funding_eth: funding.as_ref().map(|value| value.eth.rate),
                funding_btc: funding.as_ref().map(|value| value.btc.rate),
                funding_interval_hours: funding.as_ref().map(|value| value.interval_hours),
            };
            if let Err(err) = writer.write(&record) {
                warn!(error = ?err, "price record write failed");
            }
        }

        let bar = StrategyBar {
            timestamp: snapshot.timestamp,
            eth_price: snapshot.eth,
            btc_price: snapshot.btc,
            equity,
            funding_eth: funding.as_ref().map(|value| value.eth.rate),
            funding_btc: funding.as_ref().map(|value| value.btc.rate),
            funding_interval_hours: funding.as_ref().map(|value| value.interval_hours),
        };

        let outcome = self.engine.process_bar(bar).await?;
        if let Some(writer) = &self.state_writer {
            writer.save(self.engine.state().state()).await?;
        }
        if let Some(writer) = &self.stats_writer {
            if let Err(err) = writer.write(&outcome.bar_log) {
                warn!(error = ?err, "stats log write failed");
            }
        }
        if let Some(writer) = &self.trade_writer {
            for log in &outcome.trade_logs {
                if let Err(err) = writer.write(log) {
                    warn!(error = ?err, "trade log write failed");
                }
            }
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
