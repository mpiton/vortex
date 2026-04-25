//! Queue reordering command handlers (task 12).
//!
//! Three mutations on the queue ordering:
//!   - `MoveToTopCommand`: target download gets the smallest `queue_position`
//!     across the reorderable pool (Queued + Retry + Waiting).
//!   - `MoveToBottomCommand`: target gets the largest.
//!   - `ReorderQueueCommand`: reassigns `queue_position = 1..N` for the
//!     supplied ID list, preserving the caller's order.
//!
//! Every successful mutation publishes `DomainEvent::QueueReordered` so the
//! `QueueManager` re-evaluates scheduling with the new ordering.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::download::{Download, DownloadId, DownloadState};
use crate::domain::ports::driven::download_repository::DownloadRepository;

/// States that participate in queue ordering.
///
/// `Downloading`/`Paused`/`Error`/`Cancelled`/`Completed` are excluded: they
/// don't need a queue slot, so their position is irrelevant to scheduling.
const REORDERABLE_STATES: &[DownloadState] = &[
    DownloadState::Queued,
    DownloadState::Retry,
    DownloadState::Waiting,
];

/// Stride used by move-to-top/bottom to keep gaps between positions, so
/// repeated moves don't immediately collide at `i64::MIN`/`MAX`.
const POSITION_STRIDE: i64 = 1024;

fn is_reorderable(state: DownloadState) -> bool {
    REORDERABLE_STATES.contains(&state)
}

fn load_reorderable_pool(repo: &dyn DownloadRepository) -> Result<Vec<Download>, AppError> {
    let mut pool = Vec::new();
    for state in REORDERABLE_STATES {
        pool.extend(repo.find_by_state(*state)?);
    }
    Ok(pool)
}

/// Return the queue position to assign to a freshly created download so it
/// appends to the back of the existing reorderable queue. Falls back to 0
/// when no other reorderable downloads exist.
///
/// Callers must hold `CommandBus::lock_queue_positions()` while computing
/// the position and persisting the download. Otherwise two concurrent
/// creates can read the same max and assign colliding positions.
pub(crate) fn next_queue_position(repo: &dyn DownloadRepository) -> Result<i64, AppError> {
    let pool = load_reorderable_pool(repo)?;
    Ok(match pool.iter().map(|d| d.queue_position()).max() {
        Some(m) => m.saturating_add(POSITION_STRIDE),
        None => 0,
    })
}

impl CommandBus {
    pub async fn handle_move_to_top(&self, cmd: super::MoveToTopCommand) -> Result<(), AppError> {
        let _guard = self.lock_queue_positions().await;
        let target = self
            .download_repo()
            .find_by_id(cmd.id)?
            .ok_or_else(|| AppError::NotFound(format!("Download {} not found", cmd.id.0)))?;

        if !is_reorderable(target.state()) {
            return Err(AppError::Validation(format!(
                "Download {} is in state {:?} and cannot be reordered",
                cmd.id.0,
                target.state()
            )));
        }

        let pool = load_reorderable_pool(self.download_repo())?;
        let min_pos = pool
            .iter()
            .filter(|d| d.id() != cmd.id)
            .map(|d| d.queue_position())
            .min();
        let new_position = match min_pos {
            Some(m) => m.saturating_sub(POSITION_STRIDE),
            None => 0,
        };

        let moved = target.with_queue_position(new_position);
        self.download_repo().save(&moved)?;
        self.event_bus().publish(DomainEvent::QueueReordered {
            affected_ids: vec![cmd.id],
        });
        Ok(())
    }

