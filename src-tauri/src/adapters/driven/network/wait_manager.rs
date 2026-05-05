//! Wait-time scheduler driving the `Waiting` state.
//!
//! Hosters routinely impose a cooldown between two free downloads (or
//! between two queries on the same IP). The plugin signals that delay
//! through `get_wait_time()` and the engine parks the [`Download`] in the
//! `Waiting` state so the queue manager doesn't keep retrying.
//!
//! `WaitManager` owns one `tokio::time::sleep` per parked download. When
//! the deadline expires it transitions the aggregate back to
//! `Downloading` and emits a [`DomainEvent::DownloadResumedFromWait`]
//! so the queue manager can pick it up again. The user can also
//! `skip_wait` (premium hosters expose a "skip queue" path) or
//! `cancel_wait` (used by the regular cancel/fail flow); both abort the
//! timer cleanly.
//!
//! The frontend renders a live countdown using the absolute deadline
//! (`until_unix_ms`) carried by [`DomainEvent::DownloadWaitingStarted`],
//! avoiding clock-drift jitter.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::{Mutex, MutexGuard, PoisonError};

use tokio::task::JoinHandle;
use tokio::time::Duration;

use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::download::DownloadId;
use crate::domain::ports::driven::{Clock, DownloadRepository, EventBus};

pub struct WaitManager {
    download_repo: Arc<dyn DownloadRepository>,
    event_bus: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
    handles: Mutex<HashMap<DownloadId, JoinHandle<()>>>,
}

impl WaitManager {
    pub fn new(
        download_repo: Arc<dyn DownloadRepository>,
        event_bus: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
    ) -> Arc<Self> {
        Arc::new(Self {
            download_repo,
            event_bus,
            clock,
            handles: Mutex::new(HashMap::new()),
        })
    }

