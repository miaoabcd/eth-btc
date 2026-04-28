use serde_json::json;

use eth_btc_strategy::logging::{redact_json_value, redact_wallet_addresses};

#[test]
fn redaction_masks_sensitive_fields() {
    let value = json!({
        "api_key": "secret",
        "nested": { "token": "abcd" },
        "normal": "ok"
    });

    let redacted = redact_json_value(&value);
    assert_eq!(redacted["api_key"], "***");
    assert_eq!(redacted["nested"]["token"], "***");
    assert_eq!(redacted["normal"], "ok");
}

#[test]
fn redaction_masks_wallet_addresses_in_strings() {
    let value = "User or API Wallet 0x1234567890abcdef1234567890abcdef12345678 does not exist";

    let redacted = redact_wallet_addresses(value);

    assert_eq!(
        redacted,
        "User or API Wallet 0x[REDACTED] does not exist"
    );
}

#[test]
fn redaction_masks_wallet_addresses_inside_json_strings() {
    let value = json!({
        "run_error": "wallet 0x1234567890abcdef1234567890abcdef12345678 failed"
    });

    let redacted = redact_json_value(&value);

    assert_eq!(redacted["run_error"], "wallet 0x[REDACTED] failed");
}
