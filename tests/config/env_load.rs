use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use eth_btc_strategy::config::{PriceField, load_config};
use once_cell::sync::Lazy;
use rust_decimal_macros::dec;
use uuid::Uuid;

static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn temp_toml_path() -> PathBuf {
    let filename = format!("config-{}.toml", Uuid::new_v4());
    env::temp_dir().join(filename)
}

fn clear_env(keys: &[&str]) {
    for key in keys {
        unsafe {
            env::remove_var(key);
        }
    }
}

#[test]
fn load_merges_defaults_file_and_env() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env_keys = ["STRATEGY_ENTRY_Z", "STRATEGY_N_Z", "STRATEGY_PRICE_FIELD"];
    clear_env(&env_keys);

    let path = temp_toml_path();
    let toml = r#"
[strategy]
entry_z = 1.6
sl_z = 4.0
n_z = 300

[data]
price_field = "CLOSE"
"#;
    fs::write(&path, toml).unwrap();

    unsafe {
        env::set_var("STRATEGY_ENTRY_Z", "1.7");
        env::set_var("STRATEGY_N_Z", "400");
        env::set_var("STRATEGY_PRICE_FIELD", "MARK");
    }

    let config = load_config(Some(&path)).unwrap();

    assert_eq!(config.strategy.entry_z, dec!(1.7));
    assert_eq!(config.strategy.n_z, 400);
    assert_eq!(config.strategy.sl_z, dec!(4.0));
    assert_eq!(config.data.price_field, PriceField::Mark);

    clear_env(&env_keys);
    fs::remove_file(&path).unwrap();
}

#[test]
fn load_reads_runtime_and_auth_from_toml() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env_keys = [
        "HYPERLIQUID_PRIVATE_KEY",
        "STRATEGY_PRIVATE_KEY",
        "STRATEGY_API_KEY",
        "HYPERLIQUID_VAULT_ADDRESS",
        "HYPERLIQUID_VAULT",
    ];
    clear_env(&env_keys);

    let path = temp_toml_path();
    let toml = r#"
[runtime]
base_url = "http://localhost:3000"
interval_secs = 120
once = true
paper = true
disable_funding = true
state_path = "state.sqlite"

[auth]
private_key = "toml-key"
vault_address = "0xtoml"
"#;
    fs::write(&path, toml).unwrap();

    let config = load_config(Some(&path)).unwrap();

    assert_eq!(config.runtime.base_url, "http://localhost:3000");
    assert_eq!(config.runtime.interval_secs, 120);
    assert!(config.runtime.once);
    assert!(config.runtime.paper);
    assert!(config.runtime.disable_funding);
    assert_eq!(config.runtime.state_path.as_deref(), Some("state.sqlite"));
    assert_eq!(config.auth.private_key.as_deref(), Some("toml-key"));
    assert_eq!(config.auth.vault_address.as_deref(), Some("0xtoml"));

    fs::remove_file(&path).unwrap();
}

#[test]
fn env_auth_overrides_toml() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env_keys = [
        "HYPERLIQUID_PRIVATE_KEY",
        "STRATEGY_PRIVATE_KEY",
        "STRATEGY_API_KEY",
        "HYPERLIQUID_VAULT_ADDRESS",
        "HYPERLIQUID_VAULT",
    ];
    clear_env(&env_keys);

    let path = temp_toml_path();
    let toml = r#"
[auth]
private_key = "toml-key"
vault_address = "0xtoml"
"#;
    fs::write(&path, toml).unwrap();

    unsafe {
        env::set_var("HYPERLIQUID_PRIVATE_KEY", "env-key");
        env::set_var("STRATEGY_PRIVATE_KEY", "env-fallback");
        env::set_var("STRATEGY_API_KEY", "env-legacy");
        env::set_var("HYPERLIQUID_VAULT_ADDRESS", "0xenv");
    }

    let config = load_config(Some(&path)).unwrap();

    assert_eq!(config.auth.private_key.as_deref(), Some("env-key"));
    assert_eq!(config.auth.vault_address.as_deref(), Some("0xenv"));

    clear_env(&env_keys);
    fs::remove_file(&path).unwrap();
}

#[test]
fn load_fails_on_invalid_config() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env_keys = ["STRATEGY_ENTRY_Z", "STRATEGY_SL_Z"];
    clear_env(&env_keys);

    let path = temp_toml_path();
    let toml = r#"
[strategy]
entry_z = 3.6
sl_z = 3.5
"#;
    fs::write(&path, toml).unwrap();

    let result = load_config(Some(&path));
    assert!(result.is_err());

    fs::remove_file(&path).unwrap();
}
