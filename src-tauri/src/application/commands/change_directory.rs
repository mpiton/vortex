//! Handler for `ChangeDirectoryCommand` and `ChangeDirectoryBulkCommand` (task 13).
//!
//! Moves a download's on-disk file (and its `.vortex-meta` sidecar when
//! present) into a new destination directory. The basename is preserved so
//! resume metadata, history references, and external bookmarks keep working.
//!
//! State handling:
//! - `Downloading` is paused via the engine before the move, then resumed
//!   when persistence succeeds — segments stay on the filesystem so resume
//!   picks up exactly where it left off.
//! - `Extracting` and `Checking` are rejected because another worker is
//!   actively reading the file; moving it would corrupt the in-flight read.
//! - All other states (`Queued`, `Paused`, `Completed`, `Error`, `Retry`,
//!   `Waiting`) move freely. When the on-disk file does not yet exist the
//!   move is skipped and only the DB path is updated.

use std::path::{Path, PathBuf};

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::download::{DownloadId, DownloadState};

/// Reason why a single bulk entry failed to move. Mirrors `AppError` variants
/// the caller cares about so the frontend can show per-row diagnostics
/// without parsing free-form strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeDirectoryFailure {
    pub id: DownloadId,
    pub message: String,
}

/// Outcome of a bulk move: list of ids that completed successfully and a
/// per-id description of the failures so the frontend can keep the failed
/// rows selected for retry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChangeDirectoryBulkOutcome {
    pub moved: Vec<DownloadId>,
    pub failed: Vec<ChangeDirectoryFailure>,
}

/// States where another worker is touching the file — we MUST refuse the
/// move because relocating bytes mid-read would corrupt the destination.
fn rejects_move(state: DownloadState) -> bool {
    matches!(state, DownloadState::Extracting | DownloadState::Checking)
}

fn join_destination(dir: &Path, file_name: &str) -> PathBuf {
    dir.join(file_name)
}

impl CommandBus {
    pub async fn handle_change_directory(
        &self,
        cmd: super::ChangeDirectoryCommand,
    ) -> Result<(), AppError> {
        self.move_download_to(cmd.id, &cmd.new_destination_dir)
            .await
    }

    pub async fn handle_change_directory_bulk(
        &self,
        cmd: super::ChangeDirectoryBulkCommand,
    ) -> Result<ChangeDirectoryBulkOutcome, AppError> {
        if cmd.ids.is_empty() {
            return Ok(ChangeDirectoryBulkOutcome::default());
        }
        let mut outcome = ChangeDirectoryBulkOutcome::default();
        for id in cmd.ids {
            match self.move_download_to(id, &cmd.new_destination_dir).await {
                Ok(()) => outcome.moved.push(id),
                Err(e) => outcome.failed.push(ChangeDirectoryFailure {
                    id,
                    message: e.to_string(),
                }),
            }
        }
        Ok(outcome)
    }

