use std::time::Duration;

pub const HYPERLIQUID_TIMEOUT_ENV: &str = "HYPERLIQUID_TIMEOUT";
pub const HYPERLIQUID_CONNECT_TIMEOUT_ENV: &str = "HYPERLIQUID_CONNECT_TIMEOUT";
pub const DEFAULT_HYPERLIQUID_TIMEOUT_SECS: u64 = 10;
pub const DEFAULT_HYPERLIQUID_CONNECT_TIMEOUT_SECS: u64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HyperliquidHttpTimeouts {
    pub request_timeout: Duration,
    pub connect_timeout: Duration,
}

impl HyperliquidHttpTimeouts {
    pub fn from_env() -> Self {
        Self {
            request_timeout: duration_from_env(
                HYPERLIQUID_TIMEOUT_ENV,
                DEFAULT_HYPERLIQUID_TIMEOUT_SECS,
            ),
            connect_timeout: duration_from_env(
                HYPERLIQUID_CONNECT_TIMEOUT_ENV,
                DEFAULT_HYPERLIQUID_CONNECT_TIMEOUT_SECS,
            ),
        }
    }
}

impl Default for HyperliquidHttpTimeouts {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(DEFAULT_HYPERLIQUID_TIMEOUT_SECS),
            connect_timeout: Duration::from_secs(DEFAULT_HYPERLIQUID_CONNECT_TIMEOUT_SECS),
        }
    }
}

pub fn hyperliquid_reqwest_client(timeouts: HyperliquidHttpTimeouts) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(timeouts.request_timeout)
        .connect_timeout(timeouts.connect_timeout)
        .build()
        .expect("build hyperliquid reqwest client")
}

fn duration_from_env(key: &str, default_secs: u64) -> Duration {
    std::env::var(key)
        .ok()
        .and_then(|raw| parse_positive_duration_secs(&raw))
        .unwrap_or_else(|| Duration::from_secs(default_secs))
}

fn parse_positive_duration_secs(raw: &str) -> Option<Duration> {
    let secs = raw.parse::<f64>().ok()?;
    if !secs.is_finite() || secs <= 0.0 || secs > u64::MAX as f64 {
        return None;
    }
    Some(Duration::from_secs_f64(secs))
}
