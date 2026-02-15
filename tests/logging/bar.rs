use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::core::ExitReason;
use eth_btc_strategy::logging::{BarLog, LogEvent};
use eth_btc_strategy::state::StrategyStatus;

#[test]
fn bar_log_contains_required_fields() {
    let bar = BarLog {
        timestamp: Utc.timestamp_opt(0, 0).unwrap(),
        eth_price: Some(dec!(100)),
        btc_price: Some(dec!(200)),
        r: Some(dec!(0.1)),
        mu: Some(dec!(0.05)),
        sigma: Some(dec!(0.2)),
        sigma_eff: Some(dec!(0.2)),
        zscore: Some(dec!(1.5)),
        vol_eth: Some(dec!(0.3)),
        vol_btc: Some(dec!(0.4)),
        w_eth: Some(dec!(0.6)),
        w_btc: Some(dec!(0.4)),
        notional_eth: Some(dec!(6000)),
        notional_btc: Some(dec!(4000)),
        funding_eth: Some(dec!(0.01)),
        funding_btc: Some(dec!(0.02)),
        funding_cost_est: Some(dec!(5)),
        funding_skip: Some(false),
        unrealized_pnl: dec!(1.23),
        state: StrategyStatus::Flat,
        position: None,
        events: vec![LogEvent::Exit(ExitReason::TakeProfit)],
    };

    let json = bar.to_json_value();
    assert!(json.get("timestamp").is_some());
    assert!(json.get("eth_price").is_some());
    assert!(json.get("btc_price").is_some());
    assert!(json.get("zscore").is_some());
    assert!(json.get("unrealized_pnl").is_some());
    assert!(json.get("state").is_some());
    assert!(json.get("events").is_some());
}
