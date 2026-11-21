use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub(super) struct SpeedTracker {
    samples: VecDeque<(Instant, u64)>,
    last_bytes: u64,
    last_activity: Instant,
    last_decay: Option<Instant>,
}

impl SpeedTracker {
    const WINDOW: Duration = Duration::from_secs(4);
    const MIN_SAMPLE_INTERVAL: Duration = Duration::from_millis(250);
    const MIN_SPEED_SPAN: Duration = Duration::from_millis(500);
    const STALL_GRACE: Duration = Duration::from_secs(3);
    const DECAY_INTERVAL: Duration = Duration::from_secs(1);
    const STALL_ZERO_AFTER: Duration = Duration::from_secs(10);

    pub(super) fn new(now: Instant, done: u64) -> Self {
        let mut samples = VecDeque::with_capacity(20);
        samples.push_back((now, done));
        Self {
            samples,
            last_bytes: done,
            last_activity: now,
            last_decay: None,
        }
    }

    pub(super) fn record_at(
        &mut self,
        now: Instant,
        done: u64,
        previous_speed: Option<u64>,
    ) -> Option<u64> {
        if done < self.last_bytes {
            self.samples.clear();
            self.samples.push_back((now, done));
            self.last_bytes = done;
            self.last_activity = now;
            self.last_decay = None;
            return None;
        }
        if done == self.last_bytes {
            return None;
        }
        self.last_bytes = done;
        self.last_activity = now;
        self.last_decay = None;

        if self
            .samples
            .back()
            .is_some_and(|(at, _)| now.duration_since(*at) < Self::MIN_SAMPLE_INTERVAL)
        {
            return None;
        }
        self.samples.push_back((now, done));
        while self.samples.len() > 1 && now.duration_since(self.samples[1].0) >= Self::WINDOW {
            self.samples.pop_front();
        }
        let (oldest_at, oldest_done) = *self.samples.front()?;
        let span = now.duration_since(oldest_at);
        if span < Self::MIN_SPEED_SPAN {
            return None;
        }
        let window_rate =
            ((done.saturating_sub(oldest_done)) as f64 / span.as_secs_f64()).round() as u64;
        Some(compute_smoothed_speed(previous_speed, window_rate))
    }

    pub(super) fn decay_at(&mut self, now: Instant, speed: &mut u64) -> bool {
        if *speed == 0 {
            return false;
        }
        let idle = now.duration_since(self.last_activity);
        if idle < Self::STALL_GRACE {
            return false;
        }
        if self
            .last_decay
            .is_some_and(|at| now.duration_since(at) < Self::DECAY_INTERVAL)
        {
            return false;
        }
        self.last_decay = Some(now);
        *speed = if *speed < 1024 || idle >= Self::STALL_ZERO_AFTER {
            0
        } else {
            *speed / 2
        };
        true
    }
}

fn compute_smoothed_speed(previous: Option<u64>, instant: u64) -> u64 {
    match previous {
        Some(prev) if prev > 0 => ((prev as f64 * 0.6) + (instant as f64 * 0.4)).round() as u64,
        _ => instant,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothed_speed_blends_samples_without_wild_spikes() {
        assert_eq!(compute_smoothed_speed(None, 10_000_000), 10_000_000);
        assert_eq!(
            compute_smoothed_speed(Some(10_000_000), 20_000_000),
            14_000_000
        );
    }

    #[test]
    fn speed_tracker_uses_sliding_window_across_bursts_and_short_pauses() {
        let t0 = Instant::now();
        let mut tracker = SpeedTracker::new(t0, 0);
        let mut speed = None;

        // 10 MB/s burst for 1s
        speed = tracker
            .record_at(t0 + Duration::from_millis(500), 5_000_000, speed)
            .or(speed);
        assert_eq!(speed, Some(10_000_000));

        speed = tracker
            .record_at(t0 + Duration::from_secs(1), 10_000_000, speed)
            .or(speed);
        assert_eq!(speed, Some(10_000_000));

        // 2s pause between chunks must not decay speed to zero
        let mut current = speed.unwrap();
        assert!(!tracker.decay_at(t0 + Duration::from_secs(3), &mut current));
        assert_eq!(current, 10_000_000);

        // Next chunk arrives after 2s gap: 15 MB over 3s window = 5 MB/s blended with 10 MB/s -> 8 MB/s
        let next = tracker.record_at(t0 + Duration::from_secs(3), 15_000_000, Some(current));
        assert_eq!(next, Some(8_000_000));
    }

    #[test]
    fn speed_tracker_decay_respects_grace_period_and_one_second_cooldown() {
        let t0 = Instant::now();
        let mut tracker = SpeedTracker::new(t0, 1_000_000);
        let mut speed = 10_000_000;

        // Still inside 3s grace period
        assert!(!tracker.decay_at(t0 + Duration::from_millis(2_500), &mut speed));
        assert_eq!(speed, 10_000_000);

        // First decay at 3s
        assert!(tracker.decay_at(t0 + Duration::from_secs(3), &mut speed));
        assert_eq!(speed, 5_000_000);

        // Rapid 25ms engine ticks must NOT collapse speed to 0
        for tick in 1..=20 {
            let at = t0 + Duration::from_secs(3) + Duration::from_millis(tick * 25);
            assert!(!tracker.decay_at(at, &mut speed));
        }
        assert_eq!(speed, 5_000_000);

        // Next decay allowed after 1s cooldown
        assert!(tracker.decay_at(t0 + Duration::from_secs(4), &mut speed));
        assert_eq!(speed, 2_500_000);

        // Prolonged 10s stall drops cleanly to 0
        assert!(tracker.decay_at(t0 + Duration::from_secs(10), &mut speed));
        assert_eq!(speed, 0);
    }

    #[test]
    fn speed_tracker_does_not_reset_stall_timer_on_unchanged_bytes() {
        let t0 = Instant::now();
        let mut tracker = SpeedTracker::new(t0, 5_000_000);
        let mut speed = 8_000_000;

        // Repeated identical progress lines during a stall must not keep the stall timer alive
        assert_eq!(
            tracker.record_at(t0 + Duration::from_secs(2), 5_000_000, Some(speed)),
            None
        );
        assert!(tracker.decay_at(t0 + Duration::from_secs(3), &mut speed));
        assert_eq!(speed, 4_000_000);
    }
}
