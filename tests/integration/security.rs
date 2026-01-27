use serde_json::json;

use eth_btc_strategy::logging::redact_json_value;

#[test]
fn security_audit_redacts_secrets() {
    let value = json!({"api_key": "secret", "token": "abc"});
    let redacted = redact_json_value(&value);
    assert_eq!(redacted["api_key"], "***");
    assert_eq!(redacted["token"], "***");
}
