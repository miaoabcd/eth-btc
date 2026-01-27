use chrono::{TimeZone, Utc};

use eth_btc_strategy::logging::{Alert, AlertDispatcher, AlertLevel, InMemoryAlertChannel};

#[tokio::test]
async fn alert_dispatcher_fans_out() {
    let channel = InMemoryAlertChannel::default();
    let dispatcher = AlertDispatcher::new(vec![std::sync::Arc::new(channel.clone())]);

    let alert = Alert {
        level: AlertLevel::Critical,
        message: "single-leg detected".to_string(),
        timestamp: Utc.timestamp_opt(0, 0).unwrap(),
    };

    dispatcher.send(alert).await.unwrap();
    let stored = channel.alerts();
    assert_eq!(stored.len(), 1);
}
