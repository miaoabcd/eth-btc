use std::sync::Arc;

use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;
use serde_json::json;

use eth_btc_strategy::config::{PriceField, Symbol};
use eth_btc_strategy::data::{
    DataError, HttpClient, HttpResponse, MockPriceSource, PriceBar, PriceFetcher, PriceSource,
    VariationalPriceSource, align_to_bar_close,
};

#[derive(Debug, Clone)]
struct TestHttpClient {
    expected_url: String,
    expected_query: Vec<(String, String)>,
    response: HttpResponse,
}

#[async_trait::async_trait]
impl HttpClient for TestHttpClient {
    async fn get(&self, url: &str, query: &[(&str, String)]) -> Result<HttpResponse, DataError> {
        assert_eq!(url, self.expected_url);
        let mut actual = query
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect::<Vec<_>>();
        let mut expected = self.expected_query.clone();
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected);
        Ok(self.response.clone())
    }
}

#[derive(Debug, Clone)]
struct MismatchSource {
    eth: PriceBar,
    btc: PriceBar,
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
    let aligned = align_to_bar_close(timestamp);
    assert_eq!(aligned, Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap());

    let exact = Utc.with_ymd_and_hms(2024, 1, 1, 12, 15, 0).unwrap();
    let aligned_exact = align_to_bar_close(exact);
    assert_eq!(aligned_exact, exact);
}

#[tokio::test]
async fn variational_price_source_fetch_bar_parses_response() {
    let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 0, 15, 0).unwrap();
    let body = json!({
        "symbol": "ETH-PERP",
        "bars": [
            {
                "timestamp": timestamp.to_rfc3339(),
                "mid": "100.0",
                "mark": "100.1",
                "close": "99.9"
            }
        ]
    })
    .to_string();

    let client = TestHttpClient {
        expected_url: "http://localhost/v1/marketdata/bars".to_string(),
        expected_query: vec![
            ("symbol".to_string(), "ETH-PERP".to_string()),
            ("start".to_string(), timestamp.to_rfc3339()),
            ("end".to_string(), timestamp.to_rfc3339()),
            ("interval".to_string(), "15m".to_string()),
        ],
        response: HttpResponse { status: 200, body },
    };

    let source =
        VariationalPriceSource::with_client("http://localhost".to_string(), Arc::new(client));
    let bar = source.fetch_bar(Symbol::EthPerp, timestamp).await.unwrap();

    assert_eq!(bar.symbol, Symbol::EthPerp);
    assert_eq!(bar.timestamp, timestamp);
    assert_eq!(bar.mid, Some(dec!(100.0)));
    assert_eq!(bar.mark, Some(dec!(100.1)));
    assert_eq!(bar.close, Some(dec!(99.9)));
}

#[tokio::test]
async fn variational_price_source_handles_rate_limits() {
    let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 0, 15, 0).unwrap();
    let client = TestHttpClient {
        expected_url: "http://localhost/v1/marketdata/bars".to_string(),
        expected_query: vec![
            ("symbol".to_string(), "ETH-PERP".to_string()),
            ("start".to_string(), timestamp.to_rfc3339()),
            ("end".to_string(), timestamp.to_rfc3339()),
            ("interval".to_string(), "15m".to_string()),
        ],
        response: HttpResponse {
            status: 429,
            body: String::new(),
        },
    };

    let source =
        VariationalPriceSource::with_client("http://localhost".to_string(), Arc::new(client));
    let err = source
        .fetch_bar(Symbol::EthPerp, timestamp)
        .await
        .unwrap_err();
    assert!(matches!(err, DataError::RateLimited));
}

#[tokio::test]
async fn variational_price_source_handles_missing_data() {
    let timestamp = Utc.with_ymd_and_hms(2024, 1, 1, 0, 15, 0).unwrap();
    let body = json!({
        "symbol": "ETH-PERP",
        "bars": []
    })
    .to_string();
    let client = TestHttpClient {
        expected_url: "http://localhost/v1/marketdata/bars".to_string(),
        expected_query: vec![
            ("symbol".to_string(), "ETH-PERP".to_string()),
            ("start".to_string(), timestamp.to_rfc3339()),
            ("end".to_string(), timestamp.to_rfc3339()),
            ("interval".to_string(), "15m".to_string()),
        ],
        response: HttpResponse { status: 200, body },
    };

    let source =
        VariationalPriceSource::with_client("http://localhost".to_string(), Arc::new(client));
    let err = source
        .fetch_bar(Symbol::EthPerp, timestamp)
        .await
        .unwrap_err();
    assert!(matches!(err, DataError::MissingData(_)));
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
