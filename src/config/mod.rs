use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

use once_cell::sync::Lazy;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid value for {field}: {message}")]
    InvalidValue {
        field: &'static str,
        message: String,
    },
    #[error("missing required value for {field}")]
    MissingValue { field: &'static str },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml parse error: {0}")]
    TomlParse(#[from] toml::de::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Symbol {
    EthPerp,
    BtcPerp,
}

impl Symbol {
    pub fn all() -> &'static [Symbol] {
        const ALL: [Symbol; 2] = [Symbol::EthPerp, Symbol::BtcPerp];
        &ALL
    }
}

impl FromStr for Symbol {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_uppercase().as_str() {
            "ETH-PERP" | "ETH_PERP" => Ok(Symbol::EthPerp),
            "BTC-PERP" | "BTC_PERP" => Ok(Symbol::BtcPerp),
            _ => Err(ConfigError::InvalidValue {
                field: "symbol",
                message: format!("unsupported symbol: {value}"),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PriceField {
    #[default]
    Mid,
    Mark,
    Close,
}

impl FromStr for PriceField {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_uppercase().as_str() {
            "MID" => Ok(PriceField::Mid),
            "MARK" => Ok(PriceField::Mark),
            "CLOSE" => Ok(PriceField::Close),
            _ => Err(ConfigError::InvalidValue {
                field: "price_field",
                message: format!("unsupported price field: {value}"),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SigmaFloorMode {
    #[default]
    Const,
    Quantile,
    EwmaMix,
}

impl FromStr for SigmaFloorMode {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_uppercase().as_str() {
            "CONST" => Ok(SigmaFloorMode::Const),
            "QUANTILE" => Ok(SigmaFloorMode::Quantile),
            "EWMA_MIX" => Ok(SigmaFloorMode::EwmaMix),
            _ => Err(ConfigError::InvalidValue {
                field: "sigma_floor.mode",
                message: format!("unsupported sigma floor mode: {value}"),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CapitalMode {
    #[default]
    FixedNotional,
    EquityRatio,
}

impl FromStr for CapitalMode {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_uppercase().as_str() {
            "FIXED_NOTIONAL" => Ok(CapitalMode::FixedNotional),
            "EQUITY_RATIO" => Ok(CapitalMode::EquityRatio),
            _ => Err(ConfigError::InvalidValue {
                field: "position.c_mode",
                message: format!("unsupported capital mode: {value}"),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FundingMode {
    Filter,
    Threshold,
    Size,
}

impl FromStr for FundingMode {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_uppercase().as_str() {
            "FILTER" => Ok(FundingMode::Filter),
            "THRESHOLD" => Ok(FundingMode::Threshold),
            "SIZE" => Ok(FundingMode::Size),
            _ => Err(ConfigError::InvalidValue {
                field: "funding.mode",
                message: format!("unsupported funding mode: {value}"),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderType {
    #[default]
    Market,
    Limit,
}

impl FromStr for OrderType {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_uppercase().as_str() {
            "MARKET" => Ok(OrderType::Market),
            "LIMIT" => Ok(OrderType::Limit),
            _ => Err(ConfigError::InvalidValue {
                field: "execution.order_type",
                message: format!("unsupported order type: {value}"),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LogFormat {
    #[default]
    Json,
    Text,
}

impl FromStr for LogFormat {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_uppercase().as_str() {
            "JSON" => Ok(LogFormat::Json),
            "TEXT" => Ok(LogFormat::Text),
            _ => Err(ConfigError::InvalidValue {
                field: "logging.format",
                message: format!("unsupported log format: {value}"),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RoundingMode {
    #[default]
    Floor,
    Ceil,
    Round,
}

impl FromStr for RoundingMode {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_uppercase().as_str() {
            "FLOOR" => Ok(RoundingMode::Floor),
            "CEIL" => Ok(RoundingMode::Ceil),
            "ROUND" => Ok(RoundingMode::Round),
            _ => Err(ConfigError::InvalidValue {
                field: "instrument_constraints.rounding_mode",
                message: format!("unsupported rounding mode: {value}"),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyConfig {
    pub n_z: usize,
    pub entry_z: Decimal,
    pub tp_z: Decimal,
    pub sl_z: Decimal,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            n_z: 384,
            entry_z: Decimal::new(15, 1),
            tp_z: Decimal::new(45, 2),
            sl_z: Decimal::new(35, 1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SigmaFloorConfig {
    pub mode: SigmaFloorMode,
    pub sigma_floor_const: Decimal,
    pub sigma_floor_quantile_window: u32,
    pub sigma_floor_quantile_p: Decimal,
    pub ewma_half_life: u32,
}

impl Default for SigmaFloorConfig {
    fn default() -> Self {
        Self {
            mode: SigmaFloorMode::Const,
            sigma_floor_const: Decimal::new(1, 3),
            sigma_floor_quantile_window: 30,
            sigma_floor_quantile_p: Decimal::new(10, 2),
            ewma_half_life: 20,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PositionConfig {
    pub c_mode: CapitalMode,
    pub c_value: Option<Decimal>,
    pub equity_value: Option<Decimal>,
    pub equity_ratio_k: Option<Decimal>,
    pub max_notional: Option<Decimal>,
    pub n_vol: usize,
    pub max_position_groups: u32,
}

impl Default for PositionConfig {
    fn default() -> Self {
        Self {
            c_mode: CapitalMode::FixedNotional,
            c_value: Some(Decimal::new(50000, 0)),
            equity_value: None,
            equity_ratio_k: None,
            max_notional: None,
            n_vol: 672,
            max_position_groups: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FundingConfig {
    pub modes: Vec<FundingMode>,
    pub funding_cost_threshold: Option<Decimal>,
    pub funding_threshold_k: Option<Decimal>,
    pub funding_size_alpha: Option<Decimal>,
    pub c_min_ratio: Option<Decimal>,
}

impl Default for FundingConfig {
    fn default() -> Self {
        Self {
            modes: vec![FundingMode::Filter],
            funding_cost_threshold: Some(Decimal::new(1, 3)),
            funding_threshold_k: Some(Decimal::new(5, 1)),
            funding_size_alpha: Some(Decimal::new(5, 1)),
            c_min_ratio: Some(Decimal::new(3, 1)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiskConfig {
    pub max_hold_hours: u32,
    pub cooldown_hours: u32,
    pub confirm_bars_tp: u32,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_hold_hours: 48,
            cooldown_hours: 24,
            confirm_bars_tp: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataConfig {
    pub price_field: PriceField,
}

impl Default for DataConfig {
    fn default() -> Self {
        Self {
            price_field: PriceField::Mid,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionConfig {
    pub order_type: OrderType,
    pub slippage_bps: u32,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            order_type: OrderType::Market,
            slippage_bps: 5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub base_url: String,
    pub interval_secs: u64,
    pub once: bool,
    pub paper: bool,
    pub disable_funding: bool,
    pub state_path: Option<String>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.hyperliquid.xyz".to_string(),
            interval_secs: 900,
            once: false,
            paper: false,
            disable_funding: false,
            state_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AuthConfig {
    pub private_key: Option<String>,
    pub vault_address: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: LogFormat,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: LogFormat::Json,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AlertsConfig {
    pub webhook_url: String,
    pub email_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub include_fees: bool,
    pub fee_bps: u32,
    pub include_slippage: bool,
    pub slippage_bps: u32,
    pub include_funding: bool,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            include_fees: true,
            fee_bps: 2,
            include_slippage: true,
            slippage_bps: 5,
            include_funding: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstrumentConstraints {
    pub min_qty: Decimal,
    pub min_notional: Decimal,
    pub step_size: Decimal,
    pub tick_size: Decimal,
    pub qty_precision: u32,
    pub price_precision: u32,
    pub rounding_mode: RoundingMode,
}

impl Default for InstrumentConstraints {
    fn default() -> Self {
        Self {
            min_qty: Decimal::new(1, 2),
            min_notional: Decimal::new(10, 0),
            step_size: Decimal::new(1, 3),
            tick_size: Decimal::new(1, 1),
            qty_precision: 3,
            price_precision: 1,
            rounding_mode: RoundingMode::Floor,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub strategy: StrategyConfig,
    pub sigma_floor: SigmaFloorConfig,
    pub position: PositionConfig,
    pub funding: FundingConfig,
    pub risk: RiskConfig,
    pub data: DataConfig,
    pub execution: ExecutionConfig,
    pub runtime: RuntimeConfig,
    pub auth: AuthConfig,
    pub logging: LoggingConfig,
    pub alerts: AlertsConfig,
    pub backtest: BacktestConfig,
    pub instrument_constraints: HashMap<Symbol, InstrumentConstraints>,
}

impl Default for Config {
    fn default() -> Self {
        let mut instrument_constraints = HashMap::new();
        instrument_constraints.insert(Symbol::EthPerp, InstrumentConstraints::default());
        instrument_constraints.insert(Symbol::BtcPerp, InstrumentConstraints::default());

        Self {
            strategy: StrategyConfig::default(),
            sigma_floor: SigmaFloorConfig::default(),
            position: PositionConfig::default(),
            funding: FundingConfig::default(),
            risk: RiskConfig::default(),
            data: DataConfig::default(),
            execution: ExecutionConfig::default(),
            runtime: RuntimeConfig::default(),
            auth: AuthConfig::default(),
            logging: LoggingConfig::default(),
            alerts: AlertsConfig::default(),
            backtest: BacktestConfig::default(),
            instrument_constraints,
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.strategy.n_z == 0 {
            return Err(ConfigError::InvalidValue {
                field: "strategy.n_z",
                message: "must be > 0".to_string(),
            });
        }
        if self.strategy.entry_z >= self.strategy.sl_z {
            return Err(ConfigError::InvalidValue {
                field: "strategy.entry_z",
                message: "must be < sl_z".to_string(),
            });
        }
        if self.strategy.tp_z >= self.strategy.entry_z {
            return Err(ConfigError::InvalidValue {
                field: "strategy.tp_z",
                message: "must be < entry_z".to_string(),
            });
        }
        if self.sigma_floor.sigma_floor_const <= Decimal::ZERO {
            return Err(ConfigError::InvalidValue {
                field: "sigma_floor.sigma_floor_const",
                message: "must be > 0".to_string(),
            });
        }
        if self.sigma_floor.sigma_floor_quantile_window == 0 {
            return Err(ConfigError::InvalidValue {
                field: "sigma_floor.sigma_floor_quantile_window",
                message: "must be > 0".to_string(),
            });
        }
        if self.sigma_floor.sigma_floor_quantile_p <= Decimal::ZERO {
            return Err(ConfigError::InvalidValue {
                field: "sigma_floor.sigma_floor_quantile_p",
                message: "must be > 0".to_string(),
            });
        }
        if self.sigma_floor.sigma_floor_quantile_p > Decimal::ONE {
            return Err(ConfigError::InvalidValue {
                field: "sigma_floor.sigma_floor_quantile_p",
                message: "must be <= 1".to_string(),
            });
        }
        if self.sigma_floor.ewma_half_life == 0 {
            return Err(ConfigError::InvalidValue {
                field: "sigma_floor.ewma_half_life",
                message: "must be > 0".to_string(),
            });
        }
        if self.position.n_vol == 0 {
            return Err(ConfigError::InvalidValue {
                field: "position.n_vol",
                message: "must be > 0".to_string(),
            });
        }
        match self.position.c_mode {
            CapitalMode::FixedNotional if self.position.c_value.is_none() => {
                return Err(ConfigError::MissingValue {
                    field: "position.c_value",
                });
            }
            CapitalMode::EquityRatio if self.position.equity_ratio_k.is_none() => {
                return Err(ConfigError::MissingValue {
                    field: "position.equity_ratio_k",
                });
            }
            CapitalMode::EquityRatio if self.position.equity_value.is_none() => {
                return Err(ConfigError::MissingValue {
                    field: "position.equity_value",
                });
            }
            _ => {}
        }
        if let Some(max_notional) = self.position.max_notional
            && max_notional <= Decimal::ZERO
        {
            return Err(ConfigError::InvalidValue {
                field: "position.max_notional",
                message: "must be > 0".to_string(),
            });
        }
        if self.position.max_position_groups == 0 {
            return Err(ConfigError::InvalidValue {
                field: "position.max_position_groups",
                message: "must be >= 1".to_string(),
            });
        }
        if let Some(value) = self.funding.c_min_ratio
            && (value < Decimal::ZERO || value > Decimal::ONE)
        {
            return Err(ConfigError::InvalidValue {
                field: "funding.c_min_ratio",
                message: "must be between 0 and 1".to_string(),
            });
        }
        if self.runtime.base_url.trim().is_empty() {
            return Err(ConfigError::InvalidValue {
                field: "runtime.base_url",
                message: "must be non-empty".to_string(),
            });
        }
        if self.runtime.interval_secs == 0 {
            return Err(ConfigError::InvalidValue {
                field: "runtime.interval_secs",
                message: "must be > 0".to_string(),
            });
        }
        if let Some(path) = &self.runtime.state_path
            && path.trim().is_empty()
        {
            return Err(ConfigError::InvalidValue {
                field: "runtime.state_path",
                message: "must be non-empty when provided".to_string(),
            });
        }
        for symbol in Symbol::all() {
            if !self.instrument_constraints.contains_key(&symbol) {
                return Err(ConfigError::MissingValue {
                    field: "instrument_constraints",
                });
            }
        }
        for (symbol, constraints) in &self.instrument_constraints {
            if constraints.min_qty <= Decimal::ZERO {
                return Err(ConfigError::InvalidValue {
                    field: "instrument_constraints.min_qty",
                    message: format!("{symbol:?} must be > 0"),
                });
            }
            if constraints.step_size <= Decimal::ZERO {
                return Err(ConfigError::InvalidValue {
                    field: "instrument_constraints.step_size",
                    message: format!("{symbol:?} must be > 0"),
                });
            }
            if constraints.tick_size <= Decimal::ZERO {
                return Err(ConfigError::InvalidValue {
                    field: "instrument_constraints.tick_size",
                    message: format!("{symbol:?} must be > 0"),
                });
            }
        }
        Ok(())
    }

    pub fn from_toml_path(path: &Path) -> Result<Config, ConfigError> {
        let overrides = ConfigOverrides::from_toml_path(path)?;
        let mut config = Config::default();
        config.apply_overrides(overrides);
        config.validate()?;
        Ok(config)
    }

    fn apply_overrides(&mut self, overrides: ConfigOverrides) {
        if let Some(value) = overrides.strategy.n_z {
            self.strategy.n_z = value;
        }
        if let Some(value) = overrides.strategy.entry_z {
            self.strategy.entry_z = value;
        }
        if let Some(value) = overrides.strategy.tp_z {
            self.strategy.tp_z = value;
        }
        if let Some(value) = overrides.strategy.sl_z {
            self.strategy.sl_z = value;
        }
        if let Some(value) = overrides.sigma_floor.mode {
            self.sigma_floor.mode = value;
        }
        if let Some(value) = overrides.sigma_floor.sigma_floor_const {
            self.sigma_floor.sigma_floor_const = value;
        }
        if let Some(value) = overrides.sigma_floor.sigma_floor_quantile_window {
            self.sigma_floor.sigma_floor_quantile_window = value;
        }
        if let Some(value) = overrides.sigma_floor.sigma_floor_quantile_p {
            self.sigma_floor.sigma_floor_quantile_p = value;
        }
        if let Some(value) = overrides.sigma_floor.ewma_half_life {
            self.sigma_floor.ewma_half_life = value;
        }
        if let Some(value) = overrides.position.c_mode {
            self.position.c_mode = value;
        }
        if let Some(value) = overrides.position.c_value {
            self.position.c_value = Some(value);
        }
        if let Some(value) = overrides.position.equity_value {
            self.position.equity_value = Some(value);
        }
        if let Some(value) = overrides.position.equity_ratio_k {
            self.position.equity_ratio_k = Some(value);
        }
        if let Some(value) = overrides.position.max_notional {
            self.position.max_notional = Some(value);
        }
        if let Some(value) = overrides.position.n_vol {
            self.position.n_vol = value;
        }
        if let Some(value) = overrides.position.max_position_groups {
            self.position.max_position_groups = value;
        }
        if let Some(value) = overrides.funding.modes {
            self.funding.modes = value;
        }
        if let Some(value) = overrides.funding.funding_cost_threshold {
            self.funding.funding_cost_threshold = Some(value);
        }
        if let Some(value) = overrides.funding.funding_threshold_k {
            self.funding.funding_threshold_k = Some(value);
        }
        if let Some(value) = overrides.funding.funding_size_alpha {
            self.funding.funding_size_alpha = Some(value);
        }
        if let Some(value) = overrides.funding.c_min_ratio {
            self.funding.c_min_ratio = Some(value);
        }
        if let Some(value) = overrides.risk.max_hold_hours {
            self.risk.max_hold_hours = value;
        }
        if let Some(value) = overrides.risk.cooldown_hours {
            self.risk.cooldown_hours = value;
        }
        if let Some(value) = overrides.risk.confirm_bars_tp {
            self.risk.confirm_bars_tp = value;
        }
        if let Some(value) = overrides.data.price_field {
            self.data.price_field = value;
        }
        if let Some(value) = overrides.execution.order_type {
            self.execution.order_type = value;
        }
        if let Some(value) = overrides.execution.slippage_bps {
            self.execution.slippage_bps = value;
        }
        if let Some(value) = overrides.runtime.base_url {
            self.runtime.base_url = value;
        }
        if let Some(value) = overrides.runtime.interval_secs {
            self.runtime.interval_secs = value;
        }
        if let Some(value) = overrides.runtime.once {
            self.runtime.once = value;
        }
        if let Some(value) = overrides.runtime.paper {
            self.runtime.paper = value;
        }
        if let Some(value) = overrides.runtime.disable_funding {
            self.runtime.disable_funding = value;
        }
        if let Some(value) = overrides.runtime.state_path {
            self.runtime.state_path = Some(value);
        }
        if let Some(value) = overrides.auth.private_key {
            self.auth.private_key = Some(value);
        }
        if let Some(value) = overrides.auth.vault_address {
            self.auth.vault_address = Some(value);
        }
        if let Some(value) = overrides.logging.level {
            self.logging.level = value;
        }
        if let Some(value) = overrides.logging.format {
            self.logging.format = value;
        }
        if let Some(value) = overrides.alerts.webhook_url {
            self.alerts.webhook_url = value;
        }
        if let Some(value) = overrides.alerts.email_enabled {
            self.alerts.email_enabled = value;
        }
        if let Some(value) = overrides.backtest.include_fees {
            self.backtest.include_fees = value;
        }
        if let Some(value) = overrides.backtest.fee_bps {
            self.backtest.fee_bps = value;
        }
        if let Some(value) = overrides.backtest.include_slippage {
            self.backtest.include_slippage = value;
        }
        if let Some(value) = overrides.backtest.slippage_bps {
            self.backtest.slippage_bps = value;
        }
        if let Some(value) = overrides.backtest.include_funding {
            self.backtest.include_funding = value;
        }
        if let Some(overrides) = overrides.instrument_constraints {
            for (symbol, override_value) in overrides {
                let entry = self.instrument_constraints.entry(symbol).or_default();
                if let Some(value) = override_value.min_qty {
                    entry.min_qty = value;
                }
                if let Some(value) = override_value.min_notional {
                    entry.min_notional = value;
                }
                if let Some(value) = override_value.step_size {
                    entry.step_size = value;
                }
                if let Some(value) = override_value.tick_size {
                    entry.tick_size = value;
                }
                if let Some(value) = override_value.qty_precision {
                    entry.qty_precision = value;
                }
                if let Some(value) = override_value.price_precision {
                    entry.price_precision = value;
                }
                if let Some(value) = override_value.rounding_mode {
                    entry.rounding_mode = value;
                }
            }
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct ConfigOverrides {
    #[serde(default)]
    pub strategy: StrategyOverrides,
    #[serde(default)]
    pub sigma_floor: SigmaFloorOverrides,
    #[serde(default)]
    pub position: PositionOverrides,
    #[serde(default)]
    pub funding: FundingOverrides,
    #[serde(default)]
    pub risk: RiskOverrides,
    #[serde(default)]
    pub data: DataOverrides,
    #[serde(default)]
    pub execution: ExecutionOverrides,
    #[serde(default)]
    pub runtime: RuntimeOverrides,
    #[serde(default)]
    pub auth: AuthOverrides,
    #[serde(default)]
    pub logging: LoggingOverrides,
    #[serde(default)]
    pub alerts: AlertsOverrides,
    #[serde(default)]
    pub backtest: BacktestOverrides,
    pub instrument_constraints: Option<HashMap<Symbol, InstrumentConstraintsOverrides>>,
}

#[derive(Debug, Default, Deserialize)]
pub struct StrategyOverrides {
    pub n_z: Option<usize>,
    pub entry_z: Option<Decimal>,
    pub tp_z: Option<Decimal>,
    pub sl_z: Option<Decimal>,
}

#[derive(Debug, Default, Deserialize)]
pub struct SigmaFloorOverrides {
    pub mode: Option<SigmaFloorMode>,
    pub sigma_floor_const: Option<Decimal>,
    pub sigma_floor_quantile_window: Option<u32>,
    pub sigma_floor_quantile_p: Option<Decimal>,
    pub ewma_half_life: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
pub struct PositionOverrides {
    pub c_mode: Option<CapitalMode>,
    pub c_value: Option<Decimal>,
    pub equity_value: Option<Decimal>,
    pub equity_ratio_k: Option<Decimal>,
    pub max_notional: Option<Decimal>,
    pub n_vol: Option<usize>,
    pub max_position_groups: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
pub struct FundingOverrides {
    pub modes: Option<Vec<FundingMode>>,
    pub funding_cost_threshold: Option<Decimal>,
    pub funding_threshold_k: Option<Decimal>,
    pub funding_size_alpha: Option<Decimal>,
    pub c_min_ratio: Option<Decimal>,
}

#[derive(Debug, Default, Deserialize)]
pub struct RiskOverrides {
    pub max_hold_hours: Option<u32>,
    pub cooldown_hours: Option<u32>,
    pub confirm_bars_tp: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
pub struct DataOverrides {
    pub price_field: Option<PriceField>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ExecutionOverrides {
    pub order_type: Option<OrderType>,
    pub slippage_bps: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
pub struct RuntimeOverrides {
    pub base_url: Option<String>,
    pub interval_secs: Option<u64>,
    pub once: Option<bool>,
    pub paper: Option<bool>,
    pub disable_funding: Option<bool>,
    pub state_path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct AuthOverrides {
    pub private_key: Option<String>,
    pub vault_address: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct LoggingOverrides {
    pub level: Option<String>,
    pub format: Option<LogFormat>,
}

#[derive(Debug, Default, Deserialize)]
pub struct AlertsOverrides {
    pub webhook_url: Option<String>,
    pub email_enabled: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub struct BacktestOverrides {
    pub include_fees: Option<bool>,
    pub fee_bps: Option<u32>,
    pub include_slippage: Option<bool>,
    pub slippage_bps: Option<u32>,
    pub include_funding: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub struct InstrumentConstraintsOverrides {
    pub min_qty: Option<Decimal>,
    pub min_notional: Option<Decimal>,
    pub step_size: Option<Decimal>,
    pub tick_size: Option<Decimal>,
    pub qty_precision: Option<u32>,
    pub price_precision: Option<u32>,
    pub rounding_mode: Option<RoundingMode>,
}

impl ConfigOverrides {
    pub fn from_toml_path(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        let overrides = toml::from_str(&content)?;
        Ok(overrides)
    }

}

pub fn load_config(path: Option<&Path>) -> Result<Config, ConfigError> {
    load_config_with_cli(path, None)
}

pub fn load_config_with_cli(
    path: Option<&Path>,
    cli_overrides: Option<ConfigOverrides>,
) -> Result<Config, ConfigError> {
    let mut config = Config::default();
    if let Some(path) = path {
        let file_overrides = ConfigOverrides::from_toml_path(path)?;
        config.apply_overrides(file_overrides);
    }
    if let Some(cli_overrides) = cli_overrides {
        config.apply_overrides(cli_overrides);
    }
    config.validate()?;
    Ok(config)
}

pub static V1_BASELINE_CONFIG: Lazy<Config> = Lazy::new(Config::default);

pub fn get_default_config() -> Config {
    V1_BASELINE_CONFIG.clone()
}
