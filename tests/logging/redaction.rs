use serde_json::json;

use eth_btc_strategy::logging::redact_json_value;

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
