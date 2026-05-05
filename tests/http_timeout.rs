use std::sync::Mutex;
use std::time::Duration;

use eth_btc_strategy::account::ReqwestAccountClient;
use eth_btc_strategy::data::ReqwestHttpClient;
use eth_btc_strategy::execution::ReqwestOrderClient;
use eth_btc_strategy::funding::ReqwestFundingClient;
use eth_btc_strategy::util::http::{
    DEFAULT_HYPERLIQUID_CONNECT_TIMEOUT_SECS, DEFAULT_HYPERLIQUID_TIMEOUT_SECS,
    HYPERLIQUID_CONNECT_TIMEOUT_ENV, HYPERLIQUID_TIMEOUT_ENV, HyperliquidHttpTimeouts,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn clear_timeout_env() {
    unsafe {
        std::env::remove_var(HYPERLIQUID_TIMEOUT_ENV);
        std::env::remove_var(HYPERLIQUID_CONNECT_TIMEOUT_ENV);
    }
}

#[test]
fn hyperliquid_timeouts_parse_positive_env_values() {
    let _guard = ENV_LOCK.lock().unwrap();
    clear_timeout_env();
    unsafe {
        std::env::set_var(HYPERLIQUID_TIMEOUT_ENV, "7.5");
        std::env::set_var(HYPERLIQUID_CONNECT_TIMEOUT_ENV, "2.25");
    }

    let timeouts = HyperliquidHttpTimeouts::from_env();

    assert_eq!(timeouts.request_timeout, Duration::from_millis(7500));
    assert_eq!(timeouts.connect_timeout, Duration::from_millis(2250));

    clear_timeout_env();
}

#[test]
fn hyperliquid_timeouts_fall_back_for_invalid_env_values() {
    let _guard = ENV_LOCK.lock().unwrap();
    clear_timeout_env();
    unsafe {
        std::env::set_var(HYPERLIQUID_TIMEOUT_ENV, "not-a-number");
        std::env::set_var(HYPERLIQUID_CONNECT_TIMEOUT_ENV, "0");
    }

    let timeouts = HyperliquidHttpTimeouts::from_env();

    assert_eq!(
        timeouts.request_timeout,
        Duration::from_secs(DEFAULT_HYPERLIQUID_TIMEOUT_SECS)
    );
    assert_eq!(
        timeouts.connect_timeout,
        Duration::from_secs(DEFAULT_HYPERLIQUID_CONNECT_TIMEOUT_SECS)
    );

    clear_timeout_env();
}

#[test]
fn hyperliquid_reqwest_clients_use_env_timeouts() {
    let _guard = ENV_LOCK.lock().unwrap();
    clear_timeout_env();
    unsafe {
        std::env::set_var(HYPERLIQUID_TIMEOUT_ENV, "6");
        std::env::set_var(HYPERLIQUID_CONNECT_TIMEOUT_ENV, "3");
    }

    let expected = HyperliquidHttpTimeouts {
        request_timeout: Duration::from_secs(6),
        connect_timeout: Duration::from_secs(3),
    };

    assert_eq!(ReqwestHttpClient::new().timeout_settings(), expected);
    assert_eq!(ReqwestFundingClient::new().timeout_settings(), expected);
    assert_eq!(ReqwestAccountClient::new().timeout_settings(), expected);
    assert_eq!(
        ReqwestOrderClient::new(Some("api-key".to_string())).timeout_settings(),
        expected
    );

    clear_timeout_env();
}
