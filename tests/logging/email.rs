use chrono::{TimeZone, Utc};

use eth_btc_strategy::logging::{
    Alert, AlertChannel, AlertError, AlertLevel, EmailChannel, EmailTransport, NoopEmailTransport,
};

#[derive(Clone, Default)]
struct MockEmailTransport {
    sent: std::sync::Arc<std::sync::Mutex<u32>>,
}

#[async_trait::async_trait]
impl EmailTransport for MockEmailTransport {
    async fn send(&self, _subject: &str, _body: &str) -> Result<(), AlertError> {
        let mut guard = self.sent.lock().unwrap();
        *guard += 1;
        Ok(())
    }
}

#[tokio::test]
async fn email_channel_throttles_repeats() {
    let transport = MockEmailTransport::default();
    let channel = EmailChannel::new(transport.clone(), 60);

    let alert = Alert {
        level: AlertLevel::Critical,
        message: "critical".to_string(),
        timestamp: Utc.timestamp_opt(0, 0).unwrap(),
    };

    channel.send(alert.clone()).await.unwrap();
    let second = channel.send(alert).await.unwrap_err();
    assert!(matches!(second, AlertError::Throttled));
}

#[tokio::test]
async fn noop_email_transport_succeeds() {
    let transport = NoopEmailTransport::default();
    transport.send("subject", "body").await.unwrap();
}
