//! Global token-bucket rate limiter shared by all download connections.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::time::Instant;

struct Bucket {
    tokens: f64,
    last: Instant,
}

pub struct RateLimiter {
    kbps: AtomicU64,
    bucket: Mutex<Bucket>,
}

impl RateLimiter {
    pub fn new(kbps: u64) -> Self {
        let burst = kbps as f64 * 1024.0;
        Self {
            kbps: AtomicU64::new(kbps),
            bucket: Mutex::new(Bucket {
                tokens: burst,
                last: Instant::now(),
            }),
        }
    }

    pub fn set_kbps(&self, kbps: u64) {
        self.kbps.store(kbps, Ordering::Relaxed);
    }

    pub fn kbps(&self) -> u64 {
        self.kbps.load(Ordering::Relaxed)
    }

    pub async fn throttle(&self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        loop {
            let kbps = self.kbps();
            if kbps == 0 {
                return;
            }
            let rate = kbps as f64 * 1024.0;
            let wait = {
                let mut bucket = self.bucket.lock().unwrap();
                let now = Instant::now();
                let elapsed = now.duration_since(bucket.last).as_secs_f64();
                bucket.tokens = (bucket.tokens + elapsed * rate).min(rate);
                bucket.last = now;
                if bucket.tokens >= bytes as f64 {
                    bucket.tokens -= bytes as f64;
                    return;
                }
                Duration::from_secs_f64((bytes as f64 - bucket.tokens) / rate)
            };
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RateLimiter;
    use std::time::Duration;

    #[tokio::test(start_paused = true)]
    async fn unlimited_never_waits() {
        let limiter = RateLimiter::new(0);
        let start = tokio::time::Instant::now();
        limiter.throttle(10 * 1024 * 1024).await;
        assert!(start.elapsed().is_zero());
    }

    #[tokio::test(start_paused = true)]
    async fn throttle_waits_for_tokens() {
        let limiter = RateLimiter::new(1); // 1 KB/s, one second of burst
        let start = tokio::time::Instant::now();
        limiter.throttle(1024).await; // consumes the initial burst immediately
        limiter.throttle(512).await;
        assert!(start.elapsed() >= Duration::from_millis(400));
    }

    #[test]
    fn set_kbps_applies_live() {
        let limiter = RateLimiter::new(64);
        assert_eq!(limiter.kbps(), 64);
        limiter.set_kbps(0);
        assert_eq!(limiter.kbps(), 0);
    }
}
