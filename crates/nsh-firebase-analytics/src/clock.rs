//! The clock the queue's scheduler reads. Production gets
//! [`SystemClock`]; tests get [`ManualClock`], so flush intervals and
//! retry backoffs are deterministic and no real time passes.

use std::sync::atomic::{AtomicU64, Ordering};

/// Milliseconds since the Unix epoch, as the scheduler sees them.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

/// The real clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map(|since| since.as_millis() as u64)
            .unwrap_or_default()
    }
}

/// A clock the test moves: starts at `start_ms`, moved by [`Self::advance`].
pub struct ManualClock(AtomicU64);

impl ManualClock {
    pub fn new(start_ms: u64) -> Self {
        Self(AtomicU64::new(start_ms))
    }

    /// Moves the clock forward; backward is refused so a test cannot
    /// unknowingly un-schedule a retry.
    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }

    pub fn now(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manual_clock_moves_only_when_advanced() {
        let clock = ManualClock::new(1_000);
        assert_eq!(clock.now_ms(), 1_000);
        clock.advance(500);
        assert_eq!(clock.now_ms(), 1_500);
    }

    #[test]
    fn the_system_clock_is_past_2020() {
        assert!(SystemClock.now_ms() > 1_577_836_800_000);
    }
}