    /// Internal worker shared by the single-item and bulk handlers. Each
    /// call is its own atomic unit: a single failure aborts that item but
    /// never leaves a half-moved file behind.
    async fn move_download_to(&self, id: DownloadId, new_dir: &Path) -> Result<(), AppError> {
        let download = self
            .download_repo()
            .find_by_id(id)?
            .ok_or_else(|| AppError::NotFound(format!("Download {} not found", id.0)))?;

        if rejects_move(download.state()) {
            return Err(AppError::Validation(format!(
                "Download {} is in state {:?} and cannot be moved",
                id.0,
                download.state()
            )));
        }

        let new_full_path = join_destination(new_dir, download.file_name());
        let old_full_path = PathBuf::from(download.destination_path());
        if new_full_path == old_full_path {
            // Caller asked to move the file to where it already is. Skip
            // every side-effect so we don't churn the engine or emit a
            // misleading event for a no-op.
            return Ok(());
        }

        let was_downloading = download.state() == DownloadState::Downloading;
        if was_downloading {
            // Pause the engine BEFORE touching the file so no segment
            // worker has it open. We do not transition the domain state to
            // Paused — the user's intent is "keep downloading after the
            // move", which we honour by resuming below.
            self.download_engine().pause(id)?;
        }

        if let Err(e) = self.relocate_files(&old_full_path, &new_full_path) {
            // Move failed → restore the engine state we touched. We do not
            // touch the domain state because we never mutated it.
            if was_downloading && let Err(rb) = self.download_engine().resume(id) {
                tracing::error!(
                    "rollback resume failed for download {:?} after move error: {rb}",
                    id
                );
            }
            return Err(e);
        }

        let updated = download
            .clone()
            .with_destination_path(new_full_path.to_string_lossy().into_owned());
        if let Err(e) = self.download_repo().save(&updated) {
            // Persistence failed AFTER the file was already moved on disk.
            // Try to move the file back so the next read still finds it
            // where the DB still says it lives. If the rollback move fails
            // we surface the original DB error and log the divergence.
            if let Err(rb) = self
                .file_storage()
                .move_file(&new_full_path, &old_full_path)
            {
                tracing::error!(
                    "rollback move failed for download {:?}: file at {} but DB still points to {} ({rb})",
                    id,
                    new_full_path.display(),
                    old_full_path.display()
                );
            } else {
                let _ = self
                    .file_storage()
                    .move_meta(&new_full_path, &old_full_path);
            }
            if was_downloading && let Err(rb) = self.download_engine().resume(id) {
                tracing::error!(
                    "rollback resume failed for download {:?} after save error: {rb}",
                    id
                );
            }
            return Err(e.into());
        }

        if was_downloading {
            // Resume failure is non-fatal for the move itself: the file is
            // already at the new location and persisted. We surface the
            // error so the user knows their download is paused, but we do
            // not undo the move.
            self.download_engine().resume(id)?;
        }

        self.event_bus()
            .publish(DomainEvent::DownloadDirectoryChanged {
                id,
                new_destination_path: updated.destination_path().to_string(),
            });
        Ok(())
    }

