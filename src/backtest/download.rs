use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use thiserror::Error;

use crate::backtest::BacktestBar;
use crate::config::Symbol;
use crate::data::{
    DataError, HttpClient, HyperliquidPriceSource, PriceBar, PriceSource, align_to_bar_close,
};

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("data error: {0}")]
    Data(#[from] DataError),
    #[error("missing price for {0}")]
    MissingPrice(String),
    #[error("history coverage incomplete: start {start} end {end} first {first:?} last {last:?}")]
    Coverage {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        first: Option<DateTime<Utc>>,
        last: Option<DateTime<Utc>>,
    },
}

#[derive(Clone)]
pub struct HyperliquidDownloader {
    source: HyperliquidPriceSource,
}

impl HyperliquidDownloader {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            source: HyperliquidPriceSource::new(base_url),
        }
    }

    pub fn with_client(base_url: impl Into<String>, http: Arc<dyn HttpClient>) -> Self {
        Self {
            source: HyperliquidPriceSource::with_client(base_url, http),
        }
    }

    pub async fn fetch_backtest_bars(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<BacktestBar>, DownloadError> {
        let start = align_to_bar_close(start)?;
        let end = align_to_bar_close(end)?;
        let (eth_bars, btc_bars) = tokio::try_join!(
            self.source.fetch_history(Symbol::EthPerp, start, end),
            self.source.fetch_history(Symbol::BtcPerp, start, end),
        )?;

        let eth_map = Self::map_prices(eth_bars)?;
        let btc_map = Self::map_prices(btc_bars)?;

        let mut merged = Vec::new();
        for (timestamp, eth_price) in eth_map {
            if let Some(btc_price) = btc_map.get(&timestamp) {
                merged.push(BacktestBar {
                    timestamp,
                    eth_price,
                    btc_price: *btc_price,
                    funding_eth: None,
                    funding_btc: None,
                });
            }
        }
        let first = merged.first().map(|bar| bar.timestamp);
        let last = merged.last().map(|bar| bar.timestamp);
        if first.map(|ts| ts > start).unwrap_or(true) || last.map(|ts| ts < end).unwrap_or(true) {
            return Err(DownloadError::Coverage {
                start,
                end,
                first,
                last,
            });
        }
        Ok(merged)
    }

    fn map_prices(bars: Vec<PriceBar>) -> Result<BTreeMap<DateTime<Utc>, Decimal>, DownloadError> {
        let mut map = BTreeMap::new();
        for bar in bars {
            let price = bar
                .close
                .or(bar.mid)
                .or(bar.mark)
                .ok_or_else(|| {
                    DownloadError::MissingPrice(format!(
                        "{:?} at {}",
                        bar.symbol,
                        bar.timestamp.to_rfc3339()
                    ))
                })?;
            map.insert(bar.timestamp, price);
        }
        Ok(map)
    }
}
