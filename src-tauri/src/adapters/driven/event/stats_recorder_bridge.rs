//! Subscribes to `DownloadCompletedPersisted` and feeds the
//! aggregated `statistics` read model so the Statistics view's
//! daily volume / total files / avg speed KPIs stay in sync with
//! the `downloads` table.
//!
//! Without this bridge the `statistics` table is never written and
//! the view reports zeros even though completed downloads exist
//! (issue #114).
//!
//! The projection is computed from the snapshot carried on the event so
//! a concurrent `clear` / `remove` / `change-directory` cannot race the
//! recorder by mutating the persisted row between publish and callback.

use std::sync::Arc;

use crate::domain::event::DomainEvent;
use crate::domain::ports::driven::{EventBus, StatsRepository};

/// Wire a stats recorder onto the event bus.
///
/// Listens for `DownloadCompletedPersisted` (post-persist event from
/// `QueueManager`), derives `(bytes, avg_speed)` from the carried
/// snapshot and calls `StatsRepository::record_completed`.
///
/// Stats-write failures are swallowed with `tracing::warn!` — a stats
/// glitch must never propagate back into the queue/UI flow.
pub fn spawn_stats_recorder_bridge(event_bus: &dyn EventBus, stats_repo: Arc<dyn StatsRepository>) {
    event_bus.subscribe(Box::new(move |event: &DomainEvent| {
        record_for_event(stats_repo.as_ref(), event);
    }));
}

