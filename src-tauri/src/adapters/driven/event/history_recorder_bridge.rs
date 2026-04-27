//! Subscribes to `DownloadCompletedPersisted` and writes a row into the
//! `history` read model so the History view (PRD §6.8), the
//! `download_redownload {sourceKind:"history"}` flow (P0.9) and the
//! retention purge worker (P0.14) all have data to operate on.
//!
//! Without this bridge `HistoryRepository::record` has no production
//! caller, so even after dozens of completed downloads the `history`
//! table stays empty.
//!
//! The projection is computed from the snapshot carried on the event so
//! a concurrent `clear` / `remove` / `change-directory` cannot race the
//! recorder by mutating the persisted row between publish and callback.

use std::sync::Arc;

use crate::domain::event::DomainEvent;
use crate::domain::ports::driven::{EventBus, HistoryRepository};

/// Wire a history recorder onto the event bus.
///
/// Listens for `DownloadCompletedPersisted` (post-persist event from
/// `QueueManager`), projects the carried snapshot into a `HistoryEntry`
/// and calls `HistoryRepository::record`.
///
/// History-write failures are swallowed with `tracing::warn!` — a
/// history glitch must never propagate back into the queue/UI flow.
pub fn spawn_history_recorder_bridge(
    event_bus: &dyn EventBus,
    history_repo: Arc<dyn HistoryRepository>,
) {
    event_bus.subscribe(Box::new(move |event: &DomainEvent| {
        record_for_event(history_repo.as_ref(), event);
    }));
}

fn record_for_event(history_repo: &dyn HistoryRepository, event: &DomainEvent) {
    let DomainEvent::DownloadCompletedPersisted { snapshot, .. } = event else {
        return;
    };
    let entry = snapshot.to_history_entry();
    if let Err(e) = history_repo.record(&entry) {
        tracing::warn!(
            error = %e,
            download_id = snapshot.id.0,
            "history bridge: record failed",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::DomainError;
    use crate::domain::event::DownloadCompletedSnapshot;
    use crate::domain::model::download::DownloadId;
    use crate::domain::model::views::{HistoryEntry, HistoryFilter, HistorySort};
    use std::sync::Mutex;

    fn make_snapshot(
        id: u64,
        created_at_ms: u64,
        updated_at_ms: u64,
        file_size_bytes: Option<u64>,
        downloaded_bytes: u64,
    ) -> DownloadCompletedSnapshot {
        DownloadCompletedSnapshot {
            id: DownloadId(id),
            file_name: "file.zip".to_string(),
            url: "https://example.com/file.zip".to_string(),
            destination_path: "/tmp/file.zip".to_string(),
            file_size_bytes,
            downloaded_bytes,
            created_at_ms,
            updated_at_ms,
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
        let snapshot = make_snapshot(7, 100_000, 110_000, Some(2_000), 0);
        let history_repo = Arc::new(RecordingHistoryRepo::default());

        record_for_event(
            history_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted {
                id: snapshot.id,
                snapshot,
            },
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
        let snapshot = make_snapshot(8, 100_000, 110_000, None, 1_500);
        let history_repo = Arc::new(RecordingHistoryRepo::default());

        record_for_event(
            history_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted {
                id: snapshot.id,
                snapshot,
            },
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
        let snapshot = make_snapshot(9, 100_000, 100_000, Some(500), 0);
        let history_repo = Arc::new(RecordingHistoryRepo::default());

        record_for_event(
            history_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted {
                id: snapshot.id,
                snapshot,
            },
        );

        let calls = history_repo.calls();
        assert_eq!(calls[0].duration_seconds, 1);
        assert_eq!(calls[0].avg_speed, 500);
    }

    #[test]
    fn test_record_for_event_ignores_non_completed_persisted_events() {
        let history_repo = Arc::new(RecordingHistoryRepo::default());

        record_for_event(
            history_repo.as_ref(),
            &DomainEvent::DownloadStarted { id: DownloadId(7) },
        );
        record_for_event(
            history_repo.as_ref(),
            &DomainEvent::DownloadProgress {
                id: DownloadId(7),
                downloaded_bytes: 1,
                total_bytes: 0,
            },
        );
        record_for_event(
            history_repo.as_ref(),
            &DomainEvent::DownloadFailed {
                id: DownloadId(7),
                error: "boom".to_string(),
            },
        );

        assert!(history_repo.calls().is_empty());
    }

    #[test]
    fn test_record_for_event_is_immune_to_repo_mutation_after_publish() {
        // Race scenario: even if a `clear` / `remove` / `change-directory`
        // happened between publish and callback, the snapshot is frozen.
        let snapshot = make_snapshot(13, 0, 5_000, Some(2_500), 0);
        let history_repo = Arc::new(RecordingHistoryRepo::default());

        record_for_event(
            history_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted {
                id: snapshot.id,
                snapshot,
            },
        );

        let calls = history_repo.calls();
        assert_eq!(calls.len(), 1);
        // Projection observed the values from the snapshot, not a re-read.
        assert_eq!(calls[0].total_bytes, 2_500);
        assert_eq!(calls[0].destination_path, "/tmp/file.zip");
    }

    #[test]
    fn test_record_for_event_swallows_history_repo_error() {
        let snapshot = make_snapshot(7, 0, 5_000, Some(100), 0);
        let history_repo = Arc::new(RecordingHistoryRepo::default());
        history_repo.fail_next(DomainError::StorageError("disk full".to_string()));

        record_for_event(
            history_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted {
                id: snapshot.id,
                snapshot,
            },
        );

        // First call failed; nothing recorded.
        assert!(history_repo.calls().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_spawn_bridge_writes_through_event_bus() {
        use crate::adapters::driven::event::TokioEventBus;
        use std::time::Duration;

        let snapshot = make_snapshot(42, 1_000, 11_000, Some(8_000), 0);
        let history_repo = Arc::new(RecordingHistoryRepo::default());
        let bus = TokioEventBus::new(16);

        spawn_history_recorder_bridge(&bus, history_repo.clone());
        bus.publish(DomainEvent::DownloadCompletedPersisted {
            id: snapshot.id,
            snapshot,
        });

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
        use crate::adapters::driven::sqlite::history_repo::SqliteHistoryRepo;
        use std::time::Duration;

        let db = setup_test_db().await.expect("test db");
        let history_repo: Arc<dyn HistoryRepository> = Arc::new(SqliteHistoryRepo::new(db.clone()));

        let bus = TokioEventBus::new(16);
        spawn_history_recorder_bridge(&bus, history_repo.clone());

        let snapshot = DownloadCompletedSnapshot {
            id: DownloadId(101),
            file_name: "file.zip".to_string(),
            url: "https://example.com/file.zip".to_string(),
            destination_path: "/tmp/file.zip".to_string(),
            file_size_bytes: Some(50_000),
            downloaded_bytes: 50_000,
            created_at_ms: 0,
            updated_at_ms: 1_000,
        };
        bus.publish(DomainEvent::DownloadCompletedPersisted {
            id: snapshot.id,
            snapshot,
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
