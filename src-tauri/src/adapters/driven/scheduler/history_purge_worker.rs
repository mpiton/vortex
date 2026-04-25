//! Daily history retention purge.
//!
//! Reads `history_retention_days` from `ConfigStore`, hard-deletes
//! entries older than `now - retention_days` from
//! [`HistoryRepository`], and persists the run timestamp so app
//! restarts within 24h of the last run skip the work.
//!
//! Architecture note: bypasses the `CommandBus` and calls the
//! repository port directly. This keeps the worker dependency-light
//! (no DI of the entire bus into infrastructure) while still routing
//! all *user-triggered* purges through the bus from
//! `application/commands/purge_history.rs`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::domain::error::DomainError;
use crate::domain::model::config::normalize_history_retention_days;
use crate::domain::ports::driven::{Clock, ConfigStore, HistoryRepository};

/// Seconds in a day (24 × 3600).
const SECS_PER_DAY: u64 = 86_400;

/// Filename inside the app data directory used to remember when the
/// last automatic purge ran. Stored as ASCII Unix-epoch seconds so the
/// file stays trivially inspectable and survives DB resets.
pub const HISTORY_PURGE_STATE_FILE: &str = ".history_purge_state";

/// Background worker that enforces history retention.
pub struct HistoryPurgeWorker {
    history_repo: Arc<dyn HistoryRepository>,
    config_store: Arc<dyn ConfigStore>,
    clock: Arc<dyn Clock>,
    /// Path to the persisted "last purge timestamp" sidecar.
    state_path: PathBuf,
}

impl HistoryPurgeWorker {
    pub fn new(
        history_repo: Arc<dyn HistoryRepository>,
        config_store: Arc<dyn ConfigStore>,
        clock: Arc<dyn Clock>,
        state_path: PathBuf,
    ) -> Self {
        Self {
            history_repo,
            config_store,
            clock,
            state_path,
        }
    }

    /// Force a purge regardless of the last-run sentinel.
    ///
    /// Returns the number of rows deleted. `retention_days <= 0`
    /// (unlimited) is a no-op that returns `0` without writing the
    /// sentinel — leaving the next "due" check to fire as soon as the
    /// user re-enables retention.
    pub fn run_once(&self) -> Result<u64, DomainError> {
        let cfg = self.config_store.get_config()?;
        let retention = normalize_history_retention_days(cfg.history_retention_days);
        if retention == 0 {
            return Ok(0);
        }
        let now = self.clock.now_unix_secs();
        let retention_secs = (retention as u64).saturating_mul(SECS_PER_DAY);
        let cutoff = now.saturating_sub(retention_secs);
        let purged = self.history_repo.delete_older_than(cutoff)?;
        self.write_last_purge(now)?;
        tracing::info!(
            purged,
            retention_days = retention,
            "history retention purge completed"
        );
        Ok(purged)
    }

    /// Run a purge if more than 24h have passed since the last one
    /// (or if no sentinel is on disk yet).
    ///
    /// Returns `Some(rows_deleted)` when a run fires and `None` when
    /// the last run is still recent. Read errors on the sentinel are
    /// treated as "never ran" — the caller observes a fresh purge,
    /// which is safe (the cutoff is always newer than the data).
    pub fn run_if_due(&self) -> Result<Option<u64>, DomainError> {
        let now = self.clock.now_unix_secs();
        let due = match read_last_purge(&self.state_path) {
            Some(last) => now.saturating_sub(last) >= SECS_PER_DAY,
            None => true,
        };
        if !due {
            return Ok(None);
        }
        Ok(Some(self.run_once()?))
    }

    /// Spawn the daemon: run once at startup if due, then re-run every 24h.
    ///
    /// Errors during a run are logged and swallowed so a transient I/O
    /// fault does not poison the long-lived task.
    pub fn spawn(self: Arc<Self>) {
        tokio::spawn(async move {
            if let Err(e) = self.clone().run_if_due_blocking().await {
                tracing::warn!(error = %e, "history purge: startup run failed");
            }
            let mut ticker = tokio::time::interval(Duration::from_secs(SECS_PER_DAY));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            // Skip the immediate first tick — startup `run_if_due` already
            // handled it. Without this, we'd run twice in <1ms back-to-back.
            ticker.tick().await;
            loop {
                ticker.tick().await;
                if let Err(e) = self.clone().run_once_blocking().await {
                    tracing::warn!(error = %e, "history purge: daily run failed");
                }
            }
        });
    }