fn record_for_event(stats_repo: &dyn StatsRepository, event: &DomainEvent) {
    let DomainEvent::DownloadCompletedPersisted { snapshot, .. } = event else {
        return;
    };
    let (bytes, avg_speed) = snapshot.to_stats_record();
    if let Err(e) = stats_repo.record_completed(bytes, avg_speed) {
        tracing::warn!(
            error = %e,
            download_id = snapshot.id.0,
            "stats bridge: record_completed failed",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::DomainError;
    use crate::domain::event::DownloadCompletedSnapshot;
    use crate::domain::model::download::DownloadId;
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
            destination_path: "/tmp".to_string(),
            file_size_bytes,
            downloaded_bytes,
            created_at_ms,
            updated_at_ms,
        }
    }

    fn make_event(snapshot: DownloadCompletedSnapshot) -> DomainEvent {
        DomainEvent::DownloadCompletedPersisted {
            id: snapshot.id,
            snapshot,
        }
    }

    #[derive(Default)]
    struct RecordingStatsRepo {
        calls: Mutex<Vec<(u64, u64)>>,
        fail_with: Mutex<Option<DomainError>>,
    }

    impl RecordingStatsRepo {
        fn calls(&self) -> Vec<(u64, u64)> {
            self.calls.lock().expect("calls mutex").clone()
        }

        fn fail_next(self: &Arc<Self>, err: DomainError) {
            *self.fail_with.lock().expect("fail mutex") = Some(err);
        }
    }

    impl StatsRepository for RecordingStatsRepo {
        fn record_completed(&self, bytes: u64, avg_speed: u64) -> Result<(), DomainError> {
            if let Some(e) = self.fail_with.lock().expect("fail mutex").take() {
                return Err(e);
            }
            self.calls
                .lock()
                .expect("calls mutex")
                .push((bytes, avg_speed));
            Ok(())
        }

        fn get_stats(
            &self,
            _: crate::domain::model::views::StatsPeriod,
        ) -> Result<crate::domain::model::views::StatsView, DomainError> {
            unreachable!("bridge must not read stats")
        }

        fn top_modules(
            &self,
            _: u32,
        ) -> Result<Vec<crate::domain::model::views::ModuleStats>, DomainError> {
            unreachable!("bridge must not query top modules")
        }
    }

    #[test]
    fn test_record_for_event_records_when_persisted_event_uses_file_size() {
        // elapsed_ms = 110_000 - 100_000 = 10_000 → 10s.
        let snapshot = make_snapshot(7, 100_000, 110_000, Some(2_000), 0);
        let stats_repo = Arc::new(RecordingStatsRepo::default());

        record_for_event(stats_repo.as_ref(), &make_event(snapshot));

        // bytes = file_size, avg_speed = 2000 / 10s = 200
        assert_eq!(stats_repo.calls(), vec![(2_000, 200)]);
    }

    #[test]
    fn test_record_for_event_falls_back_to_downloaded_bytes_when_size_unknown() {
        let snapshot = make_snapshot(8, 100_000, 110_000, None, 1_500);
        let stats_repo = Arc::new(RecordingStatsRepo::default());

        record_for_event(stats_repo.as_ref(), &make_event(snapshot));

        // bytes = downloaded_bytes (file_size was None), avg_speed = 1500 / 10s = 150
        assert_eq!(stats_repo.calls(), vec![(1_500, 150)]);
    }

    #[test]
    fn test_record_for_event_avoids_division_by_zero_on_instant_completion() {
        let snapshot = make_snapshot(9, 200_000, 200_000, Some(4_096), 0);
        let stats_repo = Arc::new(RecordingStatsRepo::default());

        record_for_event(stats_repo.as_ref(), &make_event(snapshot));

        // elapsed_ms = 0 → elapsed_secs clamped to 1 → avg_speed = bytes / 1
        assert_eq!(stats_repo.calls(), vec![(4_096, 4_096)]);
    }

    #[test]
    fn test_record_for_event_clamps_sub_second_elapsed_to_one_second() {
        // 500 ms elapsed: elapsed_ms / 1_000 = 0, clamped to 1 second.
        let snapshot = make_snapshot(11, 0, 500, Some(2_048), 0);
        let stats_repo = Arc::new(RecordingStatsRepo::default());

        record_for_event(stats_repo.as_ref(), &make_event(snapshot));

        assert_eq!(stats_repo.calls(), vec![(2_048, 2_048)]);
    }

    #[test]
    fn test_record_for_event_ignores_unrelated_events() {
        let stats_repo = Arc::new(RecordingStatsRepo::default());

        for event in [
            DomainEvent::DownloadCompleted { id: DownloadId(1) },
            DomainEvent::DownloadStarted { id: DownloadId(1) },
            DomainEvent::DownloadFailed {
                id: DownloadId(1),
                error: "x".to_string(),
            },
            DomainEvent::SettingsUpdated,
        ] {
            record_for_event(stats_repo.as_ref(), &event);
        }

        assert!(stats_repo.calls().is_empty());
    }

    #[test]
    fn test_record_for_event_swallows_stats_repo_error() {
        let snapshot = make_snapshot(2, 0, 1, Some(100), 0);
        let stats_repo = Arc::new(RecordingStatsRepo::default());
        stats_repo.fail_next(DomainError::StorageError("disk full".to_string()));

        record_for_event(stats_repo.as_ref(), &make_event(snapshot));

        assert!(stats_repo.calls().is_empty());
    }

    #[tokio::test]
    async fn test_spawn_subscribes_to_event_bus() {
        use crate::adapters::driven::event::tokio_event_bus::TokioEventBus;
        use std::time::Duration;

        let bus = TokioEventBus::new(16);
        let snapshot = make_snapshot(42, 1_000_000, 1_010_000, Some(20_000), 0);
        let stats_repo = Arc::new(RecordingStatsRepo::default());

        spawn_stats_recorder_bridge(&bus, stats_repo.clone());

        bus.publish(make_event(snapshot));

        for _ in 0..50 {
            if !stats_repo.calls().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert_eq!(stats_repo.calls(), vec![(20_000, 2_000)]);
    }

    /// End-to-end smoke test: publish a `DownloadCompletedPersisted` on the
    /// real `TokioEventBus` and verify the row lands in a real in-memory
    /// SQLite `statistics` table. Locks down the regression behind issue #114.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_bridge_writes_to_real_sqlite_when_event_fires() {
        use crate::adapters::driven::event::tokio_event_bus::TokioEventBus;
        use crate::adapters::driven::sqlite::connection::setup_test_db;
        use crate::adapters::driven::sqlite::stats_repo::SqliteStatsRepo;
        use crate::domain::model::views::StatsPeriod;
        use std::time::Duration;

        let db = setup_test_db().await.expect("test db");
        let stats_repo: Arc<dyn StatsRepository> = Arc::new(SqliteStatsRepo::new(db.clone()));

        let bus = TokioEventBus::new(16);
        spawn_stats_recorder_bridge(&bus, stats_repo.clone());

        let snapshot = DownloadCompletedSnapshot {
            id: DownloadId(101),
            file_name: "file.zip".to_string(),
            url: "https://example.com/file.zip".to_string(),
            destination_path: "/tmp".to_string(),
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
            let stats = stats_repo
                .get_stats(StatsPeriod::AllTime)
                .expect("get_stats");
            if stats.total_files > 0 {
                assert_eq!(stats.total_downloaded_bytes, 50_000);
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("statistics row never appeared after publish — bridge wiring is broken");
    }
}
