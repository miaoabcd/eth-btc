use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

use eth_btc_strategy::config::{FundingConfig, FundingMode, Symbol};
use eth_btc_strategy::core::TradeDirection;
use eth_btc_strategy::funding::{
    FundingCostEstimate, FundingRate, apply_funding_controls, estimate_funding_cost,
};

#[test]
fn funding_cost_estimate_accounts_for_direction_and_hold() {
    let eth_rate = FundingRate {
        symbol: Symbol::EthPerp,
        rate: dec!(0.01),
        timestamp: Utc.timestamp_opt(0, 0).unwrap(),
        interval_hours: 8,
    };
    let btc_rate = FundingRate {
        symbol: Symbol::BtcPerp,
        rate: dec!(0.005),
        timestamp: Utc.timestamp_opt(0, 0).unwrap(),
        interval_hours: 8,
    };

    let estimate = estimate_funding_cost(
        TradeDirection::LongEthShortBtc,
        dec!(100),
        dec!(100),
        &eth_rate,
        &btc_rate,
        16,
    )
    .unwrap();

    assert_eq!(estimate.cost_est, dec!(1.0));
    assert_eq!(estimate.normalized, dec!(0.005));
}

#[test]
fn funding_controls_filter_threshold_and_size() {
    let estimate = FundingCostEstimate {
        cost_est: dec!(2.0),
        normalized: dec!(0.02),
        interval_hours: 8,
    };

    let mut config = FundingConfig::default();
    config.modes = vec![
        FundingMode::Filter,
        FundingMode::Threshold,
        FundingMode::Size,
    ];
    config.funding_cost_threshold = Some(dec!(1.0));
    config.funding_threshold_k = Some(dec!(5.0));
    config.funding_size_alpha = Some(dec!(5.0));
    config.c_min_ratio = Some(dec!(0.5));

    let decision = apply_funding_controls(&config, dec!(1.5), dec!(100), &estimate).unwrap();

    assert!(decision.should_skip);
    assert_eq!(decision.adjusted_entry_z, dec!(1.5) + dec!(0.1));
    assert_eq!(decision.adjusted_capital, dec!(100) * dec!(0.9));
}
