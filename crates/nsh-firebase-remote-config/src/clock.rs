//! The clock the fetch timestamps read. Production gets
//! [`SystemClock`]; tests get [`ManualClock`], so the last-fetch stamp is
//! deterministic and no real time passes.

use std::sync::atomic::{AtomicU64, Ordering};

/// Milliseconds since the Unix epoch, as the fetch stamps see them.
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
    /// unknowingly un-stamp a fetch.
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
        let clock = ManualClock::new(2_000);
        assert_eq!(clock.now_ms(), 2_000);
        clock.advance(250);
        assert_eq!(clock.now_ms(), 2_250);
    }

    #[test]
    fn the_system_clock_is_past_2020() {
        assert!(SystemClock.now_ms() > 1_577_836_800_000);
    }
}
