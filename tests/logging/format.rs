use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::logging::{BarLog, LogFormatter};
use eth_btc_strategy::state::StrategyStatus;

#[test]
fn log_format_outputs_json_and_text() {
    let bar = BarLog {
        timestamp: Utc.timestamp_opt(0, 0).unwrap(),
        eth_price: Some(dec!(100)),
        btc_price: Some(dec!(200)),
        r: Some(dec!(0.1)),
        mu: Some(dec!(0.05)),
        sigma: Some(dec!(0.2)),
        sigma_eff: Some(dec!(0.2)),
        zscore: Some(dec!(1.5)),
        vol_eth: None,
        vol_btc: None,
        w_eth: None,
        w_btc: None,
        notional_eth: None,
        notional_btc: None,
        funding_eth: None,
        funding_btc: None,
        funding_cost_est: None,
        funding_skip: None,
        state: StrategyStatus::Flat,
        position: None,
        events: vec![],
    };

    let formatter = LogFormatter::default();
    let json = formatter.format_json(&bar).unwrap();
    let text = formatter.format_text(&bar);

    assert!(json.contains("\"eth_price\""));
    assert!(text.contains("ETH"));
    assert!(text.contains("Z"));
}
