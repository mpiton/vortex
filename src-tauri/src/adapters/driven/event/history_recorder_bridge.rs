//! Subscribes to `DownloadCompletedPersisted` and writes a row into the
//! `history` read model so the History view (PRD §6.8), the
//! `download_redownload {sourceKind:"history"}` flow (P0.9) and the
//! retention purge worker (P0.14) all have data to operate on.
//!
//! Without this bridge `HistoryRepository::record` has no production
//! caller, so even after dozens of completed downloads the `history`
//! table stays empty.

use std::sync::Arc;

use crate::domain::event::DomainEvent;
use crate::domain::model::download::Download;
use crate::domain::model::views::HistoryEntry;
use crate::domain::ports::driven::{DownloadRepository, EventBus, HistoryRepository};

/// Wire a history recorder onto the event bus.
///
/// Listens for `DownloadCompletedPersisted` (post-persist event from
/// `QueueManager`), loads the matching `Download` aggregate, projects it
/// into a `HistoryEntry` and calls `HistoryRepository::record`.
///
/// All errors are swallowed with `tracing::warn!` — a history-write
/// failure must never propagate back into the queue/UI flow.
pub fn spawn_history_recorder_bridge(
    event_bus: &dyn EventBus,
    download_repo: Arc<dyn DownloadRepository>,
    history_repo: Arc<dyn HistoryRepository>,
) {
    event_bus.subscribe(Box::new(move |event: &DomainEvent| {
        record_for_event(download_repo.as_ref(), history_repo.as_ref(), event);
    }));
}

fn record_for_event(
    download_repo: &dyn DownloadRepository,
    history_repo: &dyn HistoryRepository,
    event: &DomainEvent,
) {
    let DomainEvent::DownloadCompletedPersisted { id } = event else {
        return;
    };
    let download = match download_repo.find_by_id(*id) {
        Ok(Some(d)) => d,
        Ok(None) => {
            tracing::warn!(
                download_id = id.0,
                "history bridge: download not found for completed event, skipping",
            );
            return;
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                download_id = id.0,
                "history bridge: failed to load download for completed event",
            );
            return;
        }
    };
    let entry = derive_history_entry(&download);
    if let Err(e) = history_repo.record(&entry) {
        tracing::warn!(
            error = %e,
            download_id = id.0,
            "history bridge: record failed",
        );
    }
}

