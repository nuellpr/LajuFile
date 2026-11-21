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
        if kbps == 0
            && let Ok(mut bucket) = self.bucket.lock()
        {
            bucket.tokens = bucket.tokens.max(0.0);
        }
    }

    pub fn kbps(&self) -> u64 {
        self.kbps.load(Ordering::Relaxed)
    }

    pub async fn throttle(&self, mut bytes: u64) {
        while bytes > 0 {
            let kbps = self.kbps();
            if kbps == 0 {
                return;
            }
            let rate = kbps as f64 * 1024.0;
            // Consume at most one second of tokens per pass so a chunk larger
            // than the bucket capacity can never wait forever.
            let slice = bytes.min(rate as u64).max(1);
            let wait = {
                let mut bucket = self.bucket.lock().unwrap();
                let now = Instant::now();
                let elapsed = now.duration_since(bucket.last).as_secs_f64();
                bucket.tokens = (bucket.tokens + elapsed * rate).min(rate);
                bucket.last = now;
                // Reserve tokens up front so concurrent connections queue FIFO instead of starving.
                bucket.tokens -= slice as f64;
                if bucket.tokens >= 0.0 {
                    None
                } else {
                    Some(Duration::from_secs_f64(-bucket.tokens / rate))
                }
            };
            if let Some(wait) = wait {
                tokio::time::sleep(wait).await;
            }
            bytes -= slice;
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

    #[tokio::test(start_paused = true)]
    async fn throttle_larger_than_one_second_of_limit_completes() {
        let limiter = RateLimiter::new(1); // 1 KB/s
        let start = tokio::time::Instant::now();
        tokio::time::timeout(Duration::from_secs(10), limiter.throttle(2048))
            .await
            .expect("throttle must not hang when bytes exceed the bucket capacity");
        assert!(start.elapsed() >= Duration::from_secs(1));
    }

    #[tokio::test(start_paused = true)]
    async fn lowering_limit_mid_throttle_completes() {
        let limiter = std::sync::Arc::new(RateLimiter::new(256));
        let worker = std::sync::Arc::clone(&limiter);
        let task = tokio::spawn(async move { worker.throttle(512 * 1024).await });
        tokio::task::yield_now().await;
        limiter.set_kbps(64);
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .expect("lowering the limit must not hang an in-flight throttle")
            .unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_waiters_complete_fairly_without_starvation() {
        let limiter = std::sync::Arc::new(RateLimiter::new(16)); // 16 KB/s
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let worker = std::sync::Arc::clone(&limiter);
            tasks.push(tokio::spawn(async move {
                worker.throttle(16 * 1024).await;
            }));
        }
        let start = tokio::time::Instant::now();
        for task in tasks {
            tokio::time::timeout(Duration::from_secs(12), task)
                .await
                .expect("no waiter may starve")
                .unwrap();
        }
        assert!(start.elapsed() >= Duration::from_secs(6));
    }
}
