//! Subscribes to `DownloadCompletedPersisted` and feeds the
//! aggregated `statistics` read model so the Statistics view's
//! daily volume / total files / avg speed KPIs stay in sync with
//! the `downloads` table.
//!
//! Without this bridge the `statistics` table is never written and
//! the view reports zeros even though completed downloads exist
//! (issue #114).

use std::sync::Arc;

use crate::domain::event::DomainEvent;
use crate::domain::model::download::Download;
use crate::domain::ports::driven::{DownloadRepository, EventBus, StatsRepository};

/// Wire a stats recorder onto the event bus.
///
/// Listens for `DownloadCompletedPersisted` (post-persist event from
/// `QueueManager`), loads the matching `Download` aggregate, derives
/// `(bytes, avg_speed)` from it and calls `StatsRepository::record_completed`.
///
/// All errors are swallowed with `tracing::warn!` — a stats failure must
/// never propagate back into the queue/UI flow.
pub fn spawn_stats_recorder_bridge(
    event_bus: &dyn EventBus,
    download_repo: Arc<dyn DownloadRepository>,
    stats_repo: Arc<dyn StatsRepository>,
) {
    event_bus.subscribe(Box::new(move |event: &DomainEvent| {
        record_for_event(download_repo.as_ref(), stats_repo.as_ref(), event);
    }));
}

fn record_for_event(
    download_repo: &dyn DownloadRepository,
    stats_repo: &dyn StatsRepository,
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
                "stats bridge: download not found for completed event, skipping",
            );
            return;
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                download_id = id.0,
                "stats bridge: failed to load download for completed event",
            );
            return;
        }
    };
    let (bytes, avg_speed) = derive_stats(&download);
    if let Err(e) = stats_repo.record_completed(bytes, avg_speed) {
        tracing::warn!(
            error = %e,
            download_id = id.0,
            "stats bridge: record_completed failed",
        );
    }
}

