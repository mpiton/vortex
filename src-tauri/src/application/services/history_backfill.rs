//! One-time startup sweep that records `history` rows for downloads
//! already in `Completed` state when the app launches.
//!
//! Users upgrading from broken builds (where `HistoryRepository::record`
//! had no production caller) end up with completed downloads but no
//! corresponding history rows, so the History view, the
//! `download_redownload {sourceKind:"history"}` flow (P0.9) and the
//! retention purge worker (P0.14) all behave as if the user had never
//! downloaded anything.
//!
//! This module reuses the same projection method the live recorder
//! bridge uses (`DownloadCompletedSnapshot::to_history_entry`) so the
//! backfilled rows are byte-identical to what the bridge would have
//! written at completion time. Downloads whose history row already
//! exists are skipped, so the sweep is idempotent and safe to call at
//! every startup.

use crate::application::services::queue_manager::build_completed_snapshot;
use crate::domain::error::DomainError;
use crate::domain::model::download::DownloadState;
use crate::domain::ports::driven::{DownloadRepository, HistoryRepository};

/// Sweep every persisted `Completed` download. For each one missing a
/// `history` row, build the same snapshot the live bridge would have
/// captured, project it into a `HistoryEntry` and record it.
///
/// Per-row failures (download lookup race, history insert error) are
/// logged with `tracing::warn!` but do not abort the sweep — a single
/// stuck row must not strand all the others. The function returns
/// `Err` only when the initial `find_by_state` enumeration fails, since
/// the caller has nothing useful to do with that case beyond logging.
pub fn backfill_history_for_completed_downloads(
    download_repo: &dyn DownloadRepository,
    history_repo: &dyn HistoryRepository,
) -> Result<usize, DomainError> {
    let completed = download_repo.find_by_state(DownloadState::Completed)?;
    let mut written = 0usize;
    for download in completed {
        let id = download.id();
        let already_present = match history_repo.find_by_download(id) {
            Ok(rows) => !rows.is_empty(),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    download_id = id.0,
                    "history backfill: lookup failed, skipping",
                );
                continue;
            }
        };
        if already_present {
            continue;
        }
        let snapshot = build_completed_snapshot(&download);
        let entry = snapshot.to_history_entry();
        match history_repo.record(&entry) {
            Ok(()) => written += 1,
            Err(e) => tracing::warn!(
                error = %e,
                download_id = id.0,
                "history backfill: record failed",
            ),
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::download::{Download, DownloadId, DownloadState, FileSize, Url};
    use crate::domain::model::queue::Priority;
    use crate::domain::model::views::{HistoryEntry, HistoryFilter, HistorySort};
    use std::sync::{Arc, Mutex};

    fn make_completed_download(id: u64) -> Download {
        let url = Url::new("https://example.com/file.zip").expect("valid url");
        Download::reconstruct(
            DownloadId(id),
            url,
            "file.zip".to_string(),
            Some(FileSize(1_000)),
            1_000,
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
            Vec::new(),
            0,
            0,
            5_000,
        )
    }

    #[derive(Default)]
    struct StubDownloadRepo {
        completed: Mutex<Vec<Download>>,
        fail_with: Mutex<Option<DomainError>>,
    }

    impl StubDownloadRepo {
        fn with_completed(downloads: Vec<Download>) -> Arc<Self> {
            Arc::new(Self {
                completed: Mutex::new(downloads),
                fail_with: Mutex::new(None),
            })
        }
    }

    impl DownloadRepository for StubDownloadRepo {
        fn find_by_id(&self, _: DownloadId) -> Result<Option<Download>, DomainError> {
            unreachable!("backfill must enumerate, not look up by id")
        }
        fn save(&self, _: &Download) -> Result<(), DomainError> {
            unreachable!("backfill is read-only on the download repo")
        }
        fn delete(&self, _: DownloadId) -> Result<(), DomainError> {
            unreachable!("backfill must not delete")
        }
        fn find_by_state(&self, state: DownloadState) -> Result<Vec<Download>, DomainError> {
            if let Some(e) = self.fail_with.lock().expect("fail mutex").take() {
                return Err(e);
            }
            assert_eq!(state, DownloadState::Completed);
            Ok(self.completed.lock().expect("completed mutex").clone())
        }
    }

    #[derive(Default)]
    struct RecordingHistoryRepo {
        existing: Mutex<Vec<DownloadId>>,
        records: Mutex<Vec<HistoryEntry>>,
        record_fail_for: Mutex<Option<DownloadId>>,
    }

    impl RecordingHistoryRepo {
        fn with_existing(existing: Vec<DownloadId>) -> Arc<Self> {
            Arc::new(Self {
                existing: Mutex::new(existing),
                records: Mutex::new(Vec::new()),
                record_fail_for: Mutex::new(None),
            })
        }
        fn records(&self) -> Vec<HistoryEntry> {
            self.records.lock().expect("records mutex").clone()
        }
    }

    impl HistoryRepository for RecordingHistoryRepo {
        fn record(&self, entry: &HistoryEntry) -> Result<(), DomainError> {
            if let Some(failing) = *self.record_fail_for.lock().expect("fail mutex")
                && failing == entry.download_id
            {
                return Err(DomainError::StorageError("disk full".to_string()));
            }
            self.records
                .lock()
                .expect("records mutex")
                .push(entry.clone());
            Ok(())
        }
        fn find_by_download(&self, id: DownloadId) -> Result<Vec<HistoryEntry>, DomainError> {
            let existing = self.existing.lock().expect("existing mutex");
            if existing.contains(&id) {
                Ok(vec![HistoryEntry {
                    id: 1,
                    download_id: id,
                    file_name: "preexisting".into(),
                    url: String::new(),
                    total_bytes: 0,
                    completed_at: 0,
                    duration_seconds: 1,
                    avg_speed: 0,
                    destination_path: String::new(),
                }])
            } else {
                Ok(vec![])
            }
        }
        fn find_recent(&self, _: usize) -> Result<Vec<HistoryEntry>, DomainError> {
            unreachable!()
        }
        fn list(
            &self,
            _: Option<HistoryFilter>,
            _: Option<HistorySort>,
            _: Option<usize>,
            _: Option<usize>,
        ) -> Result<Vec<HistoryEntry>, DomainError> {
            unreachable!()
        }
        fn search(&self, _: &str) -> Result<Vec<HistoryEntry>, DomainError> {
            unreachable!()
        }
        fn find_by_id(&self, _: u64) -> Result<Option<HistoryEntry>, DomainError> {
            unreachable!()
        }
        fn delete_by_id(&self, _: u64) -> Result<bool, DomainError> {
            unreachable!()
        }
        fn delete_all(&self) -> Result<u64, DomainError> {
            unreachable!()
        }
        fn delete_older_than(&self, _: u64) -> Result<u64, DomainError> {
            unreachable!()
        }
    }

    #[test]
    fn test_backfill_writes_one_row_per_missing_completed_download() {
        let downloads = vec![make_completed_download(1), make_completed_download(2)];
        let download_repo = StubDownloadRepo::with_completed(downloads);
        let history_repo = RecordingHistoryRepo::with_existing(vec![]);

        let written =
            backfill_history_for_completed_downloads(download_repo.as_ref(), history_repo.as_ref())
                .expect("sweep ok");

        assert_eq!(written, 2);
        let recorded = history_repo.records();
        assert_eq!(recorded.len(), 2);
        assert!(recorded.iter().any(|e| e.download_id == DownloadId(1)));
        assert!(recorded.iter().any(|e| e.download_id == DownloadId(2)));
    }

    #[test]
    fn test_backfill_skips_downloads_that_already_have_history_rows() {
        let downloads = vec![make_completed_download(1), make_completed_download(2)];
        let download_repo = StubDownloadRepo::with_completed(downloads);
        // Download #1 already has a history row from the live bridge.
        let history_repo = RecordingHistoryRepo::with_existing(vec![DownloadId(1)]);

        let written =
            backfill_history_for_completed_downloads(download_repo.as_ref(), history_repo.as_ref())
                .expect("sweep ok");

        assert_eq!(written, 1);
        let recorded = history_repo.records();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].download_id, DownloadId(2));
    }

    #[test]
    fn test_backfill_returns_error_when_enumeration_fails() {
        let download_repo = StubDownloadRepo::with_completed(vec![]);
        *download_repo.fail_with.lock().expect("fail mutex") =
            Some(DomainError::StorageError("connection lost".to_string()));
        let history_repo = RecordingHistoryRepo::with_existing(vec![]);

        let err =
            backfill_history_for_completed_downloads(download_repo.as_ref(), history_repo.as_ref())
                .expect_err("must surface enumeration failure");
        assert!(format!("{err}").contains("connection lost"));
    }

    #[test]
    fn test_backfill_continues_after_per_row_record_failure() {
        let downloads = vec![make_completed_download(1), make_completed_download(2)];
        let download_repo = StubDownloadRepo::with_completed(downloads);
        let history_repo = RecordingHistoryRepo::with_existing(vec![]);
        // Make the record call for download #1 fail; #2 must still land.
        *history_repo.record_fail_for.lock().expect("fail mutex") = Some(DownloadId(1));

        let written =
            backfill_history_for_completed_downloads(download_repo.as_ref(), history_repo.as_ref())
                .expect("sweep ok");

        assert_eq!(written, 1);
        let recorded = history_repo.records();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].download_id, DownloadId(2));
    }
}
