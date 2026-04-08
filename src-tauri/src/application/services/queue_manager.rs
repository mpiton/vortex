//! Queue manager — schedules queued downloads respecting concurrency limits.
//!
//! Listens to domain events and starts the next queued download
//! whenever a slot becomes available.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::application::error::AppError;
use crate::domain::error::DomainError;
use crate::domain::event::DomainEvent;
use crate::domain::model::download::{DownloadId, DownloadState};
use crate::domain::ports::driven::download_engine::DownloadEngine;
use crate::domain::ports::driven::download_repository::DownloadRepository;
use crate::domain::ports::driven::event_bus::EventBus;

pub struct QueueManager {
    download_repo: Arc<dyn DownloadRepository>,
    engine: Arc<dyn DownloadEngine>,
    event_bus: Arc<dyn EventBus>,
    max_concurrent: Arc<AtomicUsize>,
    active_count: Arc<AtomicUsize>,
    schedule_lock: Arc<tokio::sync::Mutex<()>>,
    retry_cancellations: Arc<Mutex<HashMap<u64, CancellationToken>>>,
}

fn lock_map(
    m: &Mutex<HashMap<u64, CancellationToken>>,
) -> std::sync::MutexGuard<'_, HashMap<u64, CancellationToken>> {
    match m.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl QueueManager {
    pub fn new(
        download_repo: Arc<dyn DownloadRepository>,
        engine: Arc<dyn DownloadEngine>,
        event_bus: Arc<dyn EventBus>,
        max_concurrent: usize,
    ) -> Self {
        Self {
            download_repo,
            engine,
            event_bus,
            max_concurrent: Arc::new(AtomicUsize::new(max_concurrent)),
            active_count: Arc::new(AtomicUsize::new(0)),
            schedule_lock: Arc::new(tokio::sync::Mutex::new(())),
            retry_cancellations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn active_count(&self) -> usize {
        self.active_count.load(Ordering::SeqCst)
    }

    // F1: safe decrement — never wraps below 0
    fn safe_decrement(&self) {
        self.active_count
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                Some(current.saturating_sub(1))
            })
            .ok();
    }

    // F3+F4: takes &Arc<Self> so we clone the real Arc into the spawned task
    pub fn set_max_concurrent(self: &Arc<Self>, n: usize) {
        self.max_concurrent.store(n, Ordering::SeqCst);
        let this = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(e) = this.on_slot_freed().await {
                tracing::warn!("set_max_concurrent: on_slot_freed error: {e}");
            }
        });
    }

    pub async fn on_slot_freed(&self) -> Result<(), AppError> {
        let _guard = self.schedule_lock.lock().await;

        loop {
            let active = self.active_count.load(Ordering::SeqCst);
            let max = self.max_concurrent.load(Ordering::SeqCst);
            if active >= max {
                return Ok(());
            }

            // Collect Queued and matured Retry candidates (those whose backoff
            // timer has fired, i.e. no longer in retry_cancellations).
            let mut candidates = self.download_repo.find_by_state(DownloadState::Queued)?;
            let retrying = self.download_repo.find_by_state(DownloadState::Retry)?;
            {
                let pending = lock_map(&self.retry_cancellations);
                candidates.extend(
                    retrying
                        .into_iter()
                        .filter(|d| !pending.contains_key(&d.id().0)),
                );
            }

            if candidates.is_empty() {
                return Ok(());
            }

            // Sort: priority desc, then created_at asc (FIFO)
            candidates.sort_by(|a, b| {
                b.priority()
                    .value()
                    .cmp(&a.priority().value())
                    .then_with(|| a.created_at().cmp(&b.created_at()))
            });

            let mut download = candidates.remove(0);
            let event = download.start()?;
            self.download_repo.save(&download)?;
            self.event_bus.publish(event);

            self.active_count.fetch_add(1, Ordering::SeqCst);

            if let Err(engine_err) = self.engine.start(&download) {
                // Roll back: no task was spawned, so decrement and persist Error.
                // Publish the event so other subscribers (Tauri bridge/UI) see
                // the failure. handle_download_failed gates its own decrement
                // on the download's state: it will see Error (already saved
                // here) and skip the decrement, preventing double-count.
                self.safe_decrement();
                if let Ok(fail_event) = download.fail(engine_err.to_string()) {
                    let _ = self.download_repo.save(&download);
                    self.event_bus.publish(fail_event);
                }
                return Err(AppError::Domain(engine_err));
            }
        }
    }

    pub async fn decrement_and_schedule(&self) -> Result<(), AppError> {
        self.safe_decrement(); // F1
        self.on_slot_freed().await
    }

    pub async fn handle_download_failed(self: &Arc<Self>, id: DownloadId) -> Result<(), AppError> {
        // Single find_by_id — avoids TOCTOU from double read
        let mut download = match self.download_repo.find_by_id(id)? {
            Some(d) => d,
            None => {
                self.on_slot_freed().await?;
                return Ok(());
            }
        };

        // Only decrement if the download was in an active state. Rollback-
        // generated events arrive with the download already in Error (the
        // rollback path persisted that state before publishing), so we skip
        // the decrement to avoid double-counting.
        if matches!(
            download.state(),
            DownloadState::Downloading
                | DownloadState::Waiting
                | DownloadState::Checking
                | DownloadState::Extracting
        ) {
            self.safe_decrement();
        }

        match download.retry() {
            Ok(event) => {
                self.download_repo.save(&download)?;
                self.event_bus.publish(event);
                self.schedule_retry(id, download.retry_count());
                // Slot was freed by safe_decrement above — try filling it
                self.on_slot_freed().await?;
                Ok(())
            }
            Err(DomainError::MaxRetriesExceeded { .. }) => {
                self.on_slot_freed().await?;
                Ok(())
            }
            Err(e) => Err(AppError::Domain(e)),
        }
    }

    // F3+F4: takes &Arc<Self> to clone the real Arc into the spawned task
    pub fn schedule_retry(self: &Arc<Self>, id: DownloadId, attempt: u32) {
        let token = CancellationToken::new();
        {
            let mut map = lock_map(&self.retry_cancellations); // F7
            map.insert(id.0, token.clone());
        }

        let this = Arc::clone(self);
        let delay = retry_delay(attempt);

        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = token.cancelled() => { return; }
            }

            // Timer matured: remove from pending set so on_slot_freed can
            // pick this Retry download through the normal priority/FIFO path.
            {
                let mut map = lock_map(&this.retry_cancellations);
                map.remove(&id.0);
            }

            if let Err(e) = this.on_slot_freed().await {
                tracing::warn!("schedule_retry: on_slot_freed error after delay for {id:?}: {e}");
            }
        });
    }

    pub fn cancel_retry(&self, id: DownloadId) {
        let mut map = lock_map(&self.retry_cancellations); // F7
        if let Some(token) = map.remove(&id.0) {
            token.cancel();
        }
    }

    pub fn start_listening(self: Arc<Self>) {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<DomainEvent>(1024);

        // Only forward lifecycle events that affect scheduling.
        // DownloadProgress fires every 500ms per segment and would flood the channel.
        self.event_bus.subscribe(Box::new(move |event| {
            let dominated = matches!(
                event,
                DomainEvent::DownloadCompleted { .. }
                    | DomainEvent::DownloadPaused { .. }
                    | DomainEvent::DownloadFailed { .. }
            );
            if dominated && tx.try_send(event.clone()).is_err() {
                tracing::error!("QueueManager event channel full, dropping lifecycle event");
            }
        }));

        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let result = match &event {
                    DomainEvent::DownloadCompleted { .. } => self.decrement_and_schedule().await,
                    DomainEvent::DownloadPaused { .. } => self.decrement_and_schedule().await,
                    DomainEvent::DownloadFailed { id, .. } => {
                        self.handle_download_failed(*id).await
                    }
                    _ => Ok(()),
                };
                if let Err(e) = result {
                    tracing::warn!("QueueManager event handler error: {e}");
                }
            }
        });
    }
}