    fn relocate_files(&self, from: &Path, to: &Path) -> Result<(), AppError> {
        // Skip the body move when the source doesn't exist on disk yet —
        // happens for `Queued`/`Waiting` items whose engine has never run.
        // The DB path still gets updated so the next start lands in the
        // right folder.
        if self.file_storage().file_exists(from) {
            self.file_storage().move_file(from, to)?;
        }
        // `move_meta` is already a no-op when the sidecar is missing, so we
        // don't need an `exists()` guard here.
        if let Err(e) = self.file_storage().move_meta(from, to) {
            // The body move already succeeded. Try to move the body back so
            // we don't leave the user with a split state. If THAT fails we
            // log and bail — the DB has not been updated yet, so the
            // original record still points to `from`.
            if from != to
                && self.file_storage().file_exists(to)
                && let Err(rb) = self.file_storage().move_file(to, from)
            {
                tracing::error!(
                    "failed to roll back body move from {} → {}: {rb}",
                    to.display(),
                    from.display()
                );
            }
            return Err(e.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::application::commands::{ChangeDirectoryBulkCommand, ChangeDirectoryCommand};
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::archive::{ArchiveEntry, ArchiveFormat, ExtractSummary};
    use crate::domain::model::config::{AppConfig, ConfigPatch};
    use crate::domain::model::credential::Credential;
    use crate::domain::model::download::{Download, DownloadId, DownloadState, Url};
    use crate::domain::model::http::HttpResponse;
    use crate::domain::model::meta::DownloadMeta;
    use crate::domain::model::plugin::{PluginInfo, PluginManifest};
    use crate::domain::ports::driven::{
        ArchiveExtractor, ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine,
        DownloadRepository, EventBus, FileStorage, HttpClient, PluginLoader,
    };

    struct MockRepo {
        store: Mutex<HashMap<u64, Download>>,
        save_failure: Mutex<Option<DomainError>>,
    }

    impl MockRepo {
        fn new() -> Self {
            Self {
                store: Mutex::new(HashMap::new()),
                save_failure: Mutex::new(None),
            }
        }
        fn with(self, d: Download) -> Self {
            self.store.lock().unwrap().insert(d.id().0, d);
            self
        }
        fn fail_next_save_with(self, err: DomainError) -> Self {
            *self.save_failure.lock().unwrap() = Some(err);
            self
        }
    }

    impl DownloadRepository for MockRepo {
        fn find_by_id(&self, id: DownloadId) -> Result<Option<Download>, DomainError> {
            Ok(self.store.lock().unwrap().get(&id.0).cloned())
        }
        fn save(&self, d: &Download) -> Result<(), DomainError> {
            if let Some(err) = self.save_failure.lock().unwrap().take() {
                return Err(err);
            }
            self.store.lock().unwrap().insert(d.id().0, d.clone());
            Ok(())
        }
        fn delete(&self, id: DownloadId) -> Result<(), DomainError> {
            self.store.lock().unwrap().remove(&id.0);
            Ok(())
        }
        fn find_by_state(&self, s: DownloadState) -> Result<Vec<Download>, DomainError> {
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

    /// Minimal in-memory storage that lets us assert which paths the handler
    /// actually moved without leaning on a real filesystem.
    #[derive(Default)]
    struct RecordingStorage {
        moves: Mutex<Vec<(String, String)>>,
        meta_moves: Mutex<Vec<(String, String)>>,
        existing_files: Mutex<Vec<String>>,
        fail_move: Mutex<bool>,
    }
    impl RecordingStorage {
        fn with_existing(self, p: &str) -> Self {
            self.existing_files.lock().unwrap().push(p.to_string());
            self
        }
        fn fail_next_move(self) -> Self {
            *self.fail_move.lock().unwrap() = true;
            self
        }
    }
    impl FileStorage for RecordingStorage {
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
        fn file_exists(&self, p: &Path) -> bool {
            self.existing_files
                .lock()
                .unwrap()
                .iter()
                .any(|e| e == &p.to_string_lossy())
        }
        fn move_file(&self, from: &Path, to: &Path) -> Result<(), DomainError> {
            if std::mem::take(&mut *self.fail_move.lock().unwrap()) {
                return Err(DomainError::StorageError("fault injection".into()));
            }
            self.moves.lock().unwrap().push((
                from.to_string_lossy().into_owned(),
                to.to_string_lossy().into_owned(),
            ));
            // Update the existence map so the second moveable file (meta)
            // doesn't double-count the body path.
            let mut existing = self.existing_files.lock().unwrap();
            existing.retain(|p| p != &from.to_string_lossy());
            existing.push(to.to_string_lossy().into_owned());
            Ok(())
        }
        fn move_meta(&self, from: &Path, to: &Path) -> Result<(), DomainError> {
            self.meta_moves.lock().unwrap().push((
                from.to_string_lossy().into_owned(),
                to.to_string_lossy().into_owned(),
            ));
            Ok(())
        }
    }

    struct PathExistenceFs {
        inner: Arc<RecordingStorage>,
    }
    impl FileStorage for PathExistenceFs {
        fn create_file(&self, p: &Path, s: u64) -> Result<(), DomainError> {
            self.inner.create_file(p, s)
        }
        fn write_segment(&self, p: &Path, o: u64, d: &[u8]) -> Result<(), DomainError> {
            self.inner.write_segment(p, o, d)
        }
        fn read_meta(&self, p: &Path) -> Result<Option<DownloadMeta>, DomainError> {
            self.inner.read_meta(p)
        }
        fn write_meta(&self, p: &Path, m: &DownloadMeta) -> Result<(), DomainError> {
            self.inner.write_meta(p, m)
        }
        fn delete_meta(&self, p: &Path) -> Result<(), DomainError> {
            self.inner.delete_meta(p)
        }
        fn file_exists(&self, p: &Path) -> bool {
            self.inner.file_exists(p)
        }
        fn move_file(&self, from: &Path, to: &Path) -> Result<(), DomainError> {
            self.inner.move_file(from, to)
        }
        fn move_meta(&self, from: &Path, to: &Path) -> Result<(), DomainError> {
            self.inner.move_meta(from, to)
        }
    }

    #[derive(Default)]
    struct RecordingEngine {
        pauses: Mutex<Vec<DownloadId>>,
        resumes: Mutex<Vec<DownloadId>>,
    }
    impl DownloadEngine for RecordingEngine {
        fn start(&self, _d: &Download) -> Result<(), DomainError> {
            Ok(())
        }
        fn pause(&self, id: DownloadId) -> Result<(), DomainError> {
            self.pauses.lock().unwrap().push(id);
            Ok(())
        }
        fn resume(&self, id: DownloadId) -> Result<(), DomainError> {
            self.resumes.lock().unwrap().push(id);
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

    fn make_download_at(id: u64, dest: &str, file_name: &str) -> Download {
        Download::new(
            DownloadId(id),
            Url::new(&format!("https://example.com/{file_name}")).unwrap(),
            file_name.to_string(),
            dest.to_string(),
        )
    }

    fn build_bus(
        repo: MockRepo,
        engine: Arc<RecordingEngine>,
        events: Arc<RecordingBus>,
        storage: Arc<RecordingStorage>,
    ) -> CommandBus {
        let fs: Arc<dyn FileStorage> = Arc::new(PathExistenceFs { inner: storage });
        CommandBus::new(
            Arc::new(repo),
            engine,
            events,
            fs,
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
    async fn test_change_directory_completed_moves_file_and_persists_path() {
        let dl = make_download_at(1, "/old/folder/file.bin", "file.bin");
        let mut completed = dl;
        completed.start().unwrap();
        // Force-complete via reconstruction since the state machine path is
        // not exposed publicly outside the engine — the destination value is
        // what we care about for this test.
        let completed = Download::reconstruct(
            completed.id(),
            Url::new("https://example.com/file.bin").unwrap(),
            "file.bin".to_string(),
            None,
            0,
            DownloadState::Completed,
            crate::domain::model::queue::Priority::default(),
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
            "/old/folder/file.bin".to_string(),
            0,
            0,
        );
        let storage = Arc::new(RecordingStorage::default().with_existing("/old/folder/file.bin"));
        let engine = Arc::new(RecordingEngine::default());
        let events = Arc::new(RecordingBus::new());
        let bus = build_bus(
            MockRepo::new().with(completed),
            engine.clone(),
            events.clone(),
            storage.clone(),
        );

        bus.handle_change_directory(ChangeDirectoryCommand {
            id: DownloadId(1),
            new_destination_dir: PathBuf::from("/new/folder"),
        })
        .await
        .expect("move should succeed");

        assert_eq!(
            bus.download_repo()
                .find_by_id(DownloadId(1))
                .unwrap()
                .unwrap()
                .destination_path(),
            "/new/folder/file.bin"
        );
        let body_moves = storage.moves.lock().unwrap().clone();
        assert_eq!(
            body_moves,
            vec![(
                "/old/folder/file.bin".to_string(),
                "/new/folder/file.bin".to_string()
            )]
        );
        let meta_moves = storage.meta_moves.lock().unwrap().clone();
        assert_eq!(
            meta_moves,
            vec![(
                "/old/folder/file.bin".to_string(),
                "/new/folder/file.bin".to_string()
            )]
        );
        assert!(
            engine.pauses.lock().unwrap().is_empty(),
            "completed download must not be paused"
        );
        assert!(
            engine.resumes.lock().unwrap().is_empty(),
            "completed download must not be resumed"
        );
        assert!(matches!(
            events.events.lock().unwrap().as_slice(),
            [DomainEvent::DownloadDirectoryChanged { id, new_destination_path }]
                if id.0 == 1 && new_destination_path == "/new/folder/file.bin"
        ));
    }

    #[tokio::test]
    async fn test_change_directory_downloading_pauses_engine_then_resumes() {
        let mut dl = make_download_at(7, "/old/active.bin", "active.bin");
        dl.start().unwrap(); // Queued -> Downloading
        let storage = Arc::new(RecordingStorage::default().with_existing("/old/active.bin"));
        let engine = Arc::new(RecordingEngine::default());
        let events = Arc::new(RecordingBus::new());
        let bus = build_bus(
            MockRepo::new().with(dl),
            engine.clone(),
            events.clone(),
            storage.clone(),
        );

        bus.handle_change_directory(ChangeDirectoryCommand {
            id: DownloadId(7),
            new_destination_dir: PathBuf::from("/new"),
        })
        .await
        .expect("move should succeed");

        assert_eq!(
            engine.pauses.lock().unwrap().clone(),
            vec![DownloadId(7)],
            "engine must pause once before the move"
        );
        assert_eq!(
            engine.resumes.lock().unwrap().clone(),
            vec![DownloadId(7)],
            "engine must resume once after persistence"
        );
        let saved = bus
            .download_repo()
            .find_by_id(DownloadId(7))
            .unwrap()
            .unwrap();
        assert_eq!(saved.destination_path(), "/new/active.bin");
        assert_eq!(saved.state(), DownloadState::Downloading, "state preserved");
    }

    #[tokio::test]
    async fn test_change_directory_rejects_extracting_state() {
        let extracting = Download::reconstruct(
            DownloadId(9),
            Url::new("https://example.com/x.zip").unwrap(),
            "x.zip".to_string(),
            None,
            0,
            DownloadState::Extracting,
            crate::domain::model::queue::Priority::default(),
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
            "/dl/x.zip".to_string(),
            0,
            0,
        );
        let storage = Arc::new(RecordingStorage::default().with_existing("/dl/x.zip"));
        let engine = Arc::new(RecordingEngine::default());
        let events = Arc::new(RecordingBus::new());
        let bus = build_bus(
            MockRepo::new().with(extracting),
            engine.clone(),
            events.clone(),
            storage.clone(),
        );

        let err = bus
            .handle_change_directory(ChangeDirectoryCommand {
                id: DownloadId(9),
                new_destination_dir: PathBuf::from("/elsewhere"),
            })
            .await
            .expect_err("must reject Extracting");
        assert!(matches!(err, AppError::Validation(_)));
        assert!(
            storage.moves.lock().unwrap().is_empty(),
            "no file move must occur"
        );
        assert!(
            events.events.lock().unwrap().is_empty(),
            "no event on rejection"
        );
    }

    #[tokio::test]
    async fn test_change_directory_rejects_checking_state() {
        let checking = Download::reconstruct(
            DownloadId(10),
            Url::new("https://example.com/check.bin").unwrap(),
            "check.bin".to_string(),
            None,
            0,
            DownloadState::Checking,
            crate::domain::model::queue::Priority::default(),
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
            "/dl/check.bin".to_string(),
            0,
            0,
        );
        let storage = Arc::new(RecordingStorage::default().with_existing("/dl/check.bin"));
        let engine = Arc::new(RecordingEngine::default());
        let events = Arc::new(RecordingBus::new());
        let bus = build_bus(
            MockRepo::new().with(checking),
            engine.clone(),
            events.clone(),
            storage.clone(),
        );

        let err = bus
            .handle_change_directory(ChangeDirectoryCommand {
                id: DownloadId(10),
                new_destination_dir: PathBuf::from("/elsewhere"),
            })
            .await
            .expect_err("must reject Checking");
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn test_change_directory_skips_body_when_source_missing() {
        // Queued download with no on-disk file yet — DB must still update so
        // the next start lands in the new folder.
        let queued = make_download_at(2, "/old/queued.bin", "queued.bin");
        let storage = Arc::new(RecordingStorage::default()); // no existing files
        let engine = Arc::new(RecordingEngine::default());
        let events = Arc::new(RecordingBus::new());
        let bus = build_bus(
            MockRepo::new().with(queued),
            engine.clone(),
            events.clone(),
            storage.clone(),
        );

        bus.handle_change_directory(ChangeDirectoryCommand {
            id: DownloadId(2),
            new_destination_dir: PathBuf::from("/new"),
        })
        .await
        .expect("must succeed even when body absent");

        assert!(
            storage.moves.lock().unwrap().is_empty(),
            "body move must be skipped"
        );
        assert_eq!(
            storage.meta_moves.lock().unwrap().len(),
            1,
            "meta move still attempted (no-op when sidecar missing)"
        );
        let saved = bus
            .download_repo()
            .find_by_id(DownloadId(2))
            .unwrap()
            .unwrap();
        assert_eq!(saved.destination_path(), "/new/queued.bin");
    }

    #[tokio::test]
    async fn test_change_directory_noop_when_destination_unchanged() {
        let dl = make_download_at(3, "/same/file.bin", "file.bin");
        let storage = Arc::new(RecordingStorage::default().with_existing("/same/file.bin"));
        let engine = Arc::new(RecordingEngine::default());
        let events = Arc::new(RecordingBus::new());
        let bus = build_bus(
            MockRepo::new().with(dl),
            engine.clone(),
            events.clone(),
            storage.clone(),
        );

        bus.handle_change_directory(ChangeDirectoryCommand {
            id: DownloadId(3),
            new_destination_dir: PathBuf::from("/same"),
        })
        .await
        .expect("noop must succeed");

        assert!(
            storage.moves.lock().unwrap().is_empty(),
            "no-op must not call move_file"
        );
        assert!(
            events.events.lock().unwrap().is_empty(),
            "no-op must not emit event"
        );
    }

    #[tokio::test]
    async fn test_change_directory_missing_id_returns_not_found() {
        let storage = Arc::new(RecordingStorage::default());
        let engine = Arc::new(RecordingEngine::default());
        let events = Arc::new(RecordingBus::new());
        let bus = build_bus(MockRepo::new(), engine, events, storage);

        let err = bus
            .handle_change_directory(ChangeDirectoryCommand {
                id: DownloadId(404),
                new_destination_dir: PathBuf::from("/anywhere"),
            })
            .await
            .expect_err("missing download must error");
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_change_directory_filesystem_failure_keeps_db_intact() {
        let dl = make_download_at(5, "/old/x.bin", "x.bin");
        let storage = Arc::new(
            RecordingStorage::default()
                .with_existing("/old/x.bin")
                .fail_next_move(),
        );
        let engine = Arc::new(RecordingEngine::default());
        let events = Arc::new(RecordingBus::new());
        let bus = build_bus(
            MockRepo::new().with(dl),
            engine.clone(),
            events.clone(),
            storage.clone(),
        );

        let err = bus
            .handle_change_directory(ChangeDirectoryCommand {
                id: DownloadId(5),
                new_destination_dir: PathBuf::from("/new"),
            })
            .await
            .expect_err("filesystem error must propagate");
        assert!(matches!(
            err,
            AppError::Domain(DomainError::StorageError(_))
        ));
        let saved = bus
            .download_repo()
            .find_by_id(DownloadId(5))
            .unwrap()
            .unwrap();
        assert_eq!(
            saved.destination_path(),
            "/old/x.bin",
            "DB must not advance on FS failure"
        );
        assert!(
            events.events.lock().unwrap().is_empty(),
            "no event on failure"
        );
    }

    #[tokio::test]
    async fn test_change_directory_bulk_partitions_success_and_failure() {
        let storage = Arc::new(
            RecordingStorage::default()
                .with_existing("/old/a.bin")
                .with_existing("/old/c.bin"),
        );
        let engine = Arc::new(RecordingEngine::default());
        let events = Arc::new(RecordingBus::new());
        let extracting_b = Download::reconstruct(
            DownloadId(2),
            Url::new("https://example.com/b.bin").unwrap(),
            "b.bin".to_string(),
            None,
            0,
            DownloadState::Extracting,
            crate::domain::model::queue::Priority::default(),
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
            "/old/b.bin".to_string(),
            0,
            0,
        );
        let bus = build_bus(
            MockRepo::new()
                .with(make_download_at(1, "/old/a.bin", "a.bin"))
                .with(extracting_b)
                .with(make_download_at(3, "/old/c.bin", "c.bin")),
            engine,
            events.clone(),
            storage.clone(),
        );

        let outcome = bus
            .handle_change_directory_bulk(ChangeDirectoryBulkCommand {
                ids: vec![DownloadId(1), DownloadId(2), DownloadId(3), DownloadId(404)],
                new_destination_dir: PathBuf::from("/new"),
            })
            .await
            .expect("bulk handler should always return outcome");

        assert_eq!(outcome.moved, vec![DownloadId(1), DownloadId(3)]);
        assert_eq!(outcome.failed.len(), 2);
        assert_eq!(outcome.failed[0].id, DownloadId(2));
        assert!(outcome.failed[0].message.contains("Extracting"));
        assert_eq!(outcome.failed[1].id, DownloadId(404));
        // Two events fired — one per successful move.
        let evt_count = events
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, DomainEvent::DownloadDirectoryChanged { .. }))
            .count();
        assert_eq!(evt_count, 2);
    }

    #[tokio::test]
    async fn test_change_directory_bulk_empty_ids_returns_empty_outcome() {
        let storage = Arc::new(RecordingStorage::default());
        let engine = Arc::new(RecordingEngine::default());
        let events = Arc::new(RecordingBus::new());
        let bus = build_bus(MockRepo::new(), engine, events, storage);

        let outcome = bus
            .handle_change_directory_bulk(ChangeDirectoryBulkCommand {
                ids: vec![],
                new_destination_dir: PathBuf::from("/wherever"),
            })
            .await
            .expect("empty bulk must succeed");
        assert!(outcome.moved.is_empty());
        assert!(outcome.failed.is_empty());
    }

    #[tokio::test]
    async fn test_change_directory_save_failure_rolls_back_file_move() {
        let dl = make_download_at(8, "/old/y.bin", "y.bin");
        let storage = Arc::new(RecordingStorage::default().with_existing("/old/y.bin"));
        let engine = Arc::new(RecordingEngine::default());
        let events = Arc::new(RecordingBus::new());
        let bus = build_bus(
            MockRepo::new()
                .with(dl)
                .fail_next_save_with(DomainError::StorageError("db down".into())),
            engine,
            events.clone(),
            storage.clone(),
        );

        let err = bus
            .handle_change_directory(ChangeDirectoryCommand {
                id: DownloadId(8),
                new_destination_dir: PathBuf::from("/new"),
            })
            .await
            .expect_err("save error must surface");
        assert!(matches!(
            err,
            AppError::Domain(DomainError::StorageError(_))
        ));
        let body_moves = storage.moves.lock().unwrap().clone();
        // First call is the forward move, second is the rollback to the old path.
        assert_eq!(body_moves.len(), 2);
        assert_eq!(body_moves[0], ("/old/y.bin".into(), "/new/y.bin".into()));
        assert_eq!(body_moves[1], ("/new/y.bin".into(), "/old/y.bin".into()));
        assert!(
            events.events.lock().unwrap().is_empty(),
            "no event on rollback"
        );
    }
}