/// Project a completed `Download` aggregate into a `HistoryEntry`.
///
/// `total_bytes` follows the same fallback as the stats bridge:
/// authoritative `file_size` first, running `downloaded_bytes` second.
///
/// `completed_at` and `duration_seconds` come from `created_at` /
/// `updated_at` which are stored in milliseconds (see
/// `current_timestamp_ms`). `HistoryEntry::completed_at` is documented
/// as Unix seconds, hence the `/ 1_000` conversion. `duration_seconds`
/// is clamped to a `1`-second floor so very short transfers never
/// divide by zero in `avg_speed`.
fn derive_history_entry(download: &Download) -> HistoryEntry {
    let total_bytes = download
        .file_size()
        .map(|fs| fs.0)
        .filter(|n| *n > 0)
        .unwrap_or_else(|| download.downloaded_bytes());
    let elapsed_ms = download.updated_at().saturating_sub(download.created_at());
    let duration_seconds = (elapsed_ms / 1_000).max(1);
    let avg_speed = total_bytes / duration_seconds;
    let completed_at = download.updated_at() / 1_000;
    HistoryEntry {
        id: 0,
        download_id: download.id(),
        file_name: download.file_name().to_string(),
        url: download.url().as_str().to_string(),
        total_bytes,
        completed_at,
        duration_seconds,
        avg_speed,
        destination_path: download.destination_path().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::DomainError;
    use crate::domain::model::download::{Download, DownloadId, DownloadState, FileSize, Url};
    use crate::domain::model::queue::Priority;
    use crate::domain::model::views::{HistoryFilter, HistorySort};
    use std::sync::Mutex;

    fn make_download(
        id: u64,
        created_at: u64,
        updated_at: u64,
        size_bytes: Option<u64>,
    ) -> Download {
        let url = Url::new("https://example.com/file.zip").expect("valid url");
        let mut d = Download::reconstruct(
            DownloadId(id),
            url,
            "file.zip".to_string(),
            size_bytes.map(FileSize),
            size_bytes.unwrap_or(0),
            DownloadState::Completed,
            Priority::default(),
            0,
            0,
            5,
            1,
            None,
            None,
            None,
            "example.com".to_string(),
            "https".to_string(),
            true,
            None,
            None,
            "/tmp/file.zip".to_string(),
            created_at,
            updated_at,
        );
        if size_bytes.is_none() {
            d.update_progress(0);
        }
        d
    }

    struct StubDownloadRepo {
        result: Mutex<Result<Option<Download>, DomainError>>,
    }

    impl StubDownloadRepo {
        fn returning(result: Result<Option<Download>, DomainError>) -> Arc<Self> {
            Arc::new(Self {
                result: Mutex::new(result),
            })
        }
    }

    impl DownloadRepository for StubDownloadRepo {
        fn find_by_id(&self, _: DownloadId) -> Result<Option<Download>, DomainError> {
            match &*self.result.lock().expect("stub mutex") {
                Ok(Some(d)) => Ok(Some(d.clone())),
                Ok(None) => Ok(None),
                Err(e) => Err(e.clone()),
            }
        }

        fn save(&self, _: &Download) -> Result<(), DomainError> {
            unreachable!("bridge must never write through the download repo")
        }

        fn delete(&self, _: DownloadId) -> Result<(), DomainError> {
            unreachable!("bridge must never delete through the download repo")
        }

        fn find_by_state(&self, _: DownloadState) -> Result<Vec<Download>, DomainError> {
            unreachable!("bridge must never enumerate by state")
        }
    }

    #[derive(Default)]
    struct RecordingHistoryRepo {
        calls: Mutex<Vec<HistoryEntry>>,
        fail_with: Mutex<Option<DomainError>>,
    }

    impl RecordingHistoryRepo {
        fn calls(&self) -> Vec<HistoryEntry> {
            self.calls.lock().expect("calls mutex").clone()
        }

        fn fail_next(self: &Arc<Self>, err: DomainError) {
            *self.fail_with.lock().expect("fail mutex") = Some(err);
        }
    }

    impl HistoryRepository for RecordingHistoryRepo {
        fn record(&self, entry: &HistoryEntry) -> Result<(), DomainError> {
            if let Some(e) = self.fail_with.lock().expect("fail mutex").take() {
                return Err(e);
            }
            self.calls.lock().expect("calls mutex").push(entry.clone());
            Ok(())
        }

        fn find_recent(&self, _: usize) -> Result<Vec<HistoryEntry>, DomainError> {
            unreachable!("bridge must not read history")
        }

        fn find_by_download(&self, _: DownloadId) -> Result<Vec<HistoryEntry>, DomainError> {
            unreachable!("bridge must not read history")
        }

        fn list(
            &self,
            _: Option<HistoryFilter>,
            _: Option<HistorySort>,
            _: Option<usize>,
            _: Option<usize>,
        ) -> Result<Vec<HistoryEntry>, DomainError> {
            unreachable!("bridge must not read history")
        }

        fn search(&self, _: &str) -> Result<Vec<HistoryEntry>, DomainError> {
            unreachable!("bridge must not read history")
        }

        fn find_by_id(&self, _: u64) -> Result<Option<HistoryEntry>, DomainError> {
            unreachable!("bridge must not read history")
        }

        fn delete_by_id(&self, _: u64) -> Result<bool, DomainError> {
            unreachable!("bridge must not delete history")
        }

        fn delete_all(&self) -> Result<u64, DomainError> {
            unreachable!("bridge must not delete history")
        }

        fn delete_older_than(&self, _: u64) -> Result<u64, DomainError> {
            unreachable!("bridge must not purge history")
        }
    }

    #[test]
    fn test_record_for_event_writes_history_entry_with_file_size() {
        // created_at=100_000ms, updated_at=110_000ms → 10 s elapsed.
        // file_size = 2_000 → avg_speed = 2_000 / 10 = 200.
        // completed_at = 110_000 / 1_000 = 110 (Unix seconds).
        let download = make_download(7, 100_000, 110_000, Some(2_000));
        let download_repo = StubDownloadRepo::returning(Ok(Some(download)));
        let history_repo = Arc::new(RecordingHistoryRepo::default());

        record_for_event(
            download_repo.as_ref(),
            history_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted { id: DownloadId(7) },
        );

        let calls = history_repo.calls();
        assert_eq!(calls.len(), 1);
        let entry = &calls[0];
        assert_eq!(entry.id, 0);
        assert_eq!(entry.download_id, DownloadId(7));
        assert_eq!(entry.file_name, "file.zip");
        assert_eq!(entry.url, "https://example.com/file.zip");
        assert_eq!(entry.total_bytes, 2_000);
        assert_eq!(entry.completed_at, 110);
        assert_eq!(entry.duration_seconds, 10);
        assert_eq!(entry.avg_speed, 200);
        assert_eq!(entry.destination_path, "/tmp/file.zip");
    }

    #[test]
    fn test_record_for_event_falls_back_to_downloaded_bytes_when_size_unknown() {
        let mut download = make_download(8, 100_000, 110_000, None);
        download.update_progress(1_500);
        let download_repo = StubDownloadRepo::returning(Ok(Some(download)));
        let history_repo = Arc::new(RecordingHistoryRepo::default());

        record_for_event(
            download_repo.as_ref(),
            history_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted { id: DownloadId(8) },
        );

        let calls = history_repo.calls();
        assert_eq!(calls.len(), 1);
        // total_bytes pulled from downloaded_bytes since file_size is None.
        assert_eq!(calls[0].total_bytes, 1_500);
        assert_eq!(calls[0].avg_speed, 150); // 1_500 / 10s
    }

    #[test]
    fn test_record_for_event_clamps_duration_to_one_second_floor() {
        // updated_at == created_at → elapsed = 0 ms.
        // duration_seconds must clamp to 1, avg_speed must not divide by zero.
        let download = make_download(9, 100_000, 100_000, Some(500));
        let download_repo = StubDownloadRepo::returning(Ok(Some(download)));
        let history_repo = Arc::new(RecordingHistoryRepo::default());

        record_for_event(
            download_repo.as_ref(),
            history_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted { id: DownloadId(9) },
        );

        let calls = history_repo.calls();
        assert_eq!(calls[0].duration_seconds, 1);
        assert_eq!(calls[0].avg_speed, 500);
    }

    #[test]
    fn test_record_for_event_ignores_non_completed_persisted_events() {
        let download_repo = StubDownloadRepo::returning(Ok(None));
        let history_repo = Arc::new(RecordingHistoryRepo::default());

        record_for_event(
            download_repo.as_ref(),
            history_repo.as_ref(),
            &DomainEvent::DownloadStarted { id: DownloadId(7) },
        );
        record_for_event(
            download_repo.as_ref(),
            history_repo.as_ref(),
            &DomainEvent::DownloadProgress {
                id: DownloadId(7),
                downloaded_bytes: 1,
                total_bytes: 0,
            },
        );
        record_for_event(
            download_repo.as_ref(),
            history_repo.as_ref(),
            &DomainEvent::DownloadFailed {
                id: DownloadId(7),
                error: "boom".to_string(),
            },
        );

        assert!(history_repo.calls().is_empty());
    }

    #[test]
    fn test_record_for_event_skips_when_download_not_found() {
        let download_repo = StubDownloadRepo::returning(Ok(None));
        let history_repo = Arc::new(RecordingHistoryRepo::default());

        record_for_event(
            download_repo.as_ref(),
            history_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted { id: DownloadId(7) },
        );

        assert!(history_repo.calls().is_empty());
    }

    #[test]
    fn test_record_for_event_swallows_repo_error() {
        let download = make_download(7, 0, 1_000, Some(100));
        let download_repo =
            StubDownloadRepo::returning(Err(DomainError::StorageError("db down".to_string())));
        let history_repo = Arc::new(RecordingHistoryRepo::default());

        // Must not panic — the queue must keep flowing even when SQLite is sad.
        record_for_event(
            download_repo.as_ref(),
            history_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted { id: DownloadId(7) },
        );
        record_for_event(
            StubDownloadRepo::returning(Ok(Some(download))).as_ref(),
            history_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted { id: DownloadId(7) },
        );

        assert_eq!(history_repo.calls().len(), 1);
    }

    #[test]
    fn test_record_for_event_swallows_history_repo_error() {
        let download = make_download(7, 0, 5_000, Some(100));
        let download_repo = StubDownloadRepo::returning(Ok(Some(download)));
        let history_repo = Arc::new(RecordingHistoryRepo::default());
        history_repo.fail_next(DomainError::StorageError("disk full".to_string()));

        record_for_event(
            download_repo.as_ref(),
            history_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted { id: DownloadId(7) },
        );

        // First call failed; nothing recorded.
        assert!(history_repo.calls().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_spawn_bridge_writes_through_event_bus() {
        use crate::adapters::driven::event::TokioEventBus;
        use std::time::Duration;

        let download = make_download(42, 1_000, 11_000, Some(8_000));
        let download_repo = StubDownloadRepo::returning(Ok(Some(download)));
        let history_repo = Arc::new(RecordingHistoryRepo::default());
        let bus = TokioEventBus::new(16);

        spawn_history_recorder_bridge(&bus, download_repo, history_repo.clone());
        bus.publish(DomainEvent::DownloadCompletedPersisted { id: DownloadId(42) });

        // Subscriber is async — poll until the call lands or fail after 1s.
        for _ in 0..50 {
            let calls = history_repo.calls();
            if !calls.is_empty() {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].download_id, DownloadId(42));
                assert_eq!(calls[0].total_bytes, 8_000);
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("history row never appeared after publish — bridge wiring is broken");
    }

    /// End-to-end smoke test against real SQLite. Locks down BUG #1
    /// (history table never populated).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_bridge_writes_to_real_sqlite_when_event_fires() {
        use crate::adapters::driven::event::tokio_event_bus::TokioEventBus;
        use crate::adapters::driven::sqlite::connection::setup_test_db;
        use crate::adapters::driven::sqlite::download_repo::SqliteDownloadRepo;
        use crate::adapters::driven::sqlite::history_repo::SqliteHistoryRepo;
        use std::time::Duration;

        let db = setup_test_db().await.expect("test db");
        let download_repo: Arc<dyn DownloadRepository> =
            Arc::new(SqliteDownloadRepo::new(db.clone()));
        let history_repo: Arc<dyn HistoryRepository> = Arc::new(SqliteHistoryRepo::new(db.clone()));

        let url = Url::new("https://example.com/file.zip").expect("valid url");
        let mut download = Download::new(
            DownloadId(101),
            url,
            "file.zip".to_string(),
            "/tmp".to_string(),
        );
        download.set_file_size(50_000);
        download_repo.save(&download).expect("save download");

        let bus = TokioEventBus::new(16);
        spawn_history_recorder_bridge(&bus, download_repo, history_repo.clone());

        bus.publish(DomainEvent::DownloadCompletedPersisted {
            id: DownloadId(101),
        });

        for _ in 0..50 {
            let rows = history_repo.find_recent(10).expect("find_recent");
            if let Some(entry) = rows.first() {
                assert_eq!(entry.download_id, DownloadId(101));
                assert_eq!(entry.total_bytes, 50_000);
                assert_eq!(entry.file_name, "file.zip");
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("history row never appeared after publish — bridge wiring is broken");
    }
}
