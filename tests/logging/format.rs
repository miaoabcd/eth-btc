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
        regime_half_life_bars: None,
        regime_gate_pass: None,
        expected_edge_bps: None,
        estimated_cost_bps: None,
        estimated_net_edge_bps: None,
        cost_gate_required_net_edge_bps: None,
        cost_gate_pass: None,
        eth_best_bid: None,
        eth_best_ask: None,
        eth_bid_size: None,
        eth_ask_size: None,
        eth_spread_bps: None,
        btc_best_bid: None,
        btc_best_ask: None,
        btc_bid_size: None,
        btc_ask_size: None,
        btc_spread_bps: None,
        entry_block_reason: None,
        run_error: None,
        unrealized_pnl: dec!(0.12),
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
    assert!(text.contains("UPNL"));
}
