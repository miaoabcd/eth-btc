use chrono::{TimeZone, Utc};

use eth_btc_strategy::logging::{
    Alert, AlertChannel, AlertError, AlertHttpClient, AlertLevel, AlertResponse, RetryPolicy,
    WebhookChannel,
};

#[derive(Clone)]
struct MockHttpClient {
    responses: std::sync::Arc<std::sync::Mutex<Vec<AlertResponse>>>,
}

#[test]
fn retry_policy_has_default() {
    let policy = RetryPolicy::default();
    assert!(policy.max_attempts >= 1);
}

#[async_trait::async_trait]
impl AlertHttpClient for MockHttpClient {
    async fn post(&self, _url: &str, _payload: &str) -> Result<AlertResponse, AlertError> {
        let mut guard = self.responses.lock().unwrap();
        if guard.is_empty() {
            return Err(AlertError::Transient("no response".into()));
        }
        Ok(guard.remove(0))
    }
}

#[tokio::test]
async fn webhook_retries_on_transient_failure() {
    let responses = vec![
        AlertResponse {
            status: 500,
            body: "".into(),
        },
        AlertResponse {
            status: 200,
            body: "ok".into(),
        },
    ];
    let client = MockHttpClient {
        responses: std::sync::Arc::new(std::sync::Mutex::new(responses)),
    };

    let channel = WebhookChannel::new(
        "http://localhost".to_string(),
        RetryPolicy::fast(),
        Box::new(client),
    );
    let alert = Alert {
        level: AlertLevel::Warning,
        message: "test".to_string(),
        timestamp: Utc.timestamp_opt(0, 0).unwrap(),
    };

    channel.send(alert).await.unwrap();
}
