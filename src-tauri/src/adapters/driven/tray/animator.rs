//! Drives the tray icon between a static state and the pulse animation.
//!
//! - [`AnimatorCore`] is the synchronous state machine that decides, for
//!   each domain event, whether to start cycling frames, stop and restore
//!   the static icon, or do nothing. It also advances the frame index on
//!   each tick. Pure, fully unit-tested.
//! - [`spawn_tray_animator`] wires the core to an [`EventBus`] and a tokio
//!   interval, calling an [`IconSwapper`] to actually swap the tray icon.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::interval;

use crate::adapters::driven::tray::activity_tracker::{ActivityTracker, Transition};
use crate::domain::event::DomainEvent;
use crate::domain::ports::driven::EventBus;

/// Default cycle period — 200ms keeps CPU low while still reading as motion.
pub const DEFAULT_FRAME_INTERVAL: Duration = Duration::from_millis(200);

/// Action the animator wants the runtime to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimatorAction {
    /// Begin cycling frames (active downloads detected).
    StartAnimation,
    /// Stop animation and restore the static icon.
    StopAnimation,
    /// State unchanged — no swap needed for this event.
    NoOp,
}

/// Trait abstraction over the tray icon so the loop is testable without
/// a real Tauri runtime.
pub trait IconSwapper: Send + Sync {
    /// Render the animation frame at the given index (modulo frame count).
    fn show_frame(&self, frame_index: usize);
    /// Restore the static (default) icon.
    fn show_static(&self);
}

/// Pure state machine: tracks active downloads and the current frame index.
#[derive(Debug)]
pub struct AnimatorCore {
    tracker: ActivityTracker,
    frame_count: usize,
    frame_index: usize,
    animating: bool,
}

impl AnimatorCore {
    pub fn new(frame_count: usize) -> Self {
        assert!(frame_count > 0, "frame_count must be ≥ 1");
        Self {
            tracker: ActivityTracker::new(),
            frame_count,
            frame_index: 0,
            animating: false,
        }
    }

    pub fn is_animating(&self) -> bool {
        self.animating
    }

    pub fn current_frame(&self) -> usize {
        self.frame_index
    }

    /// Process a domain event, return what the runtime should do.
    pub fn handle_event(&mut self, event: &DomainEvent) -> AnimatorAction {
        match self.tracker.apply(event) {
            Transition::Activated => {
                self.animating = true;
                self.frame_index = 0;
                AnimatorAction::StartAnimation
            }
            Transition::Deactivated => {
                self.animating = false;
                self.frame_index = 0;
                AnimatorAction::StopAnimation
            }
            Transition::NoChange => AnimatorAction::NoOp,
        }
    }

    /// Advance to the next frame and return its index, or `None` if the
    /// animator is currently idle.
    pub fn tick(&mut self) -> Option<usize> {
        if !self.animating {
            return None;
        }
        self.frame_index = (self.frame_index + 1) % self.frame_count;
        Some(self.frame_index)
    }
}

/// Wires [`AnimatorCore`] to an [`EventBus`] and a tokio interval.
///
/// Returns immediately. The spawned task lives for the duration of the
/// process; it stops cleanly when the EventBus is dropped (channel closes).
pub fn spawn_tray_animator(
    event_bus: &dyn EventBus,
    swapper: Arc<dyn IconSwapper>,
    frame_count: usize,
    frame_interval: Duration,
) {
    let (tx, mut rx) = mpsc::unbounded_channel::<DomainEvent>();

    event_bus.subscribe(Box::new(move |event: &DomainEvent| {
        if !is_relevant(event) {
            return;
        }
        let _ = tx.send(event.clone());
    }));

    tokio::spawn(async move {
        let mut core = AnimatorCore::new(frame_count);
        let mut tick = interval(frame_interval);
        // Skip the immediate first tick so we don't redraw the static icon.
        tick.tick().await;
        loop {
            tokio::select! {
                maybe_event = rx.recv() => {
                    let Some(event) = maybe_event else { break };
                    match core.handle_event(&event) {
                        AnimatorAction::StartAnimation => {
                            swapper.show_frame(core.current_frame());
                        }
                        AnimatorAction::StopAnimation => {
                            swapper.show_static();
                        }
                        AnimatorAction::NoOp => {}
                    }
                }
                _ = tick.tick(), if core.is_animating() => {
                    if let Some(idx) = core.tick() {
                        swapper.show_frame(idx);
                    }
                }
            }
        }
        // Ensure the tray returns to the static icon when shutting down.
        swapper.show_static();
    });
}

