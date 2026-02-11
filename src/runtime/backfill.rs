use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use thiserror::Error;

use crate::config::{PriceField, Symbol};
use crate::data::{DataError, PriceSource, align_to_bar_close};
use crate::storage::{PriceBarRecord, PriceStore, PriceStoreError};

#[derive(Debug, Error)]
pub enum BackfillError {
    #[error("data error: {0}")]
    Data(#[from] DataError),
    #[error("storage error: {0}")]
    Storage(#[from] PriceStoreError),
    #[error("missing bar for {symbol:?} at {timestamp}")]
    MissingBar {
        symbol: Symbol,
        timestamp: DateTime<Utc>,
    },
}

pub async fn ensure_price_history(
    source: &dyn PriceSource,
    db_path: &str,
    price_field: PriceField,
    bars_needed: usize,
    now: DateTime<Utc>,
) -> Result<(), BackfillError> {
    if bars_needed == 0 {
        return Ok(());
    }
    let end = align_to_bar_close(now)?;
    let span_secs = 900 * (bars_needed as i64 - 1);
    let start = end - Duration::seconds(span_secs);

    let store = PriceStore::new(db_path)?;
    let existing = store.load_range(start, end)?;
    if existing.len() >= bars_needed {
        return Ok(());
    }

    let eth_history = source.fetch_history(Symbol::EthPerp, start, end).await?;
    let btc_history = source.fetch_history(Symbol::BtcPerp, start, end).await?;

    let eth_map: HashMap<_, _> = eth_history
        .into_iter()
        .map(|bar| (bar.timestamp, bar))
        .collect();
    let btc_map: HashMap<_, _> = btc_history
        .into_iter()
        .map(|bar| (bar.timestamp, bar))
        .collect();

    for idx in 0..bars_needed {
        let ts = start + Duration::seconds(900 * idx as i64);
        let eth_bar = eth_map.get(&ts).ok_or(BackfillError::MissingBar {
            symbol: Symbol::EthPerp,
            timestamp: ts,
        })?;
        let btc_bar = btc_map.get(&ts).ok_or(BackfillError::MissingBar {
            symbol: Symbol::BtcPerp,
            timestamp: ts,
        })?;
        let eth_price = effective_price(price_field, eth_bar.mid, eth_bar.mark, eth_bar.close)
            .ok_or(BackfillError::MissingBar {
                symbol: Symbol::EthPerp,
                timestamp: ts,
            })?;
        let btc_price = effective_price(price_field, btc_bar.mid, btc_bar.mark, btc_bar.close)
            .ok_or(BackfillError::MissingBar {
                symbol: Symbol::BtcPerp,
                timestamp: ts,
            })?;
        let record = PriceBarRecord {
            timestamp: ts,
            eth_mid: eth_bar.mid.or(Some(eth_price)),
            eth_mark: eth_bar.mark,
            eth_close: eth_bar.close,
            btc_mid: btc_bar.mid.or(Some(btc_price)),
            btc_mark: btc_bar.mark,
            btc_close: btc_bar.close,
            funding_eth: None,
            funding_btc: None,
            funding_interval_hours: None,
        };
        store.save(&record)?;
    }

    Ok(())
}

fn effective_price(
    field: PriceField,
    mid: Option<Decimal>,
    mark: Option<Decimal>,
    close: Option<Decimal>,
) -> Option<Decimal> {
    match field {
        PriceField::Mid => mid.or(mark).or(close),
        PriceField::Mark => mark.or(mid).or(close),
        PriceField::Close => close.or(mid).or(mark),
    }
}
