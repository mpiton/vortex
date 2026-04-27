//! Burst aggregation for desktop completion notifications.
//!
//! Rule (PRD §7.5 / task 19): if **≥3** completion notifications fire within
//! a **5 s** sliding window, the third one becomes an aggregated
//! "N downloads completed" summary and any further completions in the
//! same burst are suppressed until the window drains.
//!
//! Pure: works off externally supplied epoch seconds so tests are
//! deterministic and the bridge can be driven by either real time or a
//! fake clock.

use std::collections::VecDeque;

/// Sliding window (seconds) used to detect bursts of completions.
pub const GROUPING_WINDOW_SECS: u64 = 5;

/// Number of completions in the window that triggers aggregation.
pub const GROUPING_THRESHOLD: usize = 3;

/// What the bridge should do with a completion event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationDecision {
    /// Show a normal per-download notification.
    ShowSingle,
    /// Show an aggregated "N downloads completed" notification.
    /// `count` is the total number of completions in the current burst,
    /// including the one that just fired.
    ShowAggregated { count: usize },
    /// Drop this completion silently — the burst is already represented
    /// by an aggregated notification within the same window.
    Suppress,
}

/// Stateful debouncer for completion notifications.
///
/// Keeps a fixed-capacity history of the recent completion timestamps
/// and decides per call whether to render single, aggregated, or
/// nothing. Reset is implicit: once `now` advances past the oldest
/// timestamp + `window`, that entry drops from the queue and the burst
/// effectively ends.
#[derive(Debug)]
pub struct NotificationGrouper {
    window_secs: u64,
    threshold: usize,
    /// Timestamps of completions still relevant to the current window.
    /// Kept ordered ascending; pruned on every `record`.
    timestamps: VecDeque<u64>,
    /// Set to `true` after we emit an aggregated notification for the
    /// current burst so subsequent completions in the same window do
    /// not trigger another aggregated notification (would spam the OS).
    burst_aggregated: bool,
}

impl NotificationGrouper {
    /// Create a grouper with the PRD defaults (5 s window, threshold 3).
    pub fn new() -> Self {
        Self::with_params(GROUPING_WINDOW_SECS, GROUPING_THRESHOLD)
    }

    /// Create a grouper with custom window and threshold (used by tests).
    pub fn with_params(window_secs: u64, threshold: usize) -> Self {
        Self {
            window_secs,
            threshold,
            timestamps: VecDeque::new(),
            burst_aggregated: false,
        }
    }

    /// Record a completion that occurred at `now_secs` (Unix epoch
    /// seconds) and return what the bridge should do.
    ///
    /// Pure with respect to the supplied clock — the same input
    /// sequence yields the same decisions.
    pub fn record(&mut self, now_secs: u64) -> NotificationDecision {
        // Wall-clock backwards jump (NTP step, manual time change):
        // entries timestamped in the "future" would never be pruned by
        // the new clock and would silently bias every burst decision.
        // Drop them and start a fresh window from `now_secs`.
        if self.timestamps.back().is_some_and(|&back| back > now_secs) {
            self.timestamps.clear();
            self.burst_aggregated = false;
        }
        self.prune(now_secs);
        // Window completely drained → previous burst ends, reset flag.
        if self.timestamps.is_empty() {
            self.burst_aggregated = false;
        }
        self.timestamps.push_back(now_secs);

        if self.burst_aggregated {
            return NotificationDecision::Suppress;
        }
        if self.timestamps.len() >= self.threshold {
            self.burst_aggregated = true;
            return NotificationDecision::ShowAggregated {
                count: self.timestamps.len(),
            };
        }
        NotificationDecision::ShowSingle
    }

    /// Drop entries strictly older than `now - window`. `saturating_sub`
    /// guards against clock skew where `now < window_secs`. Entries at
    /// the exact window edge stay so a burst spanning `now=0..window`
    /// still aggregates correctly when threshold ≥ window+1 events fire.
    fn prune(&mut self, now_secs: u64) {
        let cutoff = now_secs.saturating_sub(self.window_secs);
        while let Some(&front) = self.timestamps.front() {
            if front < cutoff {
                self.timestamps.pop_front();
            } else {
                break;
            }
        }
    }
}