    async fn run_if_due_blocking(self: Arc<Self>) -> Result<Option<u64>, DomainError> {
        tokio::task::spawn_blocking(move || self.run_if_due())
            .await
            .map_err(|e| DomainError::StorageError(format!("history purge join error: {e}")))?
    }

    async fn run_once_blocking(self: Arc<Self>) -> Result<u64, DomainError> {
        tokio::task::spawn_blocking(move || self.run_once())
            .await
            .map_err(|e| DomainError::StorageError(format!("history purge join error: {e}")))?
    }

    fn write_last_purge(&self, ts: u64) -> Result<(), DomainError> {
        if let Some(parent) = self.state_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| {
                DomainError::StorageError(format!("history purge: failed to create state dir: {e}"))
            })?;
        }
        std::fs::write(&self.state_path, ts.to_string()).map_err(|e| {
            DomainError::StorageError(format!("history purge: failed to write state: {e}"))
        })
    }
}

fn read_last_purge(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, atomic::AtomicU64, atomic::Ordering};

    use super::*;
    use crate::domain::model::config::{AppConfig, ConfigPatch, apply_patch};
    use crate::domain::model::download::DownloadId;
    use crate::domain::model::views::HistoryEntry;
    use tempfile::TempDir;

    // ── Fakes ────────────────────────────────────────────────────────

    struct FakeClock(AtomicU64);
    impl FakeClock {
        fn new(t: u64) -> Self {
            Self(AtomicU64::new(t))
        }
        fn set(&self, t: u64) {
            self.0.store(t, Ordering::SeqCst);
        }
    }
    impl Clock for FakeClock {
        fn now_unix_secs(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    struct StubConfigStore {
        cfg: Mutex<AppConfig>,
    }
    impl StubConfigStore {
        fn with_retention(days: i64) -> Self {
            Self {
                cfg: Mutex::new(AppConfig {
                    history_retention_days: days,
                    ..AppConfig::default()
                }),
            }
        }
    }
    impl ConfigStore for StubConfigStore {
        fn get_config(&self) -> Result<AppConfig, DomainError> {
            Ok(self.cfg.lock().unwrap().clone())
        }
        fn update_config(&self, patch: ConfigPatch) -> Result<AppConfig, DomainError> {
            let mut c = self.cfg.lock().unwrap();
            apply_patch(&mut c, &patch);
            Ok(c.clone())
        }
    }

    struct InMemoryHistory {
        rows: Mutex<Vec<HistoryEntry>>,
    }
    impl InMemoryHistory {
        fn new() -> Self {
            Self {
                rows: Mutex::new(Vec::new()),
            }
        }
        fn seed(&self, completed_ats: &[u64]) {
            let mut rows = self.rows.lock().unwrap();
            for (i, t) in completed_ats.iter().enumerate() {
                rows.push(HistoryEntry {
                    id: i as u64 + 1,
                    download_id: DownloadId(i as u64 + 1),
                    file_name: format!("f{i}.bin"),
                    url: format!("https://ex.com/f{i}"),
                    total_bytes: 0,
                    completed_at: *t,
                    duration_seconds: 0,
                    avg_speed: 0,
                    destination_path: format!("/tmp/f{i}"),
                });
            }
        }
        fn snapshot(&self) -> Vec<HistoryEntry> {
            self.rows.lock().unwrap().clone()
        }
    }
    impl HistoryRepository for InMemoryHistory {
        fn record(&self, entry: &HistoryEntry) -> Result<(), DomainError> {
            self.rows.lock().unwrap().push(entry.clone());
            Ok(())
        }
        fn find_recent(&self, _limit: usize) -> Result<Vec<HistoryEntry>, DomainError> {
            unimplemented!()
        }
        fn find_by_download(&self, _id: DownloadId) -> Result<Vec<HistoryEntry>, DomainError> {
            unimplemented!()
        }
        fn list(
            &self,
            _f: Option<crate::domain::model::views::HistoryFilter>,
            _s: Option<crate::domain::model::views::HistorySort>,
            _l: Option<usize>,
            _o: Option<usize>,
        ) -> Result<Vec<HistoryEntry>, DomainError> {
            unimplemented!()
        }
        fn search(&self, _q: &str) -> Result<Vec<HistoryEntry>, DomainError> {
            unimplemented!()
        }
        fn find_by_id(&self, _id: u64) -> Result<Option<HistoryEntry>, DomainError> {
            unimplemented!()
        }
        fn delete_by_id(&self, _id: u64) -> Result<bool, DomainError> {
            unimplemented!()
        }
        fn delete_all(&self) -> Result<u64, DomainError> {
            unimplemented!()
        }
        fn delete_older_than(&self, before: u64) -> Result<u64, DomainError> {
            let mut rows = self.rows.lock().unwrap();
            let original_len = rows.len();
            rows.retain(|r| r.completed_at >= before);
            Ok((original_len - rows.len()) as u64)
        }
    }

    // ── Builders ─────────────────────────────────────────────────────

    fn make_worker(
        retention_days: i64,
        now: u64,
    ) -> (Arc<HistoryPurgeWorker>, Arc<InMemoryHistory>, TempDir) {
        let history = Arc::new(InMemoryHistory::new());
        let config: Arc<dyn ConfigStore> =
            Arc::new(StubConfigStore::with_retention(retention_days));
        let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(now));
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join(HISTORY_PURGE_STATE_FILE);
        let worker = Arc::new(HistoryPurgeWorker::new(
            history.clone() as Arc<dyn HistoryRepository>,
            config,
            clock,
            state_path,
        ));
        (worker, history, dir)
    }

    // ── Tests ────────────────────────────────────────────────────────

    #[test]
    fn run_once_deletes_entries_older_than_retention_window() {
        // now = day 100, retention 30 days → cutoff = day 70.
        // entries at days 50, 60 (purged) and 80, 95 (kept).
        let now = 100 * SECS_PER_DAY;
        let (worker, history, _dir) = make_worker(30, now);
        history.seed(&[
            50 * SECS_PER_DAY,
            60 * SECS_PER_DAY,
            80 * SECS_PER_DAY,
            95 * SECS_PER_DAY,
        ]);

        let purged = worker.run_once().unwrap();

        assert_eq!(purged, 2);
        let kept: Vec<_> = history
            .snapshot()
            .into_iter()
            .map(|e| e.completed_at)
            .collect();
        assert_eq!(kept, vec![80 * SECS_PER_DAY, 95 * SECS_PER_DAY]);
    }

    #[test]
    fn run_once_with_zero_retention_is_noop() {
        let now = 1_000_000;
        let (worker, history, dir) = make_worker(0, now);
        history.seed(&[1, 2, 3]);

        let purged = worker.run_once().unwrap();

        assert_eq!(purged, 0);
        assert_eq!(history.snapshot().len(), 3);
        // no sentinel written → next run_if_due fires the moment user
        // re-enables retention.
        assert!(!dir.path().join(HISTORY_PURGE_STATE_FILE).exists());
    }

    #[test]
    fn run_once_with_negative_retention_is_clamped_to_zero_and_noop() {
        let now = 1_000_000;
        let (worker, history, dir) = make_worker(-99, now);
        history.seed(&[1, 2, 3]);

        let purged = worker.run_once().unwrap();

        assert_eq!(purged, 0);
        assert_eq!(history.snapshot().len(), 3);
        assert!(!dir.path().join(HISTORY_PURGE_STATE_FILE).exists());
    }

    #[test]
    fn run_once_writes_last_purge_sentinel() {
        let now = 12_345_678;
        let (worker, _history, dir) = make_worker(30, now);

        worker.run_once().unwrap();

        let path = dir.path().join(HISTORY_PURGE_STATE_FILE);
        let raw = std::fs::read_to_string(&path).unwrap();
        assert_eq!(raw.trim(), "12345678");
    }

    #[test]
    fn run_if_due_runs_when_no_state_file_exists() {
        let now = 100 * SECS_PER_DAY;
        let (worker, history, _dir) = make_worker(30, now);
        history.seed(&[10 * SECS_PER_DAY]);

        let result = worker.run_if_due().unwrap();

        assert_eq!(result, Some(1));
        assert!(history.snapshot().is_empty());
    }

    #[test]
    fn run_if_due_skips_when_last_purge_is_recent() {
        let now = 100 * SECS_PER_DAY;
        let (worker, history, dir) = make_worker(30, now);
        history.seed(&[10 * SECS_PER_DAY]);

        // 1h ago → still under the 24h gate.
        std::fs::write(
            dir.path().join(HISTORY_PURGE_STATE_FILE),
            (now - 3_600).to_string(),
        )
        .unwrap();

        let result = worker.run_if_due().unwrap();

        assert_eq!(result, None);
        assert_eq!(history.snapshot().len(), 1, "no rows should be deleted");
    }

    #[test]
    fn run_if_due_runs_when_last_purge_is_more_than_a_day_old() {
        let now = 100 * SECS_PER_DAY;
        let (worker, history, dir) = make_worker(30, now);
        history.seed(&[10 * SECS_PER_DAY]);

        // 25h ago → over the 24h gate.
        std::fs::write(
            dir.path().join(HISTORY_PURGE_STATE_FILE),
            (now - 25 * 3_600).to_string(),
        )
        .unwrap();

        let result = worker.run_if_due().unwrap();

        assert_eq!(result, Some(1));
    }

    #[test]
    fn run_if_due_treats_corrupt_state_file_as_never_ran() {
        let now = 100 * SECS_PER_DAY;
        let (worker, history, dir) = make_worker(30, now);
        history.seed(&[10 * SECS_PER_DAY]);

        std::fs::write(dir.path().join(HISTORY_PURGE_STATE_FILE), "not-a-number").unwrap();

        let result = worker.run_if_due().unwrap();

        // Corrupt sentinel = "no idea when we last ran" → run.
        assert_eq!(result, Some(1));
    }

    #[test]
    fn mock_clock_advances_drive_two_consecutive_purges() {
        // Mirrors the AC `purge runtime quotidienne (test : mock clock)`:
        // advance the clock past one day → second run_if_due fires.
        let now = 100 * SECS_PER_DAY;
        let history = Arc::new(InMemoryHistory::new());
        let clock = Arc::new(FakeClock::new(now));
        let cfg: Arc<dyn ConfigStore> = Arc::new(StubConfigStore::with_retention(30));
        let dir = TempDir::new().unwrap();
        let worker = Arc::new(HistoryPurgeWorker::new(
            history.clone() as Arc<dyn HistoryRepository>,
            cfg,
            clock.clone() as Arc<dyn Clock>,
            dir.path().join(HISTORY_PURGE_STATE_FILE),
        ));
        history.seed(&[10 * SECS_PER_DAY, 50 * SECS_PER_DAY]);

        // First run at day 100: cutoff = day 70 → both entries purged.
        assert_eq!(worker.run_if_due().unwrap(), Some(2));

        // Advance one hour → still under the 24h gate.
        clock.set(now + 3_600);
        history.seed(&[(now / SECS_PER_DAY + 1) * SECS_PER_DAY]);
        assert_eq!(worker.run_if_due().unwrap(), None);

        // Advance one more day → gate re-opens, nothing left to purge yet.
        clock.set(now + SECS_PER_DAY + 3_600);
        assert_eq!(worker.run_if_due().unwrap(), Some(0));
    }

    #[test]
    fn make_worker_helper_builds_a_runnable_instance() {
        // Smoke-tests that the helper used by sibling tests wires every
        // dependency correctly — `run_once` must succeed end-to-end.
        let (worker, history, _dir) = make_worker(30, 100 * SECS_PER_DAY);
        history.seed(&[10 * SECS_PER_DAY]);
        let purged = worker.run_once().unwrap();
        assert_eq!(purged, 1);
    }
}
