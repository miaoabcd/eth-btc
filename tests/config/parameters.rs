use eth_btc_strategy::config::{
    CapitalMode, Config, FundingMode, LogFormat, PriceField, SigmaFloorMode, Symbol,
    V1_BASELINE_CONFIG, get_default_config,
};
use eth_btc_strategy::position::MinSizePolicy;
use rust_decimal_macros::dec;

#[test]
fn default_signal_parameters() {
    let config = get_default_config();

    assert_eq!(config.strategy.n_z, 384);
    assert_eq!(config.strategy.entry_z, dec!(1.5));
    assert_eq!(config.strategy.tp_z, dec!(0.45));
    assert_eq!(config.strategy.sl_z, dec!(3.5));
}

#[test]
fn default_sigma_floor_parameters() {
    let config = get_default_config();

    assert!(matches!(config.sigma_floor.mode, SigmaFloorMode::Const));
    assert!(config.sigma_floor.sigma_floor_const > dec!(0));
    assert!(config.sigma_floor.sigma_floor_quantile_window > 0);
    assert!(config.sigma_floor.sigma_floor_quantile_p > dec!(0));
    assert!(config.sigma_floor.ewma_half_life > 0);
}

#[test]
fn default_position_parameters() {
    let config = get_default_config();

    assert!(matches!(
        config.position.c_mode,
        CapitalMode::FixedNotional | CapitalMode::EquityRatio
    ));
    assert!(config.position.c_value.is_some() || config.position.equity_ratio_k.is_some());
    assert_eq!(config.position.n_vol, 672);
    assert_eq!(config.position.max_position_groups, 1);
}

#[test]
fn default_funding_parameters() {
    let config = get_default_config();

    assert!(config.funding.modes.contains(&FundingMode::Filter));
    assert!(config.funding.funding_cost_threshold.is_some());
    assert!(config.funding.funding_threshold_k.is_some());
    assert!(config.funding.funding_size_alpha.is_some());
    assert!(config.funding.c_min_ratio.is_some());
}

#[test]
fn default_risk_parameters() {
    let config = get_default_config();

    assert_eq!(config.risk.max_hold_hours, 48);
    assert_eq!(config.risk.cooldown_hours, 24);
    assert_eq!(config.risk.confirm_bars_tp, 0);
}

#[test]
fn default_runtime_and_auth_parameters() {
    let config = get_default_config();

    assert_eq!(config.runtime.base_url, "https://api.hyperliquid.xyz");
    assert_eq!(config.runtime.interval_secs, 900);
    assert!(!config.runtime.once);
    assert!(!config.runtime.paper);
    assert!(!config.runtime.disable_funding);
    assert!(config.runtime.state_path.is_none());
    assert!(config.auth.private_key.is_none());
    assert!(config.auth.vault_address.is_none());
}

#[test]
fn default_logging_parameters() {
    let config = get_default_config();

    assert_eq!(config.logging.level, "info");
    assert!(matches!(
        config.logging.format,
        LogFormat::Json | LogFormat::Text
    ));
    assert_eq!(config.logging.stats_path.as_deref(), Some("stats.log"));
    assert!(config.logging.stats_format.is_none());
    assert!(config.logging.trade_path.is_none());
    assert!(config.logging.trade_format.is_none());
    assert!(config.logging.price_db_path.is_none());
}

#[test]
fn default_price_field_and_constraints() {
    let config = get_default_config();

    assert!(matches!(
        config.data.price_field,
        PriceField::Mid | PriceField::Mark | PriceField::Close
    ));
    assert!(config.instrument_constraints.contains_key(&Symbol::EthPerp));
    assert!(config.instrument_constraints.contains_key(&Symbol::BtcPerp));
    assert!(matches!(
        config.position.min_size_policy,
        MinSizePolicy::Skip | MinSizePolicy::Adjust
    ));
}

#[test]
fn default_config_is_valid() {
    let config = get_default_config();

    assert!(config.validate().is_ok());
}

#[test]
fn tp_z_must_be_less_than_entry_z() {
    let mut config = get_default_config();
    config.strategy.tp_z = config.strategy.entry_z;

    let err = config
        .validate()
        .expect_err("expected tp_z validation failure");
    assert!(matches!(
        err,
        eth_btc_strategy::config::ConfigError::InvalidValue { field, .. }
        if field == "strategy.tp_z"
    ));
}

#[test]
fn sigma_floor_quantile_p_must_be_leq_one() {
    let mut config = get_default_config();
    config.sigma_floor.sigma_floor_quantile_p = dec!(1.1);

    let err = config
        .validate()
        .expect_err("expected sigma_floor_quantile_p upper bound failure");
    assert!(matches!(
        err,
        eth_btc_strategy::config::ConfigError::InvalidValue { field, .. }
        if field == "sigma_floor.sigma_floor_quantile_p"
    ));
}

#[test]
fn v1_baseline_config_matches_defaults() {
    let config = get_default_config();

    assert_eq!(&config, &*V1_BASELINE_CONFIG);
    assert_eq!(V1_BASELINE_CONFIG.strategy.n_z, 384);
    assert_eq!(V1_BASELINE_CONFIG.position.n_vol, 672);
}

#[test]
fn symbol_all_returns_static_slice() {
    let symbols: &'static [Symbol] = Symbol::all();
    assert_eq!(symbols.len(), 2);
}

#[test]
fn default_config_is_type_safe() {
    let config = get_default_config();

    assert!(Config::validate(&config).is_ok());
}
