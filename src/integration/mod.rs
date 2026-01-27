use rust_decimal::Decimal;

use crate::backtest::Metrics;
use crate::config::Config;

pub fn papertrading_gate(metrics: &Metrics) -> bool {
    metrics.max_drawdown <= Decimal::new(15, 2)
}

pub fn deployment_ready(config: &Config) -> bool {
    config.validate().is_ok()
}
