//! Injectable clock. Engine code never calls `Instant::now()` directly, so tests are deterministic
//! (see `docs/CODING_STANDARDS.md` §8 — no `sleep` in tests).

use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// Source of monotonic time.
pub trait Clock: Send + Sync {
    /// Current monotonic instant.
    fn now(&self) -> Instant;
}

/// Real monotonic clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

/// Manually advanced clock for tests.
#[derive(Debug)]
pub struct FakeClock {
    now: Mutex<Instant>,
}

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

impl FakeClock {
    /// A fake clock starting at an arbitrary fixed instant.
    #[must_use]
    pub fn new() -> Self {
        Self {
            now: Mutex::new(Instant::now()),
        }
    }

    /// Move the clock forward.
    pub fn advance(&self, by: Duration) {
        let mut now = self.now.lock();
        *now += by;
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        *self.now.lock()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_should_not_move_without_advance() {
        let clock = FakeClock::new();
        let a = clock.now();
        let b = clock.now();
        assert_eq!(a, b);
    }

    #[test]
    fn fake_clock_should_move_by_exactly_advanced_duration() {
        let clock = FakeClock::new();
        let a = clock.now();
        clock.advance(Duration::from_millis(250));
        assert_eq!(clock.now() - a, Duration::from_millis(250));
    }

    #[test]
    fn system_clock_should_be_monotonic() {
        let clock = SystemClock;
        let a = clock.now();
        let b = clock.now();
        assert!(b >= a);
    }
}