impl Default for NotificationGrouper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_event_returns_show_single() {
        let mut g = NotificationGrouper::new();
        assert_eq!(g.record(100), NotificationDecision::ShowSingle);
    }

    #[test]
    fn test_two_events_within_window_both_show_single() {
        let mut g = NotificationGrouper::new();
        assert_eq!(g.record(100), NotificationDecision::ShowSingle);
        assert_eq!(g.record(102), NotificationDecision::ShowSingle);
    }

    #[test]
    fn test_third_event_within_window_returns_aggregated_with_count_three() {
        let mut g = NotificationGrouper::new();
        g.record(100);
        g.record(101);
        assert_eq!(
            g.record(102),
            NotificationDecision::ShowAggregated { count: 3 }
        );
    }

    #[test]
    fn test_fourth_and_subsequent_in_burst_are_suppressed() {
        let mut g = NotificationGrouper::new();
        g.record(100);
        g.record(101);
        g.record(102); // aggregated
        assert_eq!(g.record(103), NotificationDecision::Suppress);
        assert_eq!(g.record(104), NotificationDecision::Suppress);
    }

    #[test]
    fn test_burst_resets_after_window_drains() {
        let mut g = NotificationGrouper::new();
        g.record(100);
        g.record(101);
        g.record(102); // aggregated
        // 6 s later — window of 5 s elapsed (cutoff = 108 - 5 = 103),
        // entries at 100/101/102 are now strictly <= cutoff and pruned.
        assert_eq!(g.record(108), NotificationDecision::ShowSingle);
    }

    #[test]
    fn test_two_consecutive_bursts_each_get_one_aggregated() {
        let mut g = NotificationGrouper::new();
        // Burst 1
        g.record(100);
        g.record(101);
        let agg1 = g.record(102);
        // Window drained
        // Burst 2
        let s1 = g.record(200);
        let s2 = g.record(201);
        let agg2 = g.record(202);
        assert_eq!(agg1, NotificationDecision::ShowAggregated { count: 3 });
        assert_eq!(s1, NotificationDecision::ShowSingle);
        assert_eq!(s2, NotificationDecision::ShowSingle);
        assert_eq!(agg2, NotificationDecision::ShowAggregated { count: 3 });
    }

    #[test]
    fn test_count_grows_when_multiple_events_share_window_before_aggregation() {
        // With a higher threshold, the aggregated count must reflect
        // the actual number of pending entries, not a fixed value.
        let mut g = NotificationGrouper::with_params(10, 4);
        g.record(0);
        g.record(1);
        g.record(2);
        assert_eq!(
            g.record(3),
            NotificationDecision::ShowAggregated { count: 4 }
        );
    }

    #[test]
    fn test_event_just_outside_window_does_not_count_toward_burst() {
        let mut g = NotificationGrouper::new();
        g.record(100);
        g.record(101);
        // 6 s after first → first event has dropped (cutoff = 106-5 = 101,
        // entry at 100 is <= cutoff), only the entry at 101 remains. The
        // new entry brings count to 2 → still ShowSingle.
        assert_eq!(g.record(106), NotificationDecision::ShowSingle);
    }

    #[test]
    fn test_clock_smaller_than_window_does_not_panic() {
        let mut g = NotificationGrouper::new();
        // saturating_sub guards against `now < window`.
        assert_eq!(g.record(0), NotificationDecision::ShowSingle);
        assert_eq!(g.record(1), NotificationDecision::ShowSingle);
    }

    #[test]
    fn test_default_constructor_matches_prd_constants() {
        let g = NotificationGrouper::new();
        assert_eq!(g.window_secs, GROUPING_WINDOW_SECS);
        assert_eq!(g.threshold, GROUPING_THRESHOLD);
    }

    #[test]
    fn test_backwards_clock_jump_resets_window_and_returns_show_single() {
        let mut g = NotificationGrouper::new();
        // Build up a burst at t=1000 — third event would be aggregated.
        g.record(1000);
        g.record(1001);
        let agg = g.record(1002);
        assert_eq!(agg, NotificationDecision::ShowAggregated { count: 3 });
        // Clock steps backwards (NTP correction). Stale "future" entries
        // must be dropped so the next event starts a fresh window.
        let after_jump = g.record(500);
        assert_eq!(after_jump, NotificationDecision::ShowSingle);
        // And the burst flag must reset so a new burst can aggregate.
        g.record(501);
        assert_eq!(
            g.record(502),
            NotificationDecision::ShowAggregated { count: 3 }
        );
    }
}