    /// Recovers from a poisoned mutex by extracting the inner state. The
    /// only way the lock can be poisoned is a panic inside one of the
    /// (very small) critical sections in this module — at which point
    /// the map is structurally fine and there is no recovery left to
    /// do, so dropping the poison flag and continuing is preferable to
    /// crashing every subsequent `cancel_wait` / `skip_wait` call.
    fn handles(&self) -> MutexGuard<'_, HashMap<DownloadId, JoinHandle<()>>> {
        self.handles.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub async fn schedule_wait(
        self: &Arc<Self>,
        id: DownloadId,
        total_seconds: u32,
        reason: String,
    ) -> Result<(), AppError> {
        let mut download = self
            .download_repo
            .find_by_id(id)?
            .ok_or_else(|| AppError::NotFound(format!("download #{}", id.0)))?;

        download.wait()?;
        self.download_repo.save(&download)?;

        let until_unix_ms = self
            .clock
            .now_unix_ms()
            .saturating_add(u64::from(total_seconds).saturating_mul(1_000));

        self.event_bus.publish(DomainEvent::DownloadWaiting { id });
        self.event_bus.publish(DomainEvent::DownloadWaitingStarted {
            id,
            until_unix_ms,
            total_seconds,
            reason,
        });

        // Reserve the slot under the same lock that the spawned task will
        // later use to remove itself. Without this, a `total_seconds == 0`
        // wait (or a `tokio::time::advance` in tests) could let the timer
        // fire and call `expire_wait` *before* the parent inserted the
        // handle, leaving an orphan `JoinHandle` in the map.
        let mut guard = self.handles();
        let me = Arc::clone(self);
        let handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(u64::from(total_seconds))).await;
            if let Err(e) = me.expire_wait(id) {
                // The aggregate moved out of `Waiting` between the timer
                // arming and now (cancel / fail flow ran first). Logged
                // but not surfaced — the cancel path already published
                // its own `DownloadWaitingEnded`.
                tracing::debug!("wait expiry for download #{} dropped: {e}", id.0);
            }
        });
        guard.insert(id, handle);
        Ok(())
    }

    pub fn skip_wait(self: &Arc<Self>, id: DownloadId) -> Result<(), AppError> {
        if !self.abort_handle(id) {
            return Err(AppError::NotFound(format!(
                "no active wait for download #{}",
                id.0
            )));
        }
        self.resume_aggregate(id, /* expired_naturally = */ false)
    }

    /// No-op when no wait is active.
    pub fn cancel_wait(&self, id: DownloadId) {
        if self.abort_handle(id) {
            self.event_bus.publish(DomainEvent::DownloadWaitingEnded {
                id,
                expired_naturally: false,
            });
        }
    }

    #[cfg(test)]
    pub fn active_count(&self) -> usize {
        self.handles().len()
    }

    fn abort_handle(&self, id: DownloadId) -> bool {
        match self.handles().remove(&id) {
            Some(handle) => {
                handle.abort();
                true
            }
            None => false,
        }
    }

    fn expire_wait(self: &Arc<Self>, id: DownloadId) -> Result<(), AppError> {
        self.handles().remove(&id);
        self.resume_aggregate(id, /* expired_naturally = */ true)
    }

    fn resume_aggregate(
        self: &Arc<Self>,
        id: DownloadId,
        expired_naturally: bool,
    ) -> Result<(), AppError> {
        let mut download = self
            .download_repo
            .find_by_id(id)?
            .ok_or_else(|| AppError::NotFound(format!("download #{}", id.0)))?;
        let resume_event = download.resume_from_wait()?;
        self.download_repo.save(&download)?;

        self.event_bus.publish(DomainEvent::DownloadWaitingEnded {
            id,
            expired_naturally,
        });
        self.event_bus.publish(resume_event);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::domain::error::DomainError;
    use crate::domain::model::download::{Download, DownloadState, Url};

    // ── Test doubles ────────────────────────────────────────────

    struct InMemoryDownloadRepo {
        items: Mutex<HashMap<DownloadId, Download>>,
    }

    impl InMemoryDownloadRepo {
        fn new() -> Self {
            Self {
                items: Mutex::new(HashMap::new()),
            }
        }

        fn insert(&self, d: Download) {
            self.items.lock().unwrap().insert(d.id(), d);
        }

        fn state_of(&self, id: DownloadId) -> Option<DownloadState> {
            self.items.lock().unwrap().get(&id).map(|d| d.state())
        }
    }

    impl DownloadRepository for InMemoryDownloadRepo {
        fn find_by_id(&self, id: DownloadId) -> Result<Option<Download>, DomainError> {
            Ok(self.items.lock().unwrap().get(&id).cloned())
        }

        fn save(&self, download: &Download) -> Result<(), DomainError> {
            self.items
                .lock()
                .unwrap()
                .insert(download.id(), download.clone());
            Ok(())
        }

        fn delete(&self, id: DownloadId) -> Result<(), DomainError> {
            self.items.lock().unwrap().remove(&id);
            Ok(())
        }

        fn find_by_state(&self, state: DownloadState) -> Result<Vec<Download>, DomainError> {
            Ok(self
                .items
                .lock()
                .unwrap()
                .values()
                .filter(|d| d.state() == state)
                .cloned()
                .collect())
        }
    }

    struct CollectingEventBus {
        events: Mutex<Vec<DomainEvent>>,
    }

    impl CollectingEventBus {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }

        fn snapshot(&self) -> Vec<DomainEvent> {
            self.events.lock().unwrap().clone()
        }
    }

    impl EventBus for CollectingEventBus {
        fn publish(&self, event: DomainEvent) {
            self.events.lock().unwrap().push(event);
        }

        fn subscribe(&self, _handler: Box<dyn Fn(&DomainEvent) + Send + Sync + 'static>) {}
    }

    struct FakeClock {
        now_ms: AtomicU64,
    }

    impl FakeClock {
        fn at_ms(now_ms: u64) -> Self {
            Self {
                now_ms: AtomicU64::new(now_ms),
            }
        }
    }

    impl Clock for FakeClock {
        fn now_unix_secs(&self) -> u64 {
            self.now_ms.load(Ordering::SeqCst) / 1_000
        }

        fn now_unix_ms(&self) -> u64 {
            self.now_ms.load(Ordering::SeqCst)
        }
    }

    /// Pumps the single-threaded paused runtime — needed both before
    /// `time::advance` (so spawned tasks reach their first `.sleep().await`)
    /// and after (so the wake path runs through `expire_wait` to completion).
    async fn pump_runtime() {
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
    }

    fn make_downloading(id: u64) -> Download {
        let url = Url::new("https://example.com/file.zip").expect("valid url");
        let mut d = Download::new(DownloadId(id), url, "file.zip".into(), "/tmp".into());
        d.start().expect("Queued → Downloading");
        d
    }

    fn setup(
        clock_now_ms: u64,
    ) -> (
        Arc<WaitManager>,
        Arc<InMemoryDownloadRepo>,
        Arc<CollectingEventBus>,
    ) {
        let repo = Arc::new(InMemoryDownloadRepo::new());
        let bus = Arc::new(CollectingEventBus::new());
        let clock = Arc::new(FakeClock::at_ms(clock_now_ms));
        let mgr = WaitManager::new(
            Arc::clone(&repo) as Arc<dyn DownloadRepository>,
            Arc::clone(&bus) as Arc<dyn EventBus>,
            clock as Arc<dyn Clock>,
        );
        (mgr, repo, bus)
    }

    // ── Tests ───────────────────────────────────────────────────

    #[tokio::test(start_paused = true)]
    async fn schedule_wait_publishes_started_with_deadline_and_reason() {
        let (mgr, repo, bus) = setup(1_700_000_000_000);
        repo.insert(make_downloading(1));

        mgr.schedule_wait(DownloadId(1), 60, "hoster cooldown".into())
            .await
            .expect("schedule_wait");

        let events = bus.snapshot();
        assert!(matches!(
            events[0],
            DomainEvent::DownloadWaiting { id: DownloadId(1) }
        ));
        match &events[1] {
            DomainEvent::DownloadWaitingStarted {
                id,
                until_unix_ms,
                total_seconds,
                reason,
            } => {
                assert_eq!(*id, DownloadId(1));
                assert_eq!(*until_unix_ms, 1_700_000_000_000 + 60_000);
                assert_eq!(*total_seconds, 60);
                assert_eq!(reason, "hoster cooldown");
            }
            other => panic!("expected DownloadWaitingStarted, got {other:?}"),
        }
        assert_eq!(repo.state_of(DownloadId(1)), Some(DownloadState::Waiting));
    }

    #[tokio::test(start_paused = true)]
    async fn timer_expiry_resumes_download_and_emits_ended() {
        let (mgr, repo, bus) = setup(1_700_000_000_000);
        repo.insert(make_downloading(1));

        mgr.schedule_wait(DownloadId(1), 30, "cooldown".into())
            .await
            .expect("schedule_wait");
        pump_runtime().await;

        // Advance past the deadline + drain the scheduler so the spawned
        // task wakes from `sleep` and runs through `expire_wait`.
        tokio::time::advance(Duration::from_secs(31)).await;
        pump_runtime().await;

        let events = bus.snapshot();
        let ended = events.iter().find_map(|e| match e {
            DomainEvent::DownloadWaitingEnded {
                id,
                expired_naturally,
            } => Some((*id, *expired_naturally)),
            _ => None,
        });
        assert_eq!(ended, Some((DownloadId(1), true)));
        assert!(events.iter().any(|e| matches!(
            e,
            DomainEvent::DownloadResumedFromWait { id: DownloadId(1) }
        )));
        assert_eq!(
            repo.state_of(DownloadId(1)),
            Some(DownloadState::Downloading)
        );
        assert_eq!(mgr.active_count(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_wait_aborts_timer_and_emits_ended_not_natural() {
        let (mgr, repo, bus) = setup(1_700_000_000_000);
        repo.insert(make_downloading(1));

        mgr.schedule_wait(DownloadId(1), 60, "cooldown".into())
            .await
            .expect("schedule_wait");
        assert_eq!(mgr.active_count(), 1);

        mgr.cancel_wait(DownloadId(1));
        // Even if we let time fly, the timer is already aborted.
        tokio::time::advance(Duration::from_secs(120)).await;
        pump_runtime().await;

        let events = bus.snapshot();
        let ended = events.iter().find_map(|e| match e {
            DomainEvent::DownloadWaitingEnded {
                id,
                expired_naturally,
            } => Some((*id, *expired_naturally)),
            _ => None,
        });
        assert_eq!(ended, Some((DownloadId(1), false)));
        // No resume event because cancel just aborts.
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, DomainEvent::DownloadResumedFromWait { .. }))
        );
        // Aggregate untouched (still in Waiting until the cancel handler
        // moves it to Cancelled — that path is outside this manager).
        assert_eq!(repo.state_of(DownloadId(1)), Some(DownloadState::Waiting));
        assert_eq!(mgr.active_count(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn skip_wait_resumes_immediately_with_expired_false() {
        let (mgr, repo, bus) = setup(1_700_000_000_000);
        repo.insert(make_downloading(1));

        mgr.schedule_wait(DownloadId(1), 60, "cooldown".into())
            .await
            .expect("schedule_wait");

        mgr.skip_wait(DownloadId(1)).expect("skip");

        let events = bus.snapshot();
        let ended = events.iter().find_map(|e| match e {
            DomainEvent::DownloadWaitingEnded {
                id,
                expired_naturally,
            } => Some((*id, *expired_naturally)),
            _ => None,
        });
        assert_eq!(ended, Some((DownloadId(1), false)));
        assert!(events.iter().any(|e| matches!(
            e,
            DomainEvent::DownloadResumedFromWait { id: DownloadId(1) }
        )));
        assert_eq!(
            repo.state_of(DownloadId(1)),
            Some(DownloadState::Downloading)
        );
        assert_eq!(mgr.active_count(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn skip_wait_unknown_id_returns_not_found() {
        let (mgr, _repo, _bus) = setup(0);
        let err = mgr.skip_wait(DownloadId(99)).expect_err("expected error");
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[tokio::test(start_paused = true)]
    async fn multiple_waits_run_in_parallel_and_expire_independently() {
        let (mgr, repo, bus) = setup(1_700_000_000_000);
        repo.insert(make_downloading(1));
        repo.insert(make_downloading(2));
        repo.insert(make_downloading(3));

        mgr.schedule_wait(DownloadId(1), 10, "a".into())
            .await
            .unwrap();
        mgr.schedule_wait(DownloadId(2), 30, "b".into())
            .await
            .unwrap();
        mgr.schedule_wait(DownloadId(3), 60, "c".into())
            .await
            .unwrap();
        pump_runtime().await;
        assert_eq!(mgr.active_count(), 3);

        tokio::time::advance(Duration::from_secs(15)).await;
        pump_runtime().await;
        // #1 expired, #2 and #3 still parked.
        assert_eq!(
            repo.state_of(DownloadId(1)),
            Some(DownloadState::Downloading)
        );
        assert_eq!(repo.state_of(DownloadId(2)), Some(DownloadState::Waiting));
        assert_eq!(repo.state_of(DownloadId(3)), Some(DownloadState::Waiting));
        assert_eq!(mgr.active_count(), 2);

        tokio::time::advance(Duration::from_secs(20)).await;
        pump_runtime().await;
        // #2 expired now.
        assert_eq!(
            repo.state_of(DownloadId(2)),
            Some(DownloadState::Downloading)
        );
        assert_eq!(repo.state_of(DownloadId(3)), Some(DownloadState::Waiting));
        assert_eq!(mgr.active_count(), 1);

        tokio::time::advance(Duration::from_secs(60)).await;
        pump_runtime().await;
        assert_eq!(
            repo.state_of(DownloadId(3)),
            Some(DownloadState::Downloading)
        );
        assert_eq!(mgr.active_count(), 0);

        // Each download got exactly one Started + one Ended event.
        let events = bus.snapshot();
        for n in 1..=3 {
            let started = events
                .iter()
                .filter(
                    |e| matches!(e, DomainEvent::DownloadWaitingStarted { id, .. } if id.0 == n),
                )
                .count();
            let ended = events
                .iter()
                .filter(|e| matches!(e, DomainEvent::DownloadWaitingEnded { id, .. } if id.0 == n))
                .count();
            assert_eq!(started, 1, "download {n} should have 1 started event");
            assert_eq!(ended, 1, "download {n} should have 1 ended event");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_wait_on_unknown_id_is_silent_noop() {
        let (mgr, _repo, bus) = setup(0);
        mgr.cancel_wait(DownloadId(404));
        assert!(bus.snapshot().is_empty());
    }
}
