//! Doubling back-off for a fetch tier that keeps failing.

pub const BACKOFF_BASE_SECS: i64 = 5 * 60;
pub const BACKOFF_CAP_SECS: i64 = 30 * 60;

/// Seconds to wait after `failures` consecutive failures: 5 min, doubling, capped at 30 min.
pub fn backoff_secs(failures: u32) -> i64 {
    if failures == 0 {
        return 0;
    }
    let doublings = (failures - 1).min(16);
    (BACKOFF_BASE_SECS << doublings).min(BACKOFF_CAP_SECS)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Backoff {
    failures: u32,
    retry_at: i64,
}

impl Backoff {
    pub fn allows(&self, now: i64) -> bool {
        now >= self.retry_at
    }

    pub fn failed(self, now: i64) -> Backoff {
        let failures = self.failures.saturating_add(1);
        Backoff {
            failures,
            retry_at: now.saturating_add(backoff_secs(failures)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000;

    #[test]
    fn delay_doubles_from_five_minutes_up_to_the_cap() {
        let cases = [
            (0, 0),
            (1, 5 * 60),
            (2, 10 * 60),
            (3, 20 * 60),
            (4, 30 * 60),
            (5, 30 * 60),
            (100, 30 * 60),
            (u32::MAX, 30 * 60),
        ];
        for (failures, secs) in cases {
            assert_eq!(backoff_secs(failures), secs, "failures = {failures}");
        }
    }

    #[test]
    fn fresh_backoff_allows_right_away() {
        assert!(Backoff::default().allows(NOW));
    }

    #[test]
    fn first_failure_blocks_for_five_minutes() {
        let b = Backoff::default().failed(NOW);
        assert!(!b.allows(NOW));
        assert!(!b.allows(NOW + 5 * 60 - 1));
        assert!(b.allows(NOW + 5 * 60));
    }

    #[test]
    fn repeated_failures_wait_longer_until_the_cap() {
        let mut b = Backoff::default();
        let mut now = NOW;
        for expected in [5, 10, 20, 30, 30] {
            b = b.failed(now);
            assert!(!b.allows(now + expected * 60 - 1));
            assert!(b.allows(now + expected * 60));
            now += expected * 60;
        }
    }
}
