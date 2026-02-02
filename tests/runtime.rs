use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use async_trait::async_trait;
use rust_decimal_macros::dec;
use tokio::sync::watch;

use eth_btc_strategy::account::MockAccountSource;
use eth_btc_strategy::config::{CapitalMode, Config, SigmaFloorMode, Symbol};
use eth_btc_strategy::core::strategy::StrategyEngine;
use eth_btc_strategy::data::{DataError, MockPriceSource, PriceBar, PriceFetcher};
use eth_btc_strategy::execution::{ExecutionEngine, PaperOrderExecutor, RetryConfig};
use eth_btc_strategy::funding::{FundingFetcher, FundingRate, MockFundingSource};
use eth_btc_strategy::logging::{BarLogWriter, TradeLog, TradeLogWriter};
use eth_btc_strategy::runtime::{LiveRunner, RunnerError, StateWriter};
use eth_btc_strategy::storage::{PriceBarRecord, PriceBarWriter};
use eth_btc_strategy::state::StrategyState;

#[derive(Default)]
struct MockStateWriter {
    saved: std::sync::Mutex<Option<StrategyState>>,
}

#[derive(Default)]
struct MockBarLogWriter {
    last: std::sync::Mutex<Option<eth_btc_strategy::logging::BarLog>>,
}

impl MockBarLogWriter {
    fn last(&self) -> Option<eth_btc_strategy::logging::BarLog> {
        self.last.lock().expect("barlog lock").clone()
    }
}

impl BarLogWriter for MockBarLogWriter {
    fn write(&self, bar: &eth_btc_strategy::logging::BarLog) -> Result<(), std::io::Error> {
        *self.last.lock().expect("barlog lock") = Some(bar.clone());
        Ok(())
    }
}

#[derive(Default)]
struct MockTradeLogWriter {
    logs: std::sync::Mutex<Vec<TradeLog>>,
}

impl MockTradeLogWriter {
    fn logs(&self) -> Vec<TradeLog> {
        self.logs.lock().expect("tradelog lock").clone()
    }
}

impl TradeLogWriter for MockTradeLogWriter {
    fn write(&self, log: &TradeLog) -> Result<(), std::io::Error> {
        self.logs.lock().expect("tradelog lock").push(log.clone());
        Ok(())
    }
}

#[derive(Default)]
struct MockPriceWriter {
    last: std::sync::Mutex<Option<PriceBarRecord>>,
}

impl MockPriceWriter {
    fn last(&self) -> Option<PriceBarRecord> {
        self.last.lock().expect("price lock").clone()
    }
}

