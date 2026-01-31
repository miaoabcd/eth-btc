use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use async_trait::async_trait;
use rust_decimal_macros::dec;
use tokio::sync::watch;

use eth_btc_strategy::config::{Config, SigmaFloorMode, Symbol};
use eth_btc_strategy::data::{DataError, MockPriceSource, PriceBar, PriceFetcher};
use eth_btc_strategy::execution::{ExecutionEngine, PaperOrderExecutor, RetryConfig};
use eth_btc_strategy::funding::{FundingFetcher, FundingRate, MockFundingSource};
use eth_btc_strategy::runtime::{LiveRunner, RunnerError, StateWriter};
use eth_btc_strategy::state::StrategyState;

#[derive(Default)]
struct MockStateWriter {
    saved: std::sync::Mutex<Option<StrategyState>>,
}

impl MockStateWriter {
    fn saved_state(&self) -> Option<StrategyState> {
        self.saved.lock().expect("state lock").clone()
    }
}

#[async_trait]
impl StateWriter for MockStateWriter {
    async fn save(
        &self,
        state: &StrategyState,
    ) -> Result<(), eth_btc_strategy::state::StateError> {
        *self.saved.lock().expect("state lock") = Some(state.clone());
        Ok(())
    }
}

fn runner_with_mocks(timestamp: chrono::DateTime<Utc>) -> LiveRunner {
    let mut config = Config::default();
    config.strategy.n_z = 10;
    config.position.n_vol = 10;
    config.sigma_floor.mode = SigmaFloorMode::Const;

    let mut price_source = MockPriceSource::default();
    price_source.insert_bar(PriceBar::new(
        Symbol::EthPerp,
        timestamp,
        Some(dec!(2000)),
        None,
        None,
    ));
    price_source.insert_bar(PriceBar::new(
        Symbol::BtcPerp,
        timestamp,
        Some(dec!(30000)),
        None,
        None,
    ));

    let mut funding_source = MockFundingSource::default();
    funding_source.insert_rate(FundingRate {
        symbol: Symbol::EthPerp,
        rate: dec!(0.0001),
        timestamp,
        interval_hours: 8,
    });
    funding_source.insert_rate(FundingRate {
        symbol: Symbol::BtcPerp,
        rate: dec!(0.0002),
        timestamp,
        interval_hours: 8,
    });

    let execution = ExecutionEngine::new(Arc::new(PaperOrderExecutor), RetryConfig::fast());
    let engine = eth_btc_strategy::core::strategy::StrategyEngine::new(config.clone(), execution)
        .expect("engine");

    let price_fetcher = PriceFetcher::new(Arc::new(price_source), config.data.price_field);
    let funding_fetcher = FundingFetcher::new(Arc::new(funding_source));

    LiveRunner::new(engine, price_fetcher, Some(funding_fetcher))
}

#[tokio::test]
async fn runner_builds_bar_with_funding() {
    let timestamp = Utc.timestamp_opt(0, 0).unwrap();
    let mut runner = runner_with_mocks(timestamp);

    let outcome = runner.run_once_at(timestamp).await.unwrap();

    assert_eq!(outcome.bar_log.eth_price, Some(dec!(2000)));
    assert_eq!(outcome.bar_log.btc_price, Some(dec!(30000)));
    assert_eq!(outcome.bar_log.funding_eth, Some(dec!(0.0001)));
    assert_eq!(outcome.bar_log.funding_btc, Some(dec!(0.0002)));
}

#[tokio::test]
async fn runner_stops_on_shutdown_signal() {
    let timestamp = Utc.timestamp_opt(0, 0).unwrap();
    let runner = runner_with_mocks(timestamp);
    let now = Arc::new(move || timestamp);
    let mut runner = runner.with_clock(now);

    let (tx, rx) = watch::channel(false);
    let handle = tokio::spawn(async move {
        runner
            .run_loop(Duration::from_millis(10), rx)
            .await
            .unwrap();
    });

    tokio::time::sleep(Duration::from_millis(15)).await;
    tx.send(true).unwrap();

    handle.await.unwrap();
}

#[tokio::test]
async fn runner_persists_state_after_bar() {
    let timestamp = Utc.timestamp_opt(0, 0).unwrap();
    let mut runner = runner_with_mocks(timestamp);
    let writer = Arc::new(MockStateWriter::default());

    runner = runner.with_state_writer(writer.clone());

    runner.run_once_at(timestamp).await.unwrap();

    assert!(writer.saved_state().is_some());
}

#[tokio::test]
async fn runner_surfaces_data_errors() {
    let timestamp = Utc.timestamp_opt(0, 0).unwrap();
    let config = Config::default();
    let execution = ExecutionEngine::new(Arc::new(PaperOrderExecutor), RetryConfig::fast());
    let engine = eth_btc_strategy::core::strategy::StrategyEngine::new(config.clone(), execution)
        .expect("engine");
    let price_fetcher = PriceFetcher::new(Arc::new(MockPriceSource::default()), config.data.price_field);
    let mut runner = LiveRunner::new(engine, price_fetcher, None);

    let err = runner.run_once_at(timestamp).await.unwrap_err();
    match err {
        RunnerError::Data(DataError::MissingData(_)) => {}
        other => panic!("unexpected error: {other:?}"),
    }
}
