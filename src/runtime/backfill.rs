use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use thiserror::Error;

use crate::config::{PriceField, Symbol};
use crate::data::{DataError, PriceSource, align_to_bar_close};
use crate::storage::{PriceBarRecord, PriceStore, PriceStoreError};

const BAR_SECS: i64 = 900;

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

pub fn latest_completed_bar(now: DateTime<Utc>) -> Result<DateTime<Utc>, DataError> {
    let aligned = align_to_bar_close(now)?;
    Ok(aligned - Duration::seconds(BAR_SECS))
}

pub fn replay_warmup_gap_window(
    warmed_run_bar: DateTime<Utc>,
    target_run_bar: DateTime<Utc>,
) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    if target_run_bar <= warmed_run_bar {
        return None;
    }
    let start = warmed_run_bar + Duration::seconds(BAR_SECS);
    let end = target_run_bar - Duration::seconds(BAR_SECS);
    if start <= end {
        Some((start, end))
    } else {
        None
    }
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
    let end = latest_completed_bar(now)?;
    let span_secs = BAR_SECS * (bars_needed as i64 - 1);
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
        let ts = start + Duration::seconds(BAR_SECS * idx as i64);
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

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::TimeZone;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    use super::*;
    use crate::data::{MockPriceSource, PriceBar};

    fn ts(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, s)
            .single()
            .expect("valid timestamp")
    }

    #[test]
    fn latest_completed_bar_returns_previous_bar_for_mid_interval_time() {
        let now = ts(2026, 2, 17, 2, 21, 33);
        let got = latest_completed_bar(now).expect("aligned timestamp");
        assert_eq!(got, ts(2026, 2, 17, 2, 0, 0));
    }

    #[test]
    fn latest_completed_bar_steps_back_on_bar_boundary() {
        let now = ts(2026, 2, 17, 2, 30, 0);
        let got = latest_completed_bar(now).expect("aligned timestamp");
        assert_eq!(got, ts(2026, 2, 17, 2, 15, 0));
    }

    #[tokio::test]
    async fn ensure_price_history_uses_completed_bar_window() {
        let now = ts(2026, 2, 17, 10, 7, 0);
        let start = ts(2026, 2, 17, 9, 15, 0);
        let middle = ts(2026, 2, 17, 9, 30, 0);
        let end = ts(2026, 2, 17, 9, 45, 0);

        let mut source = MockPriceSource::default();
        source.insert_history(
            Symbol::EthPerp,
            vec![
                PriceBar::new(Symbol::EthPerp, start, Some(dec!(2000)), None, None),
                PriceBar::new(Symbol::EthPerp, middle, Some(dec!(2001)), None, None),
                PriceBar::new(Symbol::EthPerp, end, Some(dec!(2002)), None, None),
            ],
        );
        source.insert_history(
            Symbol::BtcPerp,
            vec![
                PriceBar::new(Symbol::BtcPerp, start, Some(dec!(30000)), None, None),
                PriceBar::new(Symbol::BtcPerp, middle, Some(dec!(30010)), None, None),
                PriceBar::new(Symbol::BtcPerp, end, Some(dec!(30020)), None, None),
            ],
        );

        let db_path = format!("/tmp/eth_btc_backfill_{}.sqlite", Uuid::new_v4());
        let result = ensure_price_history(&source, &db_path, PriceField::Mid, 3, now).await;
        assert!(result.is_ok(), "unexpected error: {result:?}");

        let store = PriceStore::new(&db_path).expect("open price store");
        let records = store.load_range(start, end).expect("load persisted range");
        assert_eq!(records.len(), 3);
        assert_eq!(records.last().expect("last record").timestamp, end);

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn replay_warmup_gap_window_returns_missing_middle_range() {
        let warm = ts(2026, 2, 17, 2, 15, 0);
        let target = ts(2026, 2, 17, 2, 45, 0);
        let gap = replay_warmup_gap_window(warm, target).expect("gap exists");
        assert_eq!(gap.0, ts(2026, 2, 17, 2, 30, 0));
        assert_eq!(gap.1, ts(2026, 2, 17, 2, 30, 0));
    }

    #[test]
    fn replay_warmup_gap_window_is_none_for_adjacent_bars() {
        let warm = ts(2026, 2, 17, 2, 15, 0);
        let target = ts(2026, 2, 17, 2, 30, 0);
        assert_eq!(replay_warmup_gap_window(warm, target), None);
    }
}