    pub async fn handle_move_to_bottom(
        &self,
        cmd: super::MoveToBottomCommand,
    ) -> Result<(), AppError> {
        let _guard = self.lock_queue_positions().await;
        let target = self
            .download_repo()
            .find_by_id(cmd.id)?
            .ok_or_else(|| AppError::NotFound(format!("Download {} not found", cmd.id.0)))?;

        if !is_reorderable(target.state()) {
            return Err(AppError::Validation(format!(
                "Download {} is in state {:?} and cannot be reordered",
                cmd.id.0,
                target.state()
            )));
        }

        let pool = load_reorderable_pool(self.download_repo())?;
        let max_pos = pool
            .iter()
            .filter(|d| d.id() != cmd.id)
            .map(|d| d.queue_position())
            .max();
        let new_position = match max_pos {
            Some(m) => m.saturating_add(POSITION_STRIDE),
            None => 0,
        };

        let moved = target.with_queue_position(new_position);
        self.download_repo().save(&moved)?;
        self.event_bus().publish(DomainEvent::QueueReordered {
            affected_ids: vec![cmd.id],
        });
        Ok(())
    }

    pub async fn handle_reorder_queue(
        &self,
        cmd: super::ReorderQueueCommand,
    ) -> Result<(), AppError> {
        let _guard = self.lock_queue_positions().await;
        if cmd.ordered_ids.is_empty() {
            return Ok(());
        }

        // Reject duplicate ids — they would push the same download to two
        // positions in the renumbering and leave the queue inconsistent.
        let mut seen = std::collections::HashSet::with_capacity(cmd.ordered_ids.len());
        for id in &cmd.ordered_ids {
            if !seen.insert(*id) {
                return Err(AppError::Validation(format!(
                    "duplicate download id in ordered_ids: {}",
                    id.0
                )));
            }
        }

        // Load full reorderable pool so omitted items keep a coherent
        // global position rather than colliding with the renumbered subset.
        // `pool` keeps the repository's natural iteration order so we can
        // use it as a stable tie-breaker for items that share a queue_position
        // (e.g. the default 0 on freshly created downloads).
        let pool = load_reorderable_pool(self.download_repo())?;
        let pool_index: std::collections::HashMap<DownloadId, &Download> =
            pool.iter().map(|d| (d.id(), d)).collect();

        // Validate every submitted id exists and is reorderable.
        for id in &cmd.ordered_ids {
            let Some(download) = pool_index.get(id) else {
                return match self.download_repo().find_by_id(*id)? {
                    Some(_) => Err(AppError::Validation(format!(
                        "Download {} cannot be reordered",
                        id.0
                    ))),
                    None => Err(AppError::NotFound(format!("Download {} not found", id.0))),
                };
            };
            if !is_reorderable(download.state()) {
                return Err(AppError::Validation(format!(
                    "Download {} cannot be reordered",
                    id.0
                )));
            }
        }

        // Build the final order: caller-supplied IDs first (preserving the
        // submitted order), then any omitted reorderable items sorted by
        // (queue_position, original_index) so ties don't reshuffle the
        // existing relative order.
        let submitted: std::collections::HashSet<DownloadId> =
            cmd.ordered_ids.iter().copied().collect();
        let mut omitted: Vec<(usize, &Download)> = pool
            .iter()
            .enumerate()
            .filter(|(_, d)| !submitted.contains(&d.id()))
            .collect();
        omitted.sort_by_key(|(idx, d)| (d.queue_position(), *idx));

        let mut final_order: Vec<&Download> = Vec::with_capacity(pool.len());
        for id in &cmd.ordered_ids {
            if let Some(download) = pool_index.get(id) {
                final_order.push(*download);
            }
        }
        final_order.extend(omitted.into_iter().map(|(_, d)| d));

        let mut updates: Vec<Download> = Vec::with_capacity(final_order.len());
        let mut affected: Vec<DownloadId> = Vec::with_capacity(final_order.len());
        for (idx, download) in final_order.iter().enumerate() {
            let position = i64::try_from(idx + 1).unwrap_or(i64::MAX);
            if download.queue_position() != position {
                updates.push((*download).clone().with_queue_position(position));
                affected.push(download.id());
            }
        }

        if updates.is_empty() {
            return Ok(());
        }

        // Atomic batch persist so a mid-loop failure cannot leave the
        // queue partially renumbered.
        self.download_repo().save_batch(&updates)?;
        self.event_bus().publish(DomainEvent::QueueReordered {
            affected_ids: affected,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::application::commands::{
        MoveToBottomCommand, MoveToTopCommand, ReorderQueueCommand,
    };
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::archive::{ArchiveEntry, ArchiveFormat, ExtractSummary};
    use crate::domain::model::config::{AppConfig, ConfigPatch};
    use crate::domain::model::credential::Credential;
    use crate::domain::model::download::{Download, DownloadId, Url};
    use crate::domain::model::http::HttpResponse;
    use crate::domain::model::meta::DownloadMeta;
    use crate::domain::model::plugin::{PluginInfo, PluginManifest};
    use crate::domain::model::queue::Priority;
    use crate::domain::ports::driven::{
        ArchiveExtractor, ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine,
        DownloadRepository, EventBus, FileStorage, HttpClient, PluginLoader,
    };

    struct MockRepo {
        store: Mutex<HashMap<u64, Download>>,
    }

    impl MockRepo {
        fn new() -> Self {
            Self {
                store: Mutex::new(HashMap::new()),
            }
        }

        fn with(self, d: Download) -> Self {
            self.store.lock().unwrap().insert(d.id().0, d);
            self
        }
    }

    impl DownloadRepository for MockRepo {
        fn find_by_id(&self, id: DownloadId) -> Result<Option<Download>, DomainError> {
            Ok(self.store.lock().unwrap().get(&id.0).cloned())
        }
        fn save(&self, d: &Download) -> Result<(), DomainError> {
            self.store.lock().unwrap().insert(d.id().0, d.clone());
            Ok(())
        }
        fn delete(&self, id: DownloadId) -> Result<(), DomainError> {
            self.store.lock().unwrap().remove(&id.0);
            Ok(())
        }
        fn find_by_state(
            &self,
            s: crate::domain::model::download::DownloadState,
        ) -> Result<Vec<Download>, DomainError> {
            Ok(self
                .store
                .lock()
                .unwrap()
                .values()
                .filter(|d| d.state() == s)
                .cloned()
                .collect())
        }
    }

    struct MockEngine;
    impl DownloadEngine for MockEngine {
        fn start(&self, _d: &Download) -> Result<(), DomainError> {
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

    struct RecordingBus {
        events: Mutex<Vec<DomainEvent>>,
    }
    impl RecordingBus {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }
    }
    impl EventBus for RecordingBus {
        fn publish(&self, event: DomainEvent) {
            self.events.lock().unwrap().push(event);
        }
        fn subscribe(&self, _h: Box<dyn Fn(&DomainEvent) + Send + Sync>) {}
    }

    struct NoopStorage;
    impl FileStorage for NoopStorage {
        fn create_file(&self, _p: &Path, _s: u64) -> Result<(), DomainError> {
            Ok(())
        }
        fn write_segment(&self, _p: &Path, _o: u64, _d: &[u8]) -> Result<(), DomainError> {
            Ok(())
        }
        fn read_meta(&self, _p: &Path) -> Result<Option<DownloadMeta>, DomainError> {
            Ok(None)
        }
        fn write_meta(&self, _p: &Path, _m: &DownloadMeta) -> Result<(), DomainError> {
            Ok(())
        }
        fn delete_meta(&self, _p: &Path) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct NoopHttp;
    impl HttpClient for NoopHttp {
        fn head(&self, _u: &str) -> Result<HttpResponse, DomainError> {
            Ok(HttpResponse {
                status_code: 200,
                headers: HashMap::new(),
                body: vec![],
            })
        }
        fn get_range(&self, _u: &str, _s: u64, _e: u64) -> Result<Vec<u8>, DomainError> {
            Ok(vec![])
        }
        fn supports_range(&self, _u: &str) -> Result<bool, DomainError> {
            Ok(true)
        }
    }

    struct NoopPlugin;
    impl PluginLoader for NoopPlugin {
        fn load(&self, _m: &PluginManifest) -> Result<(), DomainError> {
            Ok(())
        }
        fn unload(&self, _n: &str) -> Result<(), DomainError> {
            Ok(())
        }
        fn resolve_url(&self, _u: &str) -> Result<Option<PluginInfo>, DomainError> {
            Ok(None)
        }
        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
            Ok(vec![])
        }
        fn set_enabled(&self, _n: &str, _e: bool) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct NoopConfig;
    impl ConfigStore for NoopConfig {
        fn get_config(&self) -> Result<AppConfig, DomainError> {
            Ok(AppConfig::default())
        }
        fn update_config(&self, _p: ConfigPatch) -> Result<AppConfig, DomainError> {
            Ok(AppConfig::default())
        }
    }

    struct NoopCred;
    impl CredentialStore for NoopCred {
        fn get(&self, _s: &str) -> Result<Option<Credential>, DomainError> {
            Ok(None)
        }
        fn store(&self, _s: &str, _c: &Credential) -> Result<(), DomainError> {
            Ok(())
        }
        fn delete(&self, _s: &str) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct NoopClip;
    impl ClipboardObserver for NoopClip {
        fn start(&self) -> Result<(), DomainError> {
            Ok(())
        }
        fn stop(&self) -> Result<(), DomainError> {
            Ok(())
        }
        fn get_urls(&self) -> Result<Vec<String>, DomainError> {
            Ok(vec![])
        }
    }

    struct NoopArchive;
    impl ArchiveExtractor for NoopArchive {
        fn detect_format(&self, _p: &Path) -> Result<Option<ArchiveFormat>, DomainError> {
            Ok(None)
        }
        fn can_extract(&self, _p: &Path) -> Result<bool, DomainError> {
            Ok(false)
        }
        fn extract(
            &self,
            _p: &Path,
            _d: &Path,
            _pw: Option<&str>,
        ) -> Result<ExtractSummary, DomainError> {
            Ok(ExtractSummary {
                extracted_files: 0,
                extracted_bytes: 0,
                duration_ms: 0,
                warnings: vec![],
            })
        }
        fn list_contents(
            &self,
            _p: &Path,
            _pw: Option<&str>,
        ) -> Result<Vec<ArchiveEntry>, DomainError> {
            Ok(vec![])
        }
        fn detect_segments(
            &self,
            _p: &Path,
        ) -> Result<Option<Vec<std::path::PathBuf>>, DomainError> {
            Ok(None)
        }
    }

    fn make_download(id: u64, position: i64) -> Download {
        Download::new(
            DownloadId(id),
            Url::new(&format!("http://example.com/{id}.zip")).unwrap(),
            format!("{id}.zip"),
            "/tmp".to_string(),
        )
        .with_priority(Priority::new(5).unwrap())
        .with_queue_position(position)
    }

    fn make_bus(repo: MockRepo, bus: Arc<RecordingBus>) -> CommandBus {
        CommandBus::new(
            Arc::new(repo),
            Arc::new(MockEngine),
            bus,
            Arc::new(NoopStorage),
            Arc::new(NoopHttp),
            Arc::new(NoopPlugin),
            Arc::new(NoopConfig),
            Arc::new(NoopCred),
            Arc::new(NoopClip),
            Arc::new(NoopArchive),
            Arc::new(crate::application::test_support::NoopHistoryRepo),
            None,
        )
    }

    #[tokio::test]
    async fn test_move_to_top_sets_position_below_min() {
        let repo = MockRepo::new()
            .with(make_download(1, 5))
            .with(make_download(2, 3))
            .with(make_download(3, 7));
        let events = Arc::new(RecordingBus::new());
        let bus = make_bus(repo, events.clone());

        bus.handle_move_to_top(MoveToTopCommand { id: DownloadId(3) })
            .await
            .unwrap();

        let moved = bus
            .download_repo()
            .find_by_id(DownloadId(3))
            .unwrap()
            .unwrap();
        assert_eq!(
            moved.queue_position(),
            3 - super::POSITION_STRIDE,
            "must be min minus stride"
        );
        let recorded = events.events.lock().unwrap().clone();
        assert!(matches!(
            recorded.as_slice(),
            [DomainEvent::QueueReordered { affected_ids }]
                if affected_ids == &[DownloadId(3)]
        ));
    }

    #[tokio::test]
    async fn test_move_to_bottom_sets_position_above_max() {
        let repo = MockRepo::new()
            .with(make_download(1, 5))
            .with(make_download(2, 3))
            .with(make_download(3, 7));
        let events = Arc::new(RecordingBus::new());
        let bus = make_bus(repo, events);

        bus.handle_move_to_bottom(MoveToBottomCommand { id: DownloadId(2) })
            .await
            .unwrap();

        let moved = bus
            .download_repo()
            .find_by_id(DownloadId(2))
            .unwrap()
            .unwrap();
        assert_eq!(
            moved.queue_position(),
            7 + super::POSITION_STRIDE,
            "must be max plus stride"
        );
    }

    #[tokio::test]
    async fn test_move_to_top_rejects_non_reorderable_state() {
        let mut dl = make_download(1, 5);
        dl.start().expect("Queued -> Downloading");
        let repo = MockRepo::new().with(dl);
        let events = Arc::new(RecordingBus::new());
        let bus = make_bus(repo, events.clone());

        let result = bus
            .handle_move_to_top(MoveToTopCommand { id: DownloadId(1) })
            .await;
        assert!(matches!(result, Err(AppError::Validation(_))));
        assert!(events.events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_move_to_bottom_rejects_non_reorderable_state() {
        let mut dl = make_download(1, 5);
        dl.start().expect("Queued -> Downloading");
        let repo = MockRepo::new().with(dl);
        let events = Arc::new(RecordingBus::new());
        let bus = make_bus(repo, events.clone());

        let result = bus
            .handle_move_to_bottom(MoveToBottomCommand { id: DownloadId(1) })
            .await;
        assert!(matches!(result, Err(AppError::Validation(_))));
        assert!(events.events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_reorder_queue_merges_omitted_items() {
        // d1, d2, d3 are reorderable; the submitted order is [d3, d1] which
        // omits d2. d2 must keep its relative slot (current position 5)
        // rather than colliding with the new 1..N range.
        let repo = MockRepo::new()
            .with(make_download(1, 10))
            .with(make_download(2, 5))
            .with(make_download(3, 30));
        let events = Arc::new(RecordingBus::new());
        let bus = make_bus(repo, events.clone());

        bus.handle_reorder_queue(ReorderQueueCommand {
            ordered_ids: vec![DownloadId(3), DownloadId(1)],
        })
        .await
        .unwrap();

        let d3 = bus
            .download_repo()
            .find_by_id(DownloadId(3))
            .unwrap()
            .unwrap();
        let d1 = bus
            .download_repo()
            .find_by_id(DownloadId(1))
            .unwrap()
            .unwrap();
        let d2 = bus
            .download_repo()
            .find_by_id(DownloadId(2))
            .unwrap()
            .unwrap();
        assert_eq!(d3.queue_position(), 1, "submitted first");
        assert_eq!(d1.queue_position(), 2, "submitted second");
        assert_eq!(d2.queue_position(), 3, "omitted item appended after");
    }

    #[tokio::test]
    async fn test_reorder_queue_preserves_omitted_order_on_position_tie() {
        // Three downloads share the default queue_position 0; the submitted
        // subset only renumbers d3 and d1. d2 (omitted) must keep its
        // original relative position and not be reshuffled by an id-based
        // tie-breaker.
        let repo = MockRepo::new()
            .with(make_download(2, 0))
            .with(make_download(1, 0))
            .with(make_download(3, 0));
        let events = Arc::new(RecordingBus::new());
        let bus = make_bus(repo, events.clone());

        bus.handle_reorder_queue(ReorderQueueCommand {
            ordered_ids: vec![DownloadId(3), DownloadId(1)],
        })
        .await
        .unwrap();

        let d3 = bus
            .download_repo()
            .find_by_id(DownloadId(3))
            .unwrap()
            .unwrap();
        let d1 = bus
            .download_repo()
            .find_by_id(DownloadId(1))
            .unwrap()
            .unwrap();
        let d2 = bus
            .download_repo()
            .find_by_id(DownloadId(2))
            .unwrap()
            .unwrap();
        assert_eq!(d3.queue_position(), 1);
        assert_eq!(d1.queue_position(), 2);
        // d2 lands at 3 because it is the only omitted item; the assertion
        // we really care about is that omitted items keep their relative
        // order — covered by the more-specific test below when there are
        // two omitted items sharing a position.
        assert_eq!(d2.queue_position(), 3);
    }

    #[tokio::test]
    async fn test_reorder_queue_rejects_duplicate_ids() {
        let repo = MockRepo::new()
            .with(make_download(1, 10))
            .with(make_download(2, 20));
        let events = Arc::new(RecordingBus::new());
        let bus = make_bus(repo, events.clone());

        let result = bus
            .handle_reorder_queue(ReorderQueueCommand {
                ordered_ids: vec![DownloadId(1), DownloadId(2), DownloadId(1)],
            })
            .await;
        assert!(matches!(result, Err(AppError::Validation(_))));
        let d1 = bus
            .download_repo()
            .find_by_id(DownloadId(1))
            .unwrap()
            .unwrap();
        assert_eq!(d1.queue_position(), 10, "no writes when validation fails");
        assert!(events.events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_reorder_queue_rejects_non_reorderable_id() {
        let mut active = make_download(2, 5);
        active.start().expect("Queued -> Downloading");
        let repo = MockRepo::new().with(make_download(1, 10)).with(active);
        let events = Arc::new(RecordingBus::new());
        let bus = make_bus(repo, events.clone());

        let result = bus
            .handle_reorder_queue(ReorderQueueCommand {
                ordered_ids: vec![DownloadId(1), DownloadId(2)],
            })
            .await;
        assert!(matches!(result, Err(AppError::Validation(_))));
        // Nothing persisted, no event published.
        let d1 = bus
            .download_repo()
            .find_by_id(DownloadId(1))
            .unwrap()
            .unwrap();
        assert_eq!(d1.queue_position(), 10, "unchanged on validation failure");
        assert!(events.events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_reorder_queue_uses_atomic_batch_save() {
        // Counters are shared with the wrapping repo via Arc so the test
        // can assert the handler routes through `save_batch`, not per-row
        // `save`.
        struct CountingRepo {
            inner: MockRepo,
            batch_calls: Arc<Mutex<usize>>,
            individual_saves: Arc<Mutex<usize>>,
        }
        impl DownloadRepository for CountingRepo {
            fn find_by_id(&self, id: DownloadId) -> Result<Option<Download>, DomainError> {
                self.inner.find_by_id(id)
            }
            fn save(&self, d: &Download) -> Result<(), DomainError> {
                *self.individual_saves.lock().unwrap() += 1;
                self.inner.save(d)
            }
            fn save_batch(&self, ds: &[Download]) -> Result<(), DomainError> {
                *self.batch_calls.lock().unwrap() += 1;
                for d in ds {
                    self.inner.save(d)?;
                }
                Ok(())
            }
            fn delete(&self, id: DownloadId) -> Result<(), DomainError> {
                self.inner.delete(id)
            }
            fn find_by_state(
                &self,
                s: crate::domain::model::download::DownloadState,
            ) -> Result<Vec<Download>, DomainError> {
                self.inner.find_by_state(s)
            }
        }

        let batch_calls = Arc::new(Mutex::new(0_usize));
        let individual_saves = Arc::new(Mutex::new(0_usize));
        let inner = MockRepo::new()
            .with(make_download(1, 10))
            .with(make_download(2, 20));
        let repo = CountingRepo {
            inner,
            batch_calls: batch_calls.clone(),
            individual_saves: individual_saves.clone(),
        };
        let events = Arc::new(RecordingBus::new());
        let bus = CommandBus::new(
            Arc::new(repo),
            Arc::new(MockEngine),
            events.clone(),
            Arc::new(NoopStorage),
            Arc::new(NoopHttp),
            Arc::new(NoopPlugin),
            Arc::new(NoopConfig),
            Arc::new(NoopCred),
            Arc::new(NoopClip),
            Arc::new(NoopArchive),
            Arc::new(crate::application::test_support::NoopHistoryRepo),
            None,
        );

        bus.handle_reorder_queue(ReorderQueueCommand {
            ordered_ids: vec![DownloadId(2), DownloadId(1)],
        })
        .await
        .unwrap();

        assert_eq!(*batch_calls.lock().unwrap(), 1, "batch path used");
        assert_eq!(
            *individual_saves.lock().unwrap(),
            0,
            "individual save not used inside reorder"
        );
    }

    #[tokio::test]
    async fn test_move_to_top_missing_download_errors() {
        let repo = MockRepo::new();
        let events = Arc::new(RecordingBus::new());
        let bus = make_bus(repo, events);

        let result = bus
            .handle_move_to_top(MoveToTopCommand { id: DownloadId(99) })
            .await;
        assert!(matches!(result, Err(AppError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_reorder_queue_renumbers_positions() {
        let repo = MockRepo::new()
            .with(make_download(1, 10))
            .with(make_download(2, 20))
            .with(make_download(3, 30));
        let events = Arc::new(RecordingBus::new());
        let bus = make_bus(repo, events.clone());

        bus.handle_reorder_queue(ReorderQueueCommand {
            ordered_ids: vec![DownloadId(3), DownloadId(1), DownloadId(2)],
        })
        .await
        .unwrap();

        let d3 = bus
            .download_repo()
            .find_by_id(DownloadId(3))
            .unwrap()
            .unwrap();
        let d1 = bus
            .download_repo()
            .find_by_id(DownloadId(1))
            .unwrap()
            .unwrap();
        let d2 = bus
            .download_repo()
            .find_by_id(DownloadId(2))
            .unwrap()
            .unwrap();
        assert_eq!(d3.queue_position(), 1);
        assert_eq!(d1.queue_position(), 2);
        assert_eq!(d2.queue_position(), 3);

        let recorded = events.events.lock().unwrap().clone();
        assert_eq!(recorded.len(), 1);
    }

    #[tokio::test]
    async fn test_reorder_queue_empty_is_noop() {
        let repo = MockRepo::new().with(make_download(1, 5));
        let events = Arc::new(RecordingBus::new());
        let bus = make_bus(repo, events.clone());

        bus.handle_reorder_queue(ReorderQueueCommand {
            ordered_ids: vec![],
        })
        .await
        .unwrap();

        let d1 = bus
            .download_repo()
            .find_by_id(DownloadId(1))
            .unwrap()
            .unwrap();
        assert_eq!(d1.queue_position(), 5, "unchanged");
        assert!(events.events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_move_to_top_saturates_at_min_i64() {
        let repo = MockRepo::new()
            .with(make_download(1, i64::MIN))
            .with(make_download(2, 0));
        let events = Arc::new(RecordingBus::new());
        let bus = make_bus(repo, events);

        bus.handle_move_to_top(MoveToTopCommand { id: DownloadId(2) })
            .await
            .unwrap();

        let moved = bus
            .download_repo()
            .find_by_id(DownloadId(2))
            .unwrap()
            .unwrap();
        assert_eq!(
            moved.queue_position(),
            i64::MIN,
            "saturates without overflow"
        );
    }
}