/// Pull the byte count and average speed off a completed `Download`.
///
/// `bytes` prefers the authoritative `file_size` (set when the upstream
/// announces a Content-Length) and falls back to the running
/// `downloaded_bytes` for streams of unknown size.
///
/// `avg_speed` is `bytes / elapsed_seconds`. `created_at`/`updated_at` are
/// stored in milliseconds (see `current_timestamp_ms`), so the difference is
/// divided by `1_000` and clamped to a `1`-second floor — short transfers
/// (`< 1s` elapsed) and instant completions (`updated_at == created_at`)
/// therefore never divide by zero or by a misinterpreted unit.
///
/// Caveat: the elapsed window spans from queue admission (`created_at`) to
/// completion (`updated_at`), so it includes time spent queued, paused or
/// retrying. The `Download` aggregate doesn't currently expose a transfer-
/// start timestamp; once it does, this should switch to `started_at` so the
/// `avg_speed` aggregate reflects only the active transfer phase.
fn derive_stats(download: &Download) -> (u64, u64) {
    let bytes = download
        .file_size()
        .map(|fs| fs.0)
        .filter(|n| *n > 0)
        .unwrap_or_else(|| download.downloaded_bytes());
    let elapsed_ms = download.updated_at().saturating_sub(download.created_at());
    let elapsed_secs = (elapsed_ms / 1_000).max(1);
    let avg_speed = bytes / elapsed_secs;
    (bytes, avg_speed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::DomainError;
    use crate::domain::model::download::{Download, DownloadId, DownloadState, Url};
    use crate::domain::model::queue::Priority;
    use std::sync::Mutex;

    // ── Fixtures ────────────────────────────────────────────────

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
            size_bytes.map(crate::domain::model::download::FileSize),
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
            "/tmp".to_string(),
            created_at,
            updated_at,
        );
        // Anchor downloaded_bytes for streaming-size cases (file_size None).
        if size_bytes.is_none() {
            d.update_progress(0);
        }
        d
    }

    // ── Mock repositories ───────────────────────────────────────

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
            // Cloning here lets a test reuse the stub for several events;
            // domain `Download` is `Clone` so this is cheap.
            match &*self.result.lock().expect("stub mutex") {
                Ok(Some(d)) => Ok(Some(d.clone())),
                Ok(None) => Ok(None),
                Err(e) => Err(e.clone()),
            }
        }

        fn save(&self, _: &Download) -> Result<(), DomainError> {
            unreachable!("bridge must never write through the repo")
        }

        fn delete(&self, _: DownloadId) -> Result<(), DomainError> {
            unreachable!("bridge must never delete through the repo")
        }

        fn find_by_state(&self, _: DownloadState) -> Result<Vec<Download>, DomainError> {
            unreachable!("bridge must never enumerate by state")
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

    // ── Tests ───────────────────────────────────────────────────

    #[test]
    fn test_record_for_event_records_when_persisted_event_uses_file_size() {
        // Timestamps are in ms (see `current_timestamp_ms`):
        // elapsed_ms = 110_000 - 100_000 = 10_000 → 10s.
        let download = make_download(7, 100_000, 110_000, Some(2_000));
        let download_repo = StubDownloadRepo::returning(Ok(Some(download)));
        let stats_repo = Arc::new(RecordingStatsRepo::default());

        record_for_event(
            download_repo.as_ref(),
            stats_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted { id: DownloadId(7) },
        );

        // bytes = file_size, avg_speed = 2000 / 10s = 200
        assert_eq!(stats_repo.calls(), vec![(2_000, 200)]);
    }

    #[test]
    fn test_record_for_event_falls_back_to_downloaded_bytes_when_size_unknown() {
        let mut download = make_download(8, 100_000, 110_000, None);
        download.update_progress(1_500);
        let download_repo = StubDownloadRepo::returning(Ok(Some(download)));
        let stats_repo = Arc::new(RecordingStatsRepo::default());

        record_for_event(
            download_repo.as_ref(),
            stats_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted { id: DownloadId(8) },
        );

        // bytes = downloaded_bytes (file_size was None), avg_speed = 1500 / 10s = 150
        assert_eq!(stats_repo.calls(), vec![(1_500, 150)]);
    }

    #[test]
    fn test_record_for_event_avoids_division_by_zero_on_instant_completion() {
        let download = make_download(9, 200_000, 200_000, Some(4_096));
        let download_repo = StubDownloadRepo::returning(Ok(Some(download)));
        let stats_repo = Arc::new(RecordingStatsRepo::default());

        record_for_event(
            download_repo.as_ref(),
            stats_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted { id: DownloadId(9) },
        );

        // elapsed_ms = 0 → elapsed_secs clamped to 1 → avg_speed = bytes / 1
        assert_eq!(stats_repo.calls(), vec![(4_096, 4_096)]);
    }

    #[test]
    fn test_record_for_event_clamps_sub_second_elapsed_to_one_second() {
        // 500 ms elapsed: elapsed_ms / 1_000 = 0, clamped to 1 second.
        // Without the ms→s conversion, this would have produced a 1000×
        // inflated avg_speed (cubic P2, PR #117).
        let download = make_download(11, 0, 500, Some(2_048));
        let download_repo = StubDownloadRepo::returning(Ok(Some(download)));
        let stats_repo = Arc::new(RecordingStatsRepo::default());

        record_for_event(
            download_repo.as_ref(),
            stats_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted { id: DownloadId(11) },
        );

        assert_eq!(stats_repo.calls(), vec![(2_048, 2_048)]);
    }

    #[test]
    fn test_record_for_event_ignores_unrelated_events() {
        let download_repo = StubDownloadRepo::returning(Ok(Some(make_download(1, 0, 1, Some(10)))));
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
            record_for_event(download_repo.as_ref(), stats_repo.as_ref(), &event);
        }

        assert!(stats_repo.calls().is_empty());
    }

    #[test]
    fn test_record_for_event_skips_when_repo_returns_none() {
        let download_repo = StubDownloadRepo::returning(Ok(None));
        let stats_repo = Arc::new(RecordingStatsRepo::default());

        record_for_event(
            download_repo.as_ref(),
            stats_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted {
                id: DownloadId(404),
            },
        );

        assert!(stats_repo.calls().is_empty());
    }

    #[test]
    fn test_record_for_event_swallows_repo_error() {
        let download_repo =
            StubDownloadRepo::returning(Err(DomainError::NotFound("boom".to_string())));
        let stats_repo = Arc::new(RecordingStatsRepo::default());

        record_for_event(
            download_repo.as_ref(),
            stats_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted { id: DownloadId(1) },
        );

        assert!(stats_repo.calls().is_empty());
    }

    #[test]
    fn test_record_for_event_swallows_stats_repo_error() {
        let download_repo =
            StubDownloadRepo::returning(Ok(Some(make_download(2, 0, 1, Some(100)))));
        let stats_repo = Arc::new(RecordingStatsRepo::default());
        stats_repo.fail_next(DomainError::StorageError("disk full".to_string()));

        record_for_event(
            download_repo.as_ref(),
            stats_repo.as_ref(),
            &DomainEvent::DownloadCompletedPersisted { id: DownloadId(2) },
        );

        // First call errored (and was thrown away); the recorder didn't push it.
        assert!(stats_repo.calls().is_empty());
    }

    #[tokio::test]
    async fn test_spawn_subscribes_to_event_bus() {
        use crate::adapters::driven::event::tokio_event_bus::TokioEventBus;
        use std::time::Duration;

        let bus = TokioEventBus::new(16);
        // 10s elapsed (1_010_000 - 1_000_000 = 10_000 ms).
        let download_repo = StubDownloadRepo::returning(Ok(Some(make_download(
            42,
            1_000_000,
            1_010_000,
            Some(20_000),
        ))));
        let stats_repo = Arc::new(RecordingStatsRepo::default());

        spawn_stats_recorder_bridge(&bus, download_repo, stats_repo.clone());

        bus.publish(DomainEvent::DownloadCompletedPersisted { id: DownloadId(42) });

        // Allow the spawned subscriber task to drain the channel.
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
        use crate::adapters::driven::sqlite::download_repo::SqliteDownloadRepo;
        use crate::adapters::driven::sqlite::stats_repo::SqliteStatsRepo;
        use crate::domain::model::views::StatsPeriod;
        use std::time::Duration;

        let db = setup_test_db().await.expect("test db");
        let download_repo: Arc<dyn DownloadRepository> =
            Arc::new(SqliteDownloadRepo::new(db.clone()));
        let stats_repo: Arc<dyn StatsRepository> = Arc::new(SqliteStatsRepo::new(db.clone()));

        // Seed a sized download so the bridge has something to record.
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
        spawn_stats_recorder_bridge(&bus, download_repo, stats_repo.clone());

        bus.publish(DomainEvent::DownloadCompletedPersisted {
            id: DownloadId(101),
        });

        // Poll until the projection lands in SQLite or give up after 1s.
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
