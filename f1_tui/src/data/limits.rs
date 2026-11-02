//! Sequential request pacing and cooperative cancellation shared by acquisition.
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn check(&self) -> Result<(), String> {
        if self.0.load(Ordering::Relaxed) {
            Err("Acquisition cancelled".into())
        } else {
            Ok(())
        }
    }
    pub(crate) fn wait(&self, duration: Duration) -> Result<(), String> {
        let start = Instant::now();
        loop {
            self.check()?;
            let remaining = duration.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return Ok(());
            }
            std::thread::sleep(remaining.min(Duration::from_millis(25)));
        }
    }
}

#[derive(Debug, Clone)]
pub struct Limits {
    /// 2.1 seconds stays below the free 30/minute limit for a single client.
    pub request_spacing: Duration,
    pub chunk_span: Duration,
    pub max_response_bytes: usize,
    pub max_rows: usize,
    /// Retries after the first attempt, not total attempts.
    pub max_retries: u32,
    pub retry_backoff: Duration,
    pub max_retry_wait: Duration,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            request_spacing: Duration::from_millis(2100),
            chunk_span: Duration::from_secs(300),
            max_response_bytes: 8 * 1024 * 1024,
            max_rows: 100_000,
            max_retries: 3,
            retry_backoff: Duration::from_secs(1),
            max_retry_wait: Duration::from_secs(60),
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(30),
        }
    }
}
impl Limits {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.request_spacing.is_zero()
            || self.chunk_span.as_millis() == 0
            || self.chunk_span > Duration::from_secs(3600)
            || self.max_response_bytes == 0
            || self.max_response_bytes > 64 * 1024 * 1024
            || self.max_rows == 0
            || self.max_rows > 100_000
            || self.max_retries > 8
            || self.retry_backoff.is_zero()
            || self.max_retry_wait.is_zero()
            || self.max_retry_wait > Duration::from_secs(300)
            || self.connect_timeout.is_zero()
            || self.request_timeout.is_zero()
            || self.connect_timeout > self.request_timeout
            || self.request_timeout > Duration::from_secs(300)
        {
            return Err("Invalid acquisition limits".into());
        }
        Ok(())
    }
}