impl PriceBarWriter for MockPriceWriter {
    fn write(&self, record: &PriceBarRecord) -> Result<(), std::io::Error> {
        *self.last.lock().expect("price lock") = Some(record.clone());
        Ok(())
    }
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

fn runner_with_trade_sequence(timestamps: &[chrono::DateTime<Utc>]) -> LiveRunner {
    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.tp_z = dec!(0.45);
    config.strategy.sl_z = dec!(10.0);
    config.position.c_value = Some(dec!(100));

    let mut price_source = MockPriceSource::default();
    for (idx, ts) in timestamps.iter().enumerate() {
        let (eth, btc) = match idx {
            0 | 1 | 2 => (dec!(100), dec!(100)),
            3 => (dec!(271.8281828), dec!(100)),
            _ => (dec!(164.872127), dec!(100)),
        };
        price_source.insert_bar(PriceBar::new(Symbol::EthPerp, *ts, Some(eth), None, None));
        price_source.insert_bar(PriceBar::new(Symbol::BtcPerp, *ts, Some(btc), None, None));
    }

    let execution = ExecutionEngine::new(Arc::new(PaperOrderExecutor), RetryConfig::fast());
    let engine = eth_btc_strategy::core::strategy::StrategyEngine::new(config.clone(), execution)
        .expect("engine");
    let price_fetcher = PriceFetcher::new(Arc::new(price_source), config.data.price_field);

    LiveRunner::new(engine, price_fetcher, None)
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
async fn runner_writes_stats_log() {
    let timestamp = Utc.timestamp_opt(0, 0).unwrap();
    let mut runner = runner_with_mocks(timestamp);
    let writer = Arc::new(MockBarLogWriter::default());

    runner = runner.with_stats_writer(writer.clone());

    runner.run_once_at(timestamp).await.unwrap();

    let logged = writer.last().expect("expected stats log");
    assert_eq!(logged.timestamp, timestamp);
    assert_eq!(logged.eth_price, Some(dec!(2000)));
    assert_eq!(logged.btc_price, Some(dec!(30000)));
}

#[tokio::test]
async fn runner_writes_trade_logs() {
    let timestamps = [
        Utc.timestamp_opt(0, 0).unwrap(),
        Utc.timestamp_opt(900, 0).unwrap(),
        Utc.timestamp_opt(1800, 0).unwrap(),
        Utc.timestamp_opt(2700, 0).unwrap(),
        Utc.timestamp_opt(3600, 0).unwrap(),
    ];
    let mut runner = runner_with_trade_sequence(&timestamps);
    let writer = Arc::new(MockTradeLogWriter::default());
    runner = runner.with_trade_writer(writer.clone());

    for ts in timestamps {
        runner.run_once_at(ts).await.unwrap();
    }

    let logs = writer.logs();
    assert!(logs.iter().any(|log| matches!(log.event, eth_btc_strategy::logging::TradeEvent::Entry)));
    assert!(logs.iter().any(|log| matches!(log.event, eth_btc_strategy::logging::TradeEvent::Exit(_))));
}

#[tokio::test]
async fn runner_writes_price_records() {
    let timestamp = Utc.timestamp_opt(0, 0).unwrap();
    let mut runner = runner_with_mocks(timestamp);
    let writer = Arc::new(MockPriceWriter::default());
    runner = runner.with_price_writer(writer.clone());

    runner.run_once_at(timestamp).await.unwrap();

    let record = writer.last().expect("expected price record");
    assert_eq!(record.timestamp, timestamp);
    assert_eq!(record.eth_mid, Some(dec!(2000)));
    assert_eq!(record.eth_mark, None);
    assert_eq!(record.eth_close, None);
    assert_eq!(record.btc_mid, Some(dec!(30000)));
    assert_eq!(record.btc_mark, None);
    assert_eq!(record.btc_close, None);
    assert_eq!(record.funding_eth, Some(dec!(0.0001)));
    assert_eq!(record.funding_btc, Some(dec!(0.0002)));
    assert_eq!(record.funding_interval_hours, Some(8));
}

#[tokio::test]
async fn runner_uses_account_balance_for_equity_ratio() {
    let timestamps = [
        Utc.timestamp_opt(0, 0).unwrap(),
        Utc.timestamp_opt(900, 0).unwrap(),
        Utc.timestamp_opt(1800, 0).unwrap(),
        Utc.timestamp_opt(2700, 0).unwrap(),
    ];

    let mut config = Config::default();
    config.strategy.n_z = 3;
    config.position.n_vol = 1;
    config.strategy.entry_z = dec!(0.5);
    config.strategy.sl_z = dec!(2.0);
    config.position.c_mode = CapitalMode::EquityRatio;
    config.position.equity_ratio_k = Some(dec!(0.1));
    config.position.equity_value = Some(dec!(10000));

    let mut price_source = MockPriceSource::default();
    for (idx, ts) in timestamps.iter().enumerate() {
        let (eth, btc) = match idx {
            0 | 1 | 2 => (dec!(100), dec!(100)),
            _ => (dec!(271.8281828), dec!(100)),
        };
        price_source.insert_bar(PriceBar::new(Symbol::EthPerp, *ts, Some(eth), None, None));
        price_source.insert_bar(PriceBar::new(Symbol::BtcPerp, *ts, Some(btc), None, None));
    }

    let execution = ExecutionEngine::new(Arc::new(PaperOrderExecutor), RetryConfig::fast());
    let engine = StrategyEngine::new(config.clone(), execution).expect("engine");
    let price_fetcher = PriceFetcher::new(Arc::new(price_source), config.data.price_field);

    let mut account_source = MockAccountSource::default();
    for _ in 0..timestamps.len() {
        account_source.push_response(Ok(dec!(1000)));
    }
    let account_source = Arc::new(account_source);

    let mut runner = LiveRunner::new(engine, price_fetcher, None)
        .with_account_source(account_source);

    let mut last = None;
    for ts in timestamps {
        last = Some(runner.run_once_at(ts).await.unwrap());
    }

    let outcome = last.expect("expected outcome");
    assert_eq!(outcome.bar_log.notional_eth, Some(dec!(50)));
    assert_eq!(outcome.bar_log.notional_btc, Some(dec!(50)));
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
