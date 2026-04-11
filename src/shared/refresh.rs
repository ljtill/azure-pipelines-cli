//! Periodic refresh state tracking for data polling.

use std::time::{Duration, Instant};

/// Tracks in-flight state, failure count, and exponential backoff for a periodic
/// async refresh cycle (data refresh, log refresh, etc.).
#[derive(Debug, Default)]
pub struct RefreshState {
    pub in_flight: bool,
    pub failures: u32,
    pub backoff_until: Option<Instant>,
}

impl RefreshState {
    /// Marks a refresh as started. Returns `false` if one is already in flight.
    pub fn start(&mut self) -> bool {
        if self.in_flight {
            return false;
        }
        self.in_flight = true;
        true
    }

    /// Records a successful completion — resets failure count and backoff.
    pub fn succeed(&mut self) {
        self.in_flight = false;
        self.failures = 0;
        self.backoff_until = None;
    }

    /// Records a failed completion — increments failure count and sets backoff.
    pub fn fail(&mut self, base_secs: u64, max_secs: u64) {
        self.in_flight = false;
        self.failures = self.failures.saturating_add(1);
        let backoff = refresh_backoff(self.failures, base_secs, max_secs);
        self.backoff_until = Some(Instant::now() + backoff);
    }

    /// Checks whether the backoff period has elapsed (or was never set).
    pub fn backoff_elapsed(&self) -> bool {
        self.backoff_until
            .map(|until| Instant::now() >= until)
            .unwrap_or(true)
    }
}

/// Computes exponential backoff: `base_secs * 2^(failures-1)`, clamped to `max_secs`.
pub fn refresh_backoff(failures: u32, base_secs: u64, max_secs: u64) -> Duration {
    let shift = failures.saturating_sub(1).min(6);
    let multiplier = 1u64 << shift;
    Duration::from_secs(base_secs.saturating_mul(multiplier).min(max_secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_state_start_prevents_double_start() {
        let mut rs = RefreshState::default();
        assert!(rs.start());
        assert!(!rs.start()); // Already in flight.
    }

    #[test]
    fn refresh_state_succeed_resets() {
        let mut rs = RefreshState::default();
        rs.start();
        rs.fail(30, 300);
        assert!(rs.failures > 0);
        rs.start();
        rs.succeed();
        assert_eq!(rs.failures, 0);
        assert!(rs.backoff_until.is_none());
        assert!(!rs.in_flight);
    }

    #[test]
    fn backoff_scales_exponentially() {
        assert_eq!(refresh_backoff(0, 30, 300), Duration::from_secs(30));
        assert_eq!(refresh_backoff(1, 30, 300), Duration::from_secs(30));
        assert_eq!(refresh_backoff(2, 30, 300), Duration::from_secs(60));
        assert_eq!(refresh_backoff(3, 30, 300), Duration::from_secs(120));
        assert_eq!(refresh_backoff(4, 30, 300), Duration::from_secs(240));
        assert_eq!(refresh_backoff(5, 30, 300), Duration::from_secs(300)); // Clamped.
    }
}