/// Filter out high-frequency events the animator doesn't need.
///
/// Progress events fire many times per second and would just clog the
/// channel — the tracker ignores them anyway.
fn is_relevant(event: &DomainEvent) -> bool {
    !matches!(
        event,
        DomainEvent::DownloadProgress { .. }
            | DomainEvent::SegmentStarted { .. }
            | DomainEvent::SegmentCompleted { .. }
            | DomainEvent::SegmentFailed { .. }
            | DomainEvent::SegmentSplit { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::driven::event::TokioEventBus;
    use crate::domain::model::download::DownloadId;
    use std::sync::Mutex;
    use std::time::Duration;

    #[derive(Default)]
    struct RecordingSwapper {
        events: Mutex<Vec<SwapEvent>>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum SwapEvent {
        Frame(usize),
        Static,
    }

    impl IconSwapper for RecordingSwapper {
        fn show_frame(&self, frame_index: usize) {
            self.events
                .lock()
                .unwrap()
                .push(SwapEvent::Frame(frame_index));
        }
        fn show_static(&self) {
            self.events.lock().unwrap().push(SwapEvent::Static);
        }
    }

    impl RecordingSwapper {
        fn snapshot(&self) -> Vec<SwapEvent> {
            self.events.lock().unwrap().clone()
        }
    }

    fn started(id: u64) -> DomainEvent {
        DomainEvent::DownloadStarted { id: DownloadId(id) }
    }

    fn paused(id: u64) -> DomainEvent {
        DomainEvent::DownloadPaused { id: DownloadId(id) }
    }

    #[test]
    fn test_new_core_is_not_animating() {
        let core = AnimatorCore::new(8);
        assert!(!core.is_animating());
        assert_eq!(core.current_frame(), 0);
    }

    #[test]
    #[should_panic(expected = "frame_count must be ≥ 1")]
    fn test_zero_frame_count_panics() {
        let _ = AnimatorCore::new(0);
    }

    #[test]
    fn test_started_returns_start_action() {
        let mut core = AnimatorCore::new(8);
        assert_eq!(
            core.handle_event(&started(1)),
            AnimatorAction::StartAnimation
        );
        assert!(core.is_animating());
    }

    #[test]
    fn test_paused_when_only_active_returns_stop_action() {
        let mut core = AnimatorCore::new(8);
        core.handle_event(&started(1));
        assert_eq!(core.handle_event(&paused(1)), AnimatorAction::StopAnimation);
        assert!(!core.is_animating());
    }

    #[test]
    fn test_second_started_returns_noop() {
        let mut core = AnimatorCore::new(8);
        core.handle_event(&started(1));
        assert_eq!(core.handle_event(&started(2)), AnimatorAction::NoOp);
    }

    #[test]
    fn test_progress_event_is_noop_and_does_not_change_frame() {
        let mut core = AnimatorCore::new(8);
        core.handle_event(&started(1));
        core.tick();
        let frame_before = core.current_frame();
        let evt = DomainEvent::DownloadProgress {
            id: DownloadId(1),
            downloaded_bytes: 50,
            total_bytes: 100,
        };
        assert_eq!(core.handle_event(&evt), AnimatorAction::NoOp);
        assert_eq!(core.current_frame(), frame_before);
    }

    #[test]
    fn test_tick_returns_none_when_idle() {
        let mut core = AnimatorCore::new(8);
        assert!(core.tick().is_none());
    }

    #[test]
    fn test_tick_advances_frame_when_active() {
        let mut core = AnimatorCore::new(8);
        core.handle_event(&started(1));
        assert_eq!(core.tick(), Some(1));
        assert_eq!(core.tick(), Some(2));
    }

    #[test]
    fn test_tick_wraps_around_frame_count() {
        let mut core = AnimatorCore::new(3);
        core.handle_event(&started(1));
        let frames: Vec<_> = (0..6).map(|_| core.tick().unwrap()).collect();
        assert_eq!(frames, vec![1, 2, 0, 1, 2, 0]);
    }

    #[test]
    fn test_resuming_after_stop_resets_frame_to_zero() {
        let mut core = AnimatorCore::new(8);
        core.handle_event(&started(1));
        core.tick();
        core.tick();
        core.handle_event(&paused(1));
        assert_eq!(core.current_frame(), 0);
        core.handle_event(&started(2));
        assert_eq!(core.current_frame(), 0);
    }

    #[test]
    fn test_is_relevant_filters_progress_and_segments() {
        assert!(!is_relevant(&DomainEvent::DownloadProgress {
            id: DownloadId(1),
            downloaded_bytes: 0,
            total_bytes: 1,
        }));
        assert!(!is_relevant(&DomainEvent::SegmentStarted {
            download_id: DownloadId(1),
            segment_id: 0,
            start_byte: 0,
            end_byte: 100,
        }));
        assert!(is_relevant(&started(1)));
        assert!(is_relevant(&paused(1)));
    }

    #[tokio::test]
    async fn test_spawn_animator_starts_animation_on_download_started() {
        let bus = TokioEventBus::new(16);
        let swapper = Arc::new(RecordingSwapper::default());
        spawn_tray_animator(
            &bus,
            swapper.clone() as Arc<dyn IconSwapper>,
            4,
            Duration::from_millis(50),
        );
        // Give the subscription task a moment to register.
        tokio::time::sleep(Duration::from_millis(10)).await;

        bus.publish(started(1));
        tokio::time::sleep(Duration::from_millis(180)).await;

        let snapshot = swapper.snapshot();
        // Must contain at least one frame swap.
        assert!(
            snapshot.iter().any(|e| matches!(e, SwapEvent::Frame(_))),
            "expected ≥1 frame swap, got {snapshot:?}",
        );
    }

    #[tokio::test]
    async fn test_spawn_animator_restores_static_when_all_paused() {
        let bus = TokioEventBus::new(16);
        let swapper = Arc::new(RecordingSwapper::default());
        spawn_tray_animator(
            &bus,
            swapper.clone() as Arc<dyn IconSwapper>,
            4,
            Duration::from_millis(50),
        );
        tokio::time::sleep(Duration::from_millis(10)).await;

        bus.publish(started(1));
        tokio::time::sleep(Duration::from_millis(80)).await;
        bus.publish(paused(1));
        tokio::time::sleep(Duration::from_millis(120)).await;

        let snapshot = swapper.snapshot();
        assert!(
            snapshot.contains(&SwapEvent::Static),
            "expected Static at end of cycle, got {snapshot:?}",
        );
        // The last call must be Static — once paused, no more frames.
        let last_frame_idx = snapshot
            .iter()
            .rposition(|e| matches!(e, SwapEvent::Frame(_)));
        let last_static_idx = snapshot
            .iter()
            .rposition(|e| matches!(e, SwapEvent::Static));
        assert!(
            last_static_idx > last_frame_idx,
            "Static must come after the last frame, snapshot={snapshot:?}",
        );
    }

    #[tokio::test]
    async fn test_spawn_animator_ignores_progress_events() {
        let bus = TokioEventBus::new(64);
        let swapper = Arc::new(RecordingSwapper::default());
        spawn_tray_animator(
            &bus,
            swapper.clone() as Arc<dyn IconSwapper>,
            4,
            Duration::from_millis(500), // slow enough that ticks won't fire
        );
        tokio::time::sleep(Duration::from_millis(10)).await;

        for _ in 0..50 {
            bus.publish(DomainEvent::DownloadProgress {
                id: DownloadId(1),
                downloaded_bytes: 0,
                total_bytes: 100,
            });
        }
        tokio::time::sleep(Duration::from_millis(50)).await;

        let snapshot = swapper.snapshot();
        assert!(
            snapshot.is_empty(),
            "progress should not produce swaps, got {snapshot:?}",
        );
    }
}
