use std::io;
use std::str::FromStr;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rusqlite::Connection;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct PriceBarRecord {
    pub timestamp: DateTime<Utc>,
    pub eth_mid: Option<Decimal>,
    pub eth_mark: Option<Decimal>,
    pub eth_close: Option<Decimal>,
    pub btc_mid: Option<Decimal>,
    pub btc_mark: Option<Decimal>,
    pub btc_close: Option<Decimal>,
    pub funding_eth: Option<Decimal>,
    pub funding_btc: Option<Decimal>,
    pub funding_interval_hours: Option<u32>,
}

#[derive(Debug, Error)]
pub enum PriceStoreError {
    #[error("persistence error: {0}")]
    Persistence(String),
    #[error("parse error: {0}")]
    Parse(String),
}

pub trait PriceBarWriter: Send + Sync {
    fn write(&self, record: &PriceBarRecord) -> Result<(), io::Error>;
}

pub struct PriceStore {
    conn: Connection,
}

impl PriceStore {
    pub fn new(path: &str) -> Result<Self, PriceStoreError> {
        let conn =
            Connection::open(path).map_err(|err| PriceStoreError::Persistence(err.to_string()))?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    pub fn new_in_memory() -> Result<Self, PriceStoreError> {
        let conn = Connection::open_in_memory()
            .map_err(|err| PriceStoreError::Persistence(err.to_string()))?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), PriceStoreError> {
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS price_bars (\n                    timestamp TEXT PRIMARY KEY,\n                    eth_mid TEXT,\n                    eth_mark TEXT,\n                    eth_close TEXT,\n                    btc_mid TEXT,\n                    btc_mark TEXT,\n                    btc_close TEXT,\n                    funding_eth TEXT,\n                    funding_btc TEXT,\n                    funding_interval_hours INTEGER,\n                    created_at TEXT NOT NULL\n                )",
                [],
            )
            .map_err(|err| PriceStoreError::Persistence(err.to_string()))?;
        Ok(())
    }

    pub fn save(&self, record: &PriceBarRecord) -> Result<(), PriceStoreError> {
        let timestamp = record.timestamp.to_rfc3339();
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO price_bars (\n                    timestamp,\n                    eth_mid,\n                    eth_mark,\n                    eth_close,\n                    btc_mid,\n                    btc_mark,\n                    btc_close,\n                    funding_eth,\n                    funding_btc,\n                    funding_interval_hours,\n                    created_at\n                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    timestamp,
                    Self::decimal_to_string(record.eth_mid),
                    Self::decimal_to_string(record.eth_mark),
                    Self::decimal_to_string(record.eth_close),
                    Self::decimal_to_string(record.btc_mid),
                    Self::decimal_to_string(record.btc_mark),
                    Self::decimal_to_string(record.btc_close),
                    Self::decimal_to_string(record.funding_eth),
                    Self::decimal_to_string(record.funding_btc),
                    record
                        .funding_interval_hours
                        .map(|value| value as i64),
                    now,
                ],
            )
            .map_err(|err| PriceStoreError::Persistence(err.to_string()))?;
        Ok(())
    }

    pub fn load(&self, timestamp: DateTime<Utc>) -> Result<Option<PriceBarRecord>, PriceStoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT timestamp, eth_mid, eth_mark, eth_close, btc_mid, btc_mark, btc_close, funding_eth, funding_btc, funding_interval_hours FROM price_bars WHERE timestamp = ?",
            )
            .map_err(|err| PriceStoreError::Persistence(err.to_string()))?;
        let mut rows = stmt
            .query([timestamp.to_rfc3339()])
            .map_err(|err| PriceStoreError::Persistence(err.to_string()))?;
        let row = match rows
            .next()
            .map_err(|err| PriceStoreError::Persistence(err.to_string()))?
        {
            Some(row) => row,
            None => return Ok(None),
        };
        let timestamp: String = row
            .get(0)
            .map_err(|err| PriceStoreError::Persistence(err.to_string()))?;
        let timestamp = DateTime::parse_from_rfc3339(&timestamp)
            .map_err(|err| PriceStoreError::Parse(err.to_string()))?
            .with_timezone(&Utc);
        let eth_mid: Option<String> = row
            .get(1)
            .map_err(|err| PriceStoreError::Persistence(err.to_string()))?;
        let eth_mark: Option<String> = row
            .get(2)
            .map_err(|err| PriceStoreError::Persistence(err.to_string()))?;
        let eth_close: Option<String> = row
            .get(3)
            .map_err(|err| PriceStoreError::Persistence(err.to_string()))?;
        let btc_mid: Option<String> = row
            .get(4)
            .map_err(|err| PriceStoreError::Persistence(err.to_string()))?;
        let btc_mark: Option<String> = row
            .get(5)
            .map_err(|err| PriceStoreError::Persistence(err.to_string()))?;
        let btc_close: Option<String> = row
            .get(6)
            .map_err(|err| PriceStoreError::Persistence(err.to_string()))?;
        let funding_eth: Option<String> = row
            .get(7)
            .map_err(|err| PriceStoreError::Persistence(err.to_string()))?;
        let funding_btc: Option<String> = row
            .get(8)
            .map_err(|err| PriceStoreError::Persistence(err.to_string()))?;
        let funding_interval_hours: Option<i64> = row
            .get(9)
            .map_err(|err| PriceStoreError::Persistence(err.to_string()))?;

        Ok(Some(PriceBarRecord {
            timestamp,
            eth_mid: Self::string_to_decimal(eth_mid)?,
            eth_mark: Self::string_to_decimal(eth_mark)?,
            eth_close: Self::string_to_decimal(eth_close)?,
            btc_mid: Self::string_to_decimal(btc_mid)?,
            btc_mark: Self::string_to_decimal(btc_mark)?,
            btc_close: Self::string_to_decimal(btc_close)?,
            funding_eth: Self::string_to_decimal(funding_eth)?,
            funding_btc: Self::string_to_decimal(funding_btc)?,
            funding_interval_hours: funding_interval_hours.map(|value| value as u32),
        }))
    }

    fn decimal_to_string(value: Option<Decimal>) -> Option<String> {
        value.map(|value| value.to_string())
    }

    fn string_to_decimal(value: Option<String>) -> Result<Option<Decimal>, PriceStoreError> {
        match value {
            Some(value) => Decimal::from_str(&value)
                .map(Some)
                .map_err(|err| PriceStoreError::Parse(err.to_string())),
            None => Ok(None),
        }
    }
}

pub struct PriceStoreWriter {
    store: Mutex<PriceStore>,
}

impl PriceStoreWriter {
    pub fn new(store: PriceStore) -> Self {
        Self {
            store: Mutex::new(store),
        }
    }
}

impl PriceBarWriter for PriceStoreWriter {
    fn write(&self, record: &PriceBarRecord) -> Result<(), io::Error> {
        let store = self.store.lock().expect("price store lock");
        store
            .save(record)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))
    }
}
