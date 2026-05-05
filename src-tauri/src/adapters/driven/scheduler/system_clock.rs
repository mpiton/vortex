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

    fn now_unix_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_clock_returns_value_within_observed_window() {
        // SystemTime is wall-clock and not monotonic, so we can't rely on
        // an absolute threshold (frozen CI clocks) or pairwise ordering
        // (NTP step-backs). Instead, sandwich the call between two direct
        // SystemTime reads and assert the observed value falls in that
        // window.
        let clock = SystemClock;
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let observed = clock.now_unix_secs();
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        assert!(
            before <= observed && observed <= after,
            "expected {before} <= {observed} <= {after}"
        );
    }
}