// F9: pub(crate) visibility
pub(crate) fn retry_delay(attempt: u32) -> Duration {
    // Clamp exponent to 5 so the intermediate 2^exp never overflows u64.
    // 10 * 2^5 = 320, capped to 300 by the min() below.
    let exp = attempt.saturating_sub(1).min(5);
    let delay = Duration::from_secs(10 * (1u64 << exp));
    delay.min(Duration::from_secs(300))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::download::{Download, DownloadId, DownloadState, Url};
    use crate::domain::model::queue::Priority;

    // --- Inline mocks ---

    struct MockDownloadRepo {
        downloads: Mutex<HashMap<u64, Download>>,
    }

    impl MockDownloadRepo {
        fn new(downloads: Vec<Download>) -> Self {
            Self {
                downloads: Mutex::new(downloads.into_iter().map(|d| (d.id().0, d)).collect()),
            }
        }
    }

    impl DownloadRepository for MockDownloadRepo {
        fn find_by_id(&self, id: DownloadId) -> Result<Option<Download>, DomainError> {
            Ok(self.downloads.lock().unwrap().get(&id.0).cloned())
        }

        fn save(&self, download: &Download) -> Result<(), DomainError> {
            self.downloads
                .lock()
                .unwrap()
                .insert(download.id().0, download.clone());
            Ok(())
        }

        fn delete(&self, id: DownloadId) -> Result<(), DomainError> {
            self.downloads.lock().unwrap().remove(&id.0);
            Ok(())
        }

        fn find_by_state(&self, state: DownloadState) -> Result<Vec<Download>, DomainError> {
            Ok(self
                .downloads
                .lock()
                .unwrap()
                .values()
                .filter(|d| d.state() == state)
                .cloned()
                .collect())
        }
    }

    struct MockEngine {
        started: Mutex<Vec<u64>>,
    }

    impl MockEngine {
        fn new() -> Self {
            Self {
                started: Mutex::new(Vec::new()),
            }
        }
    }

    impl DownloadEngine for MockEngine {
        fn start(&self, download: &Download) -> Result<(), DomainError> {
            self.started.lock().unwrap().push(download.id().0);
            Ok(())
        }

        fn pause(&self, _id: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }

        fn resume(&self, _id: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }

        fn cancel(&self, _id: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct MockEventBus {
        events: Mutex<Vec<DomainEvent>>,
    }

    impl MockEventBus {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }
    }

    impl EventBus for MockEventBus {
        fn publish(&self, event: DomainEvent) {
            self.events.lock().unwrap().push(event);
        }

        fn subscribe(&self, _handler: Box<dyn Fn(&DomainEvent) + Send + Sync + 'static>) {}
    }

    fn make_download(id: u64, priority: u8, state: DownloadState) -> Download {
        let url = Url::new("https://example.com/file.zip").unwrap();
        let mut d = Download::new(DownloadId(id), url, "file.zip".into(), "/tmp".into())
            .with_priority(Priority::new(priority).unwrap());

        match state {
            DownloadState::Queued => {}
            DownloadState::Error => {
                d.start().unwrap();
                d.fail("err".to_string()).unwrap();
            }
            DownloadState::Retry => {
                d.start().unwrap();
                d.fail("err".to_string()).unwrap();
                d.retry().unwrap();
            }
            _ => {}
        }
        d
    }

    fn make_manager(
        repo: Arc<MockDownloadRepo>,
        engine: Arc<MockEngine>,
        event_bus: Arc<MockEventBus>,
        max: usize,
        active: usize,
    ) -> Arc<QueueManager> {
        let qm = QueueManager::new(repo, engine, event_bus, max);
        qm.active_count.store(active, Ordering::SeqCst);
        Arc::new(qm)
    }

    #[tokio::test]
    async fn test_on_slot_freed_starts_next_queued() {
        let d = make_download(1, 5, DownloadState::Queued);
        let repo = Arc::new(MockDownloadRepo::new(vec![d]));
        let engine = Arc::new(MockEngine::new());
        let bus = Arc::new(MockEventBus::new());
        let qm = make_manager(
            Arc::clone(&repo),
            Arc::clone(&engine),
            Arc::clone(&bus),
            3,
            0,
        );

        qm.on_slot_freed().await.unwrap();

        assert!(engine.started.lock().unwrap().contains(&1));
        assert_eq!(qm.active_count(), 1);
    }

    #[tokio::test]
    async fn test_on_slot_freed_respects_max_concurrent() {
        let d = make_download(1, 5, DownloadState::Queued);
        let repo = Arc::new(MockDownloadRepo::new(vec![d]));
        let engine = Arc::new(MockEngine::new());
        let bus = Arc::new(MockEventBus::new());
        let qm = make_manager(
            Arc::clone(&repo),
            Arc::clone(&engine),
            Arc::clone(&bus),
            3,
            3,
        );

        qm.on_slot_freed().await.unwrap();

        assert!(engine.started.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_on_slot_freed_priority_ordering() {
        let d1 = make_download(1, 3, DownloadState::Queued);
        let d2 = make_download(2, 7, DownloadState::Queued);
        let d3 = make_download(3, 1, DownloadState::Queued);
        let repo = Arc::new(MockDownloadRepo::new(vec![d1, d2, d3]));
        let engine = Arc::new(MockEngine::new());
        let bus = Arc::new(MockEventBus::new());
        let qm = make_manager(
            Arc::clone(&repo),
            Arc::clone(&engine),
            Arc::clone(&bus),
            1,
            0,
        );

        qm.on_slot_freed().await.unwrap();

        let started = engine.started.lock().unwrap().clone();
        assert_eq!(started, vec![2]);
    }

    #[tokio::test]
    async fn test_on_slot_freed_empty_queue() {
        let repo = Arc::new(MockDownloadRepo::new(vec![]));
        let engine = Arc::new(MockEngine::new());
        let bus = Arc::new(MockEventBus::new());
        let qm = make_manager(
            Arc::clone(&repo),
            Arc::clone(&engine),
            Arc::clone(&bus),
            3,
            0,
        );

        qm.on_slot_freed().await.unwrap();

        assert!(engine.started.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_on_slot_freed_fills_all_available_slots() {
        let d1 = make_download(1, 5, DownloadState::Queued);
        let d2 = make_download(2, 5, DownloadState::Queued);
        let d3 = make_download(3, 5, DownloadState::Queued);
        let repo = Arc::new(MockDownloadRepo::new(vec![d1, d2, d3]));
        let engine = Arc::new(MockEngine::new());
        let bus = Arc::new(MockEventBus::new());
        let qm = make_manager(
            Arc::clone(&repo),
            Arc::clone(&engine),
            Arc::clone(&bus),
            3,
            0,
        );

        qm.on_slot_freed().await.unwrap();

        // All 3 slots should be filled
        assert_eq!(engine.started.lock().unwrap().len(), 3);
        assert_eq!(qm.active_count(), 3);
    }

    #[tokio::test]
    async fn test_on_slot_freed_picks_up_retry_state() {
        // Retry-state downloads should also be started by on_slot_freed
        let d = make_download(1, 5, DownloadState::Retry);
        let repo = Arc::new(MockDownloadRepo::new(vec![d]));
        let engine = Arc::new(MockEngine::new());
        let bus = Arc::new(MockEventBus::new());
        let qm = make_manager(
            Arc::clone(&repo),
            Arc::clone(&engine),
            Arc::clone(&bus),
            3,
            0,
        );

        qm.on_slot_freed().await.unwrap();

        assert_eq!(engine.started.lock().unwrap().clone(), vec![1]);
        assert_eq!(qm.active_count(), 1);
    }

    #[test]
    fn test_retry_delay_exponential() {
        assert_eq!(retry_delay(1), Duration::from_secs(10));
        assert_eq!(retry_delay(2), Duration::from_secs(20));
        assert_eq!(retry_delay(3), Duration::from_secs(40));
        assert_eq!(retry_delay(4), Duration::from_secs(80));
        assert_eq!(retry_delay(5), Duration::from_secs(160));
    }

    #[test]
    fn test_retry_delay_capped_at_300s() {
        assert_eq!(retry_delay(6), Duration::from_secs(300));
        assert_eq!(retry_delay(10), Duration::from_secs(300));
    }

    #[tokio::test]
    async fn test_circuit_breaker_stops_retries() {
        let mut d = make_download(1, 5, DownloadState::Queued);
        d = d.with_max_retries(0);
        // Transition to Error: start -> fail
        d.start().unwrap();
        d.fail("err".to_string()).unwrap();
        assert_eq!(d.state(), DownloadState::Error);

        let repo = Arc::new(MockDownloadRepo::new(vec![d]));
        let engine = Arc::new(MockEngine::new());
        let bus = Arc::new(MockEventBus::new());
        let qm = make_manager(
            Arc::clone(&repo),
            Arc::clone(&engine),
            Arc::clone(&bus),
            3,
            1,
        );

        qm.handle_download_failed(DownloadId(1)).await.unwrap();

        // Download should remain in Error state
        let saved = repo.find_by_id(DownloadId(1)).unwrap().unwrap();
        assert_eq!(saved.state(), DownloadState::Error);
    }

    #[tokio::test]
    async fn test_on_slot_freed_idempotent() {
        let d = make_download(1, 5, DownloadState::Queued);
        let repo = Arc::new(MockDownloadRepo::new(vec![d]));
        let engine = Arc::new(MockEngine::new());
        let bus = Arc::new(MockEventBus::new());
        let qm = make_manager(
            Arc::clone(&repo),
            Arc::clone(&engine),
            Arc::clone(&bus),
            1,
            0,
        );

        let qm2 = Arc::clone(&qm);
        let (r1, r2) = tokio::join!(qm.on_slot_freed(), qm2.on_slot_freed());
        r1.unwrap();
        r2.unwrap();

        let started = engine.started.lock().unwrap().clone();
        assert_eq!(started.len(), 1);
        assert_eq!(qm.active_count(), 1);
    }

    #[tokio::test]
    async fn test_safe_decrement_no_underflow() {
        let repo = Arc::new(MockDownloadRepo::new(vec![]));
        let engine = Arc::new(MockEngine::new());
        let bus = Arc::new(MockEventBus::new());
        let qm = make_manager(
            Arc::clone(&repo),
            Arc::clone(&engine),
            Arc::clone(&bus),
            3,
            0,
        );

        // Decrementing when already 0 should not wrap
        qm.safe_decrement();
        qm.safe_decrement();
        assert_eq!(qm.active_count(), 0);
    }

    #[tokio::test]
    async fn test_handle_failed_rollback_skips_decrement() {
        // When handle_download_failed sees a download already in Error state
        // (rollback-generated event), it skips safe_decrement to avoid
        // double-counting. active_count stays unchanged.
        let d = make_download(1, 5, DownloadState::Error);
        let repo = Arc::new(MockDownloadRepo::new(vec![d]));
        let engine = Arc::new(MockEngine::new());
        let bus = Arc::new(MockEventBus::new());
        let qm = make_manager(
            Arc::clone(&repo),
            Arc::clone(&engine),
            Arc::clone(&bus),
            3,
            1,
        );

        qm.handle_download_failed(DownloadId(1)).await.unwrap();

        // No decrement — download was already Error (rollback), active stays 1
        assert_eq!(qm.active_count(), 1);
        assert!(engine.started.lock().unwrap().is_empty());
        // Download is in Retry state, waiting for backoff timer
        let saved = repo.find_by_id(DownloadId(1)).unwrap().unwrap();
        assert_eq!(saved.state(), DownloadState::Retry);
    }
}
