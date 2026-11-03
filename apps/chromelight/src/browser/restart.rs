//! [`RestartPolicy`]: how often the browser may restart a failing gpu (spec §5).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Most gpu restarts within any [`RESTART_WINDOW`]; the next failure is final (no frame).
pub const GPU_RESTARTS_PER_MINUTE: u32 = 3;

/// The sliding window [`GPU_RESTARTS_PER_MINUTE`] counts over.
pub const RESTART_WINDOW: Duration = Duration::from_mins(1);

/// Remembers the recent restarts.
#[derive(Debug, Default)]
pub struct RestartPolicy {
    /// When each allowed restart happened, oldest first; never more than
    /// [`GPU_RESTARTS_PER_MINUTE`] entries.
    recent: VecDeque<Instant>,
}

impl RestartPolicy {
    /// Whether a restart at `now` is allowed; an allowed one is recorded, a refused one is not.
    pub fn allow(&mut self, now: Instant) -> bool {
        while self
            .recent
            .front()
            .is_some_and(|&t| now.saturating_duration_since(t) >= RESTART_WINDOW)
        {
            self.recent.pop_front();
        }
        if self.recent.len() >= usize::try_from(GPU_RESTARTS_PER_MINUTE).unwrap_or(usize::MAX) {
            return false;
        }
        self.recent.push_back(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{GPU_RESTARTS_PER_MINUTE, RestartPolicy};

    #[test]
    fn three_restarts_a_minute_are_allowed_and_the_fourth_is_not() {
        let t0 = Instant::now();
        let mut policy = RestartPolicy::default();
        for i in 0..GPU_RESTARTS_PER_MINUTE {
            assert!(policy.allow(t0 + Duration::from_secs(u64::from(i))));
        }
        assert!(!policy.allow(t0 + Duration::from_secs(10)));
    }

    #[test]
    fn a_restart_older_than_a_minute_no_longer_counts() {
        let t0 = Instant::now();
        let mut policy = RestartPolicy::default();
        for _ in 0..GPU_RESTARTS_PER_MINUTE {
            assert!(policy.allow(t0));
        }
        assert!(!policy.allow(t0 + Duration::from_secs(59)));
        assert!(policy.allow(t0 + Duration::from_mins(1)));
    }

    #[test]
    fn a_refused_restart_is_not_counted() {
        let t0 = Instant::now();
        let mut policy = RestartPolicy::default();
        for _ in 0..GPU_RESTARTS_PER_MINUTE {
            assert!(policy.allow(t0));
        }
        for s in 1..50 {
            assert!(!policy.allow(t0 + Duration::from_secs(s)));
        }
        assert!(policy.allow(t0 + Duration::from_mins(1)));
    }
}
