use std::collections::HashMap;
use std::sync::Arc;

use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::json;

use eth_btc_strategy::backtest::BacktestBar;
use eth_btc_strategy::backtest::download::{
    DownloadError, HyperliquidDownloader, write_bars_to_output,
};
use eth_btc_strategy::data::{DataError, HttpClient, HttpResponse};
use eth_btc_strategy::storage::PriceStore;

#[derive(Clone)]
struct MockHttpClient {
    responses: Arc<HashMap<String, String>>,
}

impl MockHttpClient {
    fn new(responses: HashMap<String, String>) -> Self {
        Self {
            responses: Arc::new(responses),
        }
    }
}

#[async_trait::async_trait]
impl HttpClient for MockHttpClient {
    async fn post(&self, _url: &str, body: serde_json::Value) -> Result<HttpResponse, DataError> {
        let coin = body
            .get("req")
            .and_then(|req| req.get("coin"))
            .and_then(|value| value.as_str())
            .ok_or_else(|| DataError::MissingData("missing coin".to_string()))?;
        let payload = self
            .responses
            .get(coin)
            .ok_or_else(|| DataError::MissingData("missing response".to_string()))?;
        Ok(HttpResponse {
            status: 200,
            body: payload.clone(),
        })
    }
}

fn candle_payload(candles: &[(i64, Decimal)]) -> String {
    let payload: Vec<_> = candles
        .iter()
        .map(|(ts, price)| json!({ "t": ts, "c": price.to_string() }))
        .collect();
    serde_json::to_string(&payload).expect("serialize payload")
}

#[tokio::test]
async fn download_merges_eth_and_btc_candles() {
    let ts1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 15, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 30, 0).unwrap();

    let eth_payload = candle_payload(&[
        (ts1.timestamp_millis(), dec!(2300)),
        (ts2.timestamp_millis(), dec!(2310)),
    ]);
    let btc_payload = candle_payload(&[
        (ts1.timestamp_millis(), dec!(42000)),
        (ts2.timestamp_millis(), dec!(42100)),
    ]);

    let mut responses = HashMap::new();
    responses.insert("ETH".to_string(), eth_payload);
    responses.insert("BTC".to_string(), btc_payload);

    let http = Arc::new(MockHttpClient::new(responses));
    let downloader = HyperliquidDownloader::with_client("http://localhost", http);

    let bars = downloader.fetch_backtest_bars(ts1, ts2).await.unwrap();

    assert_eq!(bars.len(), 2);
    assert_eq!(bars[0].timestamp, ts1);
    assert_eq!(bars[0].eth_price, dec!(2300));
    assert_eq!(bars[0].btc_price, dec!(42000));
    assert!(bars[0].funding_eth.is_none());
    assert!(bars[0].funding_btc.is_none());
    assert_eq!(bars[1].timestamp, ts2);
    assert_eq!(bars[1].eth_price, dec!(2310));
    assert_eq!(bars[1].btc_price, dec!(42100));
}

#[tokio::test]
async fn download_errors_on_incomplete_coverage() {
    let ts1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 15, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 30, 0).unwrap();

    let eth_payload = candle_payload(&[(ts2.timestamp_millis(), dec!(2300))]);
    let btc_payload = candle_payload(&[(ts2.timestamp_millis(), dec!(42000))]);

    let mut responses = HashMap::new();
    responses.insert("ETH".to_string(), eth_payload);
    responses.insert("BTC".to_string(), btc_payload);

    let http = Arc::new(MockHttpClient::new(responses));
    let downloader = HyperliquidDownloader::with_client("http://localhost", http);

    let err = downloader.fetch_backtest_bars(ts1, ts2).await.unwrap_err();
    assert!(matches!(err, DownloadError::Coverage { .. }));
}

#[tokio::test]
async fn download_writes_json_output() {
    let ts1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 15, 0).unwrap();
    let bars = vec![BacktestBar {
        timestamp: ts1,
        eth_price: dec!(2300),
        btc_price: dec!(42000),
        funding_eth: None,
        funding_btc: None,
    }];

    let path = format!("/tmp/bars-{}.json", ts1.timestamp());
    write_bars_to_output(&bars, std::path::Path::new(&path)).unwrap();

    let contents = std::fs::read_to_string(&path).unwrap();
    let decoded: Vec<BacktestBar> = serde_json::from_str(&contents).unwrap();
    assert_eq!(decoded, bars);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn download_writes_sqlite_output() {
    let ts1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 15, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 30, 0).unwrap();
    let bars = vec![
        BacktestBar {
            timestamp: ts1,
            eth_price: dec!(2300),
            btc_price: dec!(42000),
            funding_eth: Some(dec!(0.0001)),
            funding_btc: Some(dec!(0.0002)),
        },
        BacktestBar {
            timestamp: ts2,
            eth_price: dec!(2310),
            btc_price: dec!(42100),
            funding_eth: None,
            funding_btc: None,
        },
    ];

    let path = format!("/tmp/bars-{}.sqlite", ts1.timestamp());
    write_bars_to_output(&bars, std::path::Path::new(&path)).unwrap();

    let store = PriceStore::new(&path).unwrap();
    let records = store.load_range(ts1, ts2).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].eth_mid, Some(dec!(2300)));
    assert_eq!(records[0].btc_mid, Some(dec!(42000)));
    assert_eq!(records[0].funding_eth, Some(dec!(0.0001)));
    assert_eq!(records[0].funding_btc, Some(dec!(0.0002)));
    assert_eq!(records[1].timestamp, ts2);
    let _ = std::fs::remove_file(path);
}
