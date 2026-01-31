use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use crate::backtest::Metrics;
use crate::config::Config;
use crate::data::PriceFetcher;

fn max_drawdown() -> Decimal {
    Decimal::new(15, 2)
}

fn min_sharpe() -> Decimal {
    Decimal::new(5, 1)
}

fn min_win_rate() -> Decimal {
    Decimal::new(5, 1)
}

fn min_profit_factor() -> Decimal {
    Decimal::new(12, 1)
}

fn min_trades() -> usize {
    30
}

pub fn papertrading_gate(metrics: &Metrics) -> bool {
    metrics.max_drawdown <= max_drawdown()
        && metrics.sharpe_ratio >= min_sharpe()
        && metrics.win_rate >= min_win_rate()
        && metrics.profit_factor >= min_profit_factor()
        && metrics.trade_count >= min_trades()
}

pub async fn api_connectivity_ok(fetcher: &PriceFetcher, timestamp: DateTime<Utc>) -> bool {
    fetcher.fetch_pair_prices(timestamp).await.is_ok()
}

pub fn deployment_ready(config: &Config, api_ok: bool) -> bool {
    config.validate().is_ok() && api_ok
}
