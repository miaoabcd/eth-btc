use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;
use serde_json::json;

use eth_btc_strategy::config::{PriceField, Symbol};
use eth_btc_strategy::data::{
    DataError, HttpClient, HttpResponse, HyperliquidPriceSource, MockPriceSource, PriceBar,
    PriceFetcher, PriceSource, align_to_bar_close,
};
use eth_btc_strategy::util::rate_limiter::RateLimiter;

#[derive(Debug, Clone)]
struct TestHttpClient {
    expected_url: String,
    expected_body: serde_json::Value,
    response: HttpResponse,
}

#[async_trait::async_trait]
impl HttpClient for TestHttpClient {
    async fn post(&self, url: &str, body: serde_json::Value) -> Result<HttpResponse, DataError> {
        assert_eq!(url, self.expected_url);
        assert_eq!(body, self.expected_body);
        Ok(self.response.clone())
    }
}

#[derive(Debug, Clone)]
struct SequencedHttpClient {
    expected_url: String,
    expected_bodies: Vec<serde_json::Value>,
    responses: Vec<HttpResponse>,
    calls: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl HttpClient for SequencedHttpClient {
    async fn post(&self, url: &str, body: serde_json::Value) -> Result<HttpResponse, DataError> {
        let index = self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(url, self.expected_url);
        assert!(index < self.expected_bodies.len());
        assert_eq!(body, self.expected_bodies[index]);
        Ok(self.responses[index].clone())
    }
}

#[derive(Debug, Clone)]
struct MismatchSource {
    eth: PriceBar,
    btc: PriceBar,
}

#[derive(Default)]
struct CountingRateLimiter {
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl RateLimiter for CountingRateLimiter {
    async fn wait(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl PriceSource for MismatchSource {
    async fn fetch_bar(
        &self,
        symbol: Symbol,
        _timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<PriceBar, DataError> {
        match symbol {
            Symbol::EthPerp => Ok(self.eth.clone()),
            Symbol::BtcPerp => Ok(self.btc.clone()),
        }
    }

    async fn fetch_history(
        &self,
        _symbol: Symbol,
        _start: chrono::DateTime<chrono::Utc>,
        _end: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<PriceBar>, DataError> {
        Ok(Vec::new())
    }
}

#[test]
fn aligns_to_bar_close_on_15m_boundary() {
    let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 12, 7, 30).unwrap();
    let aligned = align_to_bar_close(timestamp).expect("aligned timestamp");
    assert_eq!(aligned, Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap());

    let exact = Utc.with_ymd_and_hms(2024, 1, 1, 12, 15, 0).unwrap();
    let aligned_exact = align_to_bar_close(exact).expect("aligned timestamp");
    assert_eq!(aligned_exact, exact);
}

#[tokio::test]
async fn hyperliquid_price_source_fetch_bar_parses_response() {
    let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 0, 15, 0).unwrap();
    let start_ms = timestamp.timestamp_millis();
    let end_ms = start_ms + 900_000;
    let body = json!([
        {
            "t": start_ms,
            "T": end_ms,
            "o": "100.0",
            "h": "101.0",
            "l": "99.0",
            "c": "99.9",
            "v": "10.0"
        }
    ])
    .to_string();

    let client = TestHttpClient {
        expected_url: "http://localhost/info".to_string(),
        expected_body: json!({
            "type": "candleSnapshot",
            "req": {
                "coin": "ETH",
                "interval": "15m",
                "startTime": start_ms,
                "endTime": end_ms,
            }
        }),
        response: HttpResponse { status: 200, body },
    };

    let source =
        HyperliquidPriceSource::with_client("http://localhost".to_string(), Arc::new(client));
    let bar = source.fetch_bar(Symbol::EthPerp, timestamp).await.unwrap();

    assert_eq!(bar.symbol, Symbol::EthPerp);
    assert_eq!(bar.timestamp, timestamp);
    assert_eq!(bar.mid, Some(dec!(99.9)));
    assert_eq!(bar.mark, Some(dec!(99.9)));
    assert_eq!(bar.close, Some(dec!(99.9)));
}

#[tokio::test]
async fn hyperliquid_price_source_handles_rate_limits() {
    let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 0, 15, 0).unwrap();
    let start_ms = timestamp.timestamp_millis();
    let end_ms = start_ms + 900_000;
    let client = TestHttpClient {
        expected_url: "http://localhost/info".to_string(),
        expected_body: json!({
            "type": "candleSnapshot",
            "req": {
                "coin": "ETH",
                "interval": "15m",
                "startTime": start_ms,
                "endTime": end_ms,
            }
        }),
        response: HttpResponse {
            status: 429,
            body: String::new(),
        },
    };

    let source =
        HyperliquidPriceSource::with_client("http://localhost".to_string(), Arc::new(client));
    let err = source
        .fetch_bar(Symbol::EthPerp, timestamp)
        .await
        .unwrap_err();
    assert!(matches!(err, DataError::RateLimited));
}

#[tokio::test]
async fn hyperliquid_price_source_applies_rate_limiter() {
    let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 0, 15, 0).unwrap();
    let start_ms = timestamp.timestamp_millis();
    let end_ms = start_ms + 900_000;
    let body = json!([
        {
            "t": start_ms,
            "T": end_ms,
            "o": "100.0",
            "h": "101.0",
            "l": "99.0",
            "c": "99.9",
            "v": "10.0"
        }
    ])
    .to_string();

    let client = TestHttpClient {
        expected_url: "http://localhost/info".to_string(),
        expected_body: json!({
            "type": "candleSnapshot",
            "req": {
                "coin": "ETH",
                "interval": "15m",
                "startTime": start_ms,
                "endTime": end_ms,
            }
        }),
        response: HttpResponse { status: 200, body },
    };

    let limiter = Arc::new(CountingRateLimiter::default());
    let source = HyperliquidPriceSource::with_client_and_rate_limiter(
        "http://localhost",
        Arc::new(client),
        limiter.clone(),
    );
    source.fetch_bar(Symbol::EthPerp, timestamp).await.unwrap();

    assert_eq!(limiter.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn hyperliquid_price_source_handles_missing_data() {
    let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 0, 15, 0).unwrap();
    let start_ms = timestamp.timestamp_millis();
    let end_ms = start_ms + 900_000;
    let body = json!([]).to_string();
    let client = TestHttpClient {
        expected_url: "http://localhost/info".to_string(),
        expected_body: json!({
            "type": "candleSnapshot",
            "req": {
                "coin": "ETH",
                "interval": "15m",
                "startTime": start_ms,
                "endTime": end_ms,
            }
        }),
        response: HttpResponse { status: 200, body },
    };

    let source =
        HyperliquidPriceSource::with_client("http://localhost".to_string(), Arc::new(client));
    let err = source
        .fetch_bar(Symbol::EthPerp, timestamp)
        .await
        .unwrap_err();
    assert!(matches!(err, DataError::MissingData(_)));
}

#[tokio::test]
async fn hyperliquid_price_source_fetch_history_paginates() {
    const INTERVAL_MS: i64 = 900_000;
    const MAX_BARS: i64 = 5_000;

    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let end = start + chrono::Duration::minutes(15 * MAX_BARS);
    let start_ms = start.timestamp_millis();
    let end_ms = end.timestamp_millis();
    let first_end_ms = start_ms + INTERVAL_MS * MAX_BARS;

    let expected_bodies = vec![
        json!({
            "type": "candleSnapshot",
            "req": {
                "coin": "ETH",
                "interval": "15m",
                "startTime": start_ms,
                "endTime": first_end_ms,
            }
        }),
        json!({
            "type": "candleSnapshot",
            "req": {
                "coin": "ETH",
                "interval": "15m",
                "startTime": start_ms + INTERVAL_MS * MAX_BARS,
                "endTime": end_ms + INTERVAL_MS,
            }
        }),
    ];

    let responses = vec![
        HttpResponse {
            status: 200,
            body: json!([
                {
                    "t": start_ms,
                    "T": start_ms + INTERVAL_MS,
                    "o": "100.0",
                    "h": "101.0",
                    "l": "99.0",
                    "c": "100.0",
                    "v": "10.0"
                }
            ])
            .to_string(),
        },
        HttpResponse {
            status: 200,
            body: json!([
                {
                    "t": end_ms,
                    "T": end_ms + INTERVAL_MS,
                    "o": "110.0",
                    "h": "111.0",
                    "l": "109.0",
                    "c": "110.0",
                    "v": "11.0"
                }
            ])
            .to_string(),
        },
    ];

    let calls = Arc::new(AtomicUsize::new(0));
    let client = SequencedHttpClient {
        expected_url: "http://localhost/info".to_string(),
        expected_bodies,
        responses,
        calls: calls.clone(),
    };
    let limiter = Arc::new(CountingRateLimiter::default());
    let source = HyperliquidPriceSource::with_client_and_rate_limiter(
        "http://localhost",
        Arc::new(client),
        limiter.clone(),
    );

    let bars = source
        .fetch_history(Symbol::EthPerp, start, end)
        .await
        .unwrap();

    assert_eq!(bars.len(), 2);
    assert_eq!(bars[0].timestamp, start);
    assert_eq!(bars[1].timestamp, end);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(limiter.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn price_fetcher_resolves_prices_with_fallback() {
    let mut source = MockPriceSource::default();
    let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 1, 0, 0).unwrap();

    source.insert_bar(PriceBar::new(
        Symbol::EthPerp,
        timestamp,
        Some(dec!(100.0)),
        None,
        None,
    ));
    source.insert_bar(PriceBar::new(
        Symbol::BtcPerp,
        timestamp,
        Some(dec!(200.0)),
        Some(dec!(201.0)),
        None,
    ));

    let fetcher = PriceFetcher::new(Arc::new(source), PriceField::Mark);
    let snapshot = fetcher.fetch_pair_prices(timestamp).await.unwrap();

    assert_eq!(snapshot.eth, dec!(100.0));
    assert_eq!(snapshot.btc, dec!(201.0));
    assert_eq!(snapshot.field, PriceField::Mark);
}

#[tokio::test]
async fn price_fetcher_returns_missing_when_fields_unavailable() {
    let mut source = MockPriceSource::default();
    let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 2, 0, 0).unwrap();

    source.insert_bar(PriceBar::new(Symbol::EthPerp, timestamp, None, None, None));
    source.insert_bar(PriceBar::new(Symbol::BtcPerp, timestamp, None, None, None));

    let fetcher = PriceFetcher::new(Arc::new(source), PriceField::Mid);
    let err = fetcher.fetch_pair_prices(timestamp).await.unwrap_err();

    assert!(matches!(err, DataError::MissingData(_)));
}

#[tokio::test]
async fn price_fetcher_rejects_mismatched_timestamps() {
    let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 3, 0, 0).unwrap();
    let source = MismatchSource {
        eth: PriceBar::new(Symbol::EthPerp, timestamp, Some(dec!(100.0)), None, None),
        btc: PriceBar::new(
            Symbol::BtcPerp,
            Utc.with_ymd_and_hms(2024, 1, 1, 3, 15, 0).unwrap(),
            Some(dec!(200.0)),
            None,
            None,
        ),
    };

    let fetcher = PriceFetcher::new(Arc::new(source), PriceField::Mid);
    let err = fetcher.fetch_pair_prices(timestamp).await.unwrap_err();

    assert!(matches!(err, DataError::InconsistentData(_)));
}
