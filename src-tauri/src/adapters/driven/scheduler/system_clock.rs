//! `Clock` implementation backed by `std::time::SystemTime`.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::ports::driven::Clock;

/// Reads wall-clock time from the host operating system.
///
/// Falls back to `0` if the system clock is set before the Unix epoch
/// (only possible on a misconfigured machine). Domain consumers treat
/// `0` the same as any past timestamp, so this never produces unsafe
/// behaviour — it just disables the "skip purge if just ran" guard
/// until the clock is fixed.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix_secs(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_clock_returns_post_2026_timestamp() {
        // 2026-01-01T00:00:00Z = 1_767_225_600s. The build host clock
        // could only fall before this if it is wildly misconfigured,
        // and we accept that as out-of-scope for this guard.
        let clock = SystemClock;
        assert!(clock.now_unix_secs() >= 1_767_225_600);
    }

    #[test]
    fn test_system_clock_is_monotonic_within_call() {
        let clock = SystemClock;
        let a = clock.now_unix_secs();
        let b = clock.now_unix_secs();
        assert!(b >= a);
    }
}
