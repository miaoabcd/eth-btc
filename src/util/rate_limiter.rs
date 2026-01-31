use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::{Instant, sleep};

#[async_trait::async_trait]
pub trait RateLimiter: Send + Sync {
    async fn wait(&self);
}

#[derive(Default, Clone)]
pub struct NoopRateLimiter;

#[async_trait::async_trait]
impl RateLimiter for NoopRateLimiter {
    async fn wait(&self) {}
}

#[derive(Clone)]
pub struct FixedRateLimiter {
    min_interval: Duration,
    last: Arc<Mutex<Option<Instant>>>,
}

impl FixedRateLimiter {
    pub fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last: Arc::new(Mutex::new(None)),
        }
    }

    pub fn disabled() -> Self {
        Self::new(Duration::from_millis(0))
    }
}

#[async_trait::async_trait]
impl RateLimiter for FixedRateLimiter {
    async fn wait(&self) {
        if self.min_interval.is_zero() {
            return;
        }
        let mut guard = self.last.lock().await;
        if let Some(last) = *guard {
            let elapsed = last.elapsed();
            if elapsed < self.min_interval {
                sleep(self.min_interval - elapsed).await;
            }
        }
        *guard = Some(Instant::now());
    }
}
