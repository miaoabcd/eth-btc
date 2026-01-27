use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;

use eth_btc_strategy::config::Symbol;
use eth_btc_strategy::data::{DataError, PriceBar, PriceHistorySet, PriceWindow};

fn bar(symbol: Symbol, timestamp: i64, mid: i64) -> PriceBar {
    PriceBar::new(
        symbol,
        Utc.timestamp_opt(timestamp, 0).unwrap(),
        Some(Decimal::from(mid)),
        None,
        None,
    )
}

#[test]
fn price_history_set_tracks_multiple_windows() {
    let mut history = PriceHistorySet::new(3, 5, 6).unwrap();

    for i in 0..6 {
        let ts = 100 + i;
        history
            .push_pair(
                bar(Symbol::EthPerp, ts, 100 + i),
                bar(Symbol::BtcPerp, ts, 200 + i),
            )
            .unwrap();
    }

    assert!(history.is_warmed_up(PriceWindow::ZScore));
    assert!(history.is_warmed_up(PriceWindow::Volatility));
    assert!(history.is_warmed_up(PriceWindow::SigmaQuantile));

    let eth_z = history.window(Symbol::EthPerp, PriceWindow::ZScore);
    assert_eq!(eth_z.len(), 3);
    assert_eq!(eth_z[0].timestamp, Utc.timestamp_opt(103, 0).unwrap());
    assert_eq!(eth_z[2].timestamp, Utc.timestamp_opt(105, 0).unwrap());

    let btc_vol = history.window(Symbol::BtcPerp, PriceWindow::Volatility);
    assert_eq!(btc_vol.len(), 5);
    assert_eq!(btc_vol[0].timestamp, Utc.timestamp_opt(101, 0).unwrap());
    assert_eq!(btc_vol[4].timestamp, Utc.timestamp_opt(105, 0).unwrap());
}

#[test]
fn price_history_set_rejects_out_of_order_timestamps() {
    let mut history = PriceHistorySet::new(2, 3, 4).unwrap();
    let ts = 100;

    history
        .push_pair(bar(Symbol::EthPerp, ts, 100), bar(Symbol::BtcPerp, ts, 200))
        .unwrap();

    let err = history.push_pair(bar(Symbol::EthPerp, ts, 101), bar(Symbol::BtcPerp, ts, 201));
    assert!(matches!(err, Err(DataError::InvalidTimestamp(_))));
}

#[test]
fn price_history_set_rejects_mismatched_symbols_or_times() {
    let mut history = PriceHistorySet::new(2, 3, 4).unwrap();

    let err = history.push_pair(
        bar(Symbol::EthPerp, 100, 100),
        bar(Symbol::BtcPerp, 101, 200),
    );
    assert!(matches!(err, Err(DataError::InconsistentData(_))));
}
