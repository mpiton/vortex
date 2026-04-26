//! Pure state machine that tracks the set of currently-active downloads.
//!
//! Consumed by the tray animator: when the count of active downloads goes
//! 0 → ≥1 the animator starts cycling frames; when it returns to 0 it
//! restores the static icon. The tracker never touches the tray itself —
//! the adapter wires it to the EventBus and to a frame-swap callback.

use std::collections::HashSet;

use crate::domain::event::DomainEvent;
use crate::domain::model::download::DownloadId;

/// State transition produced by [`ActivityTracker::apply`].
///
/// Lets the animator decide whether to start the interval task, stop it,
/// or do nothing on a given event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// Active count went from 0 to ≥1.
    Activated,
    /// Active count returned to 0.
    Deactivated,
    /// Count changed but stayed in the same activity bucket, or the event
    /// didn't affect the count at all.
    NoChange,
}

#[derive(Debug, Default)]
pub struct ActivityTracker {
    active: HashSet<DownloadId>,
}

impl ActivityTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        !self.active.is_empty()
    }

    #[cfg(test)]
    pub(super) fn active_count(&self) -> usize {
        self.active.len()
    }

    /// Update internal state from a domain event and report the resulting
    /// activity transition.
    pub fn apply(&mut self, event: &DomainEvent) -> Transition {
        let was_active = self.is_active();
        match event {
            DomainEvent::DownloadStarted { id }
            | DomainEvent::DownloadResumed { id }
            | DomainEvent::DownloadResumedFromWait { id } => {
                self.active.insert(*id);
            }
            DomainEvent::DownloadPaused { id }
            | DomainEvent::DownloadCompleted { id }
            | DomainEvent::DownloadCompletedPersisted { id }
            | DomainEvent::DownloadFailed { id, .. }
            | DomainEvent::DownloadCancelled { id }
            | DomainEvent::DownloadRemoved { id }
            | DomainEvent::DownloadWaiting { id } => {
                self.active.remove(id);
            }
            _ => {}
        }
        match (was_active, self.is_active()) {
            (false, true) => Transition::Activated,
            (true, false) => Transition::Deactivated,
            _ => Transition::NoChange,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started(id: u64) -> DomainEvent {
        DomainEvent::DownloadStarted { id: DownloadId(id) }
    }
    fn paused(id: u64) -> DomainEvent {
        DomainEvent::DownloadPaused { id: DownloadId(id) }
    }
    fn completed(id: u64) -> DomainEvent {
        DomainEvent::DownloadCompletedPersisted { id: DownloadId(id) }
    }

    #[test]
    fn test_new_tracker_is_inactive() {
        let t = ActivityTracker::new();
        assert!(!t.is_active());
        assert_eq!(t.active_count(), 0);
    }

    #[test]
    fn test_started_event_activates_tracker() {
        let mut t = ActivityTracker::new();
        assert_eq!(t.apply(&started(1)), Transition::Activated);
        assert!(t.is_active());
        assert_eq!(t.active_count(), 1);
    }

    #[test]
    fn test_second_started_event_keeps_state_active_no_transition() {
        let mut t = ActivityTracker::new();
        t.apply(&started(1));
        assert_eq!(t.apply(&started(2)), Transition::NoChange);
        assert_eq!(t.active_count(), 2);
    }

    #[test]
    fn test_paused_when_only_active_deactivates() {
        let mut t = ActivityTracker::new();
        t.apply(&started(1));
        assert_eq!(t.apply(&paused(1)), Transition::Deactivated);
        assert!(!t.is_active());
    }

    #[test]
    fn test_paused_with_other_active_no_transition() {
        let mut t = ActivityTracker::new();
        t.apply(&started(1));
        t.apply(&started(2));
        assert_eq!(t.apply(&paused(1)), Transition::NoChange);
        assert_eq!(t.active_count(), 1);
        assert!(t.is_active());
    }

    #[test]
    fn test_completed_persisted_deactivates() {
        let mut t = ActivityTracker::new();
        t.apply(&started(7));
        assert_eq!(t.apply(&completed(7)), Transition::Deactivated);
    }

    #[test]
    fn test_failed_deactivates() {
        let mut t = ActivityTracker::new();
        t.apply(&started(3));
        let evt = DomainEvent::DownloadFailed {
            id: DownloadId(3),
            error: "boom".into(),
        };
        assert_eq!(t.apply(&evt), Transition::Deactivated);
    }

    #[test]
    fn test_cancelled_deactivates() {
        let mut t = ActivityTracker::new();
        t.apply(&started(8));
        let evt = DomainEvent::DownloadCancelled { id: DownloadId(8) };
        assert_eq!(t.apply(&evt), Transition::Deactivated);
    }

    #[test]
    fn test_resumed_from_wait_activates() {
        let mut t = ActivityTracker::new();
        let evt = DomainEvent::DownloadResumedFromWait { id: DownloadId(2) };
        assert_eq!(t.apply(&evt), Transition::Activated);
    }

    #[test]
    fn test_waiting_deactivates_when_last() {
        let mut t = ActivityTracker::new();
        t.apply(&started(4));
        let evt = DomainEvent::DownloadWaiting { id: DownloadId(4) };
        assert_eq!(t.apply(&evt), Transition::Deactivated);
    }

    #[test]
    fn test_progress_event_does_not_change_state() {
        let mut t = ActivityTracker::new();
        t.apply(&started(1));
        let evt = DomainEvent::DownloadProgress {
            id: DownloadId(1),
            downloaded_bytes: 50,
            total_bytes: 100,
        };
        assert_eq!(t.apply(&evt), Transition::NoChange);
        assert_eq!(t.active_count(), 1);
    }

    #[test]
    fn test_unrelated_event_is_ignored() {
        let mut t = ActivityTracker::new();
        let evt = DomainEvent::SettingsUpdated;
        assert_eq!(t.apply(&evt), Transition::NoChange);
        assert!(!t.is_active());
    }

    #[test]
    fn test_double_pause_for_unknown_id_is_safe() {
        let mut t = ActivityTracker::new();
        assert_eq!(t.apply(&paused(99)), Transition::NoChange);
        assert!(!t.is_active());
    }

    #[test]
    fn test_started_then_started_same_id_dedupes() {
        let mut t = ActivityTracker::new();
        t.apply(&started(1));
        t.apply(&started(1));
        assert_eq!(t.active_count(), 1);
        assert_eq!(t.apply(&paused(1)), Transition::Deactivated);
    }
}
