use std::io::ErrorKind;
use std::path::Path;

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::download::DownloadState;

impl CommandBus {
    pub async fn handle_clear_downloads_by_state(
        &self,
        cmd: super::ClearDownloadsByStateCommand,
    ) -> Result<u32, AppError> {
        if !matches!(cmd.state, DownloadState::Completed | DownloadState::Error) {
            return Err(AppError::Validation(
                "state must be Completed or Error".to_string(),
            ));
        }

        let downloads = self.download_repo().find_by_state(cmd.state)?;
        let mut count: u32 = 0;

        for download in downloads {
            // Repository delete first — if the durable store rejects the write
            // we must not orphan files on disk under a gone DB row.
            if let Err(e) = self.download_repo().delete(download.id()) {
                tracing::error!(
                    id = download.id().0,
                    error = %e,
                    "failed to delete download from repository; skipping file cleanup"
                );
                continue;
            }

            if cmd.delete_files {
                let dest = Path::new(download.destination_path());
                if let Err(e) = std::fs::remove_file(dest)
                    && e.kind() != ErrorKind::NotFound
                {
                    tracing::warn!(
                        path = %dest.display(),
                        error = %e,
                        "failed to delete download file"
                    );
                }
                if let Err(e) = self.file_storage().delete_meta(dest) {
                    tracing::warn!(
                        path = %format!("{}.vortex-meta", download.destination_path()),
                        error = %e,
                        "failed to delete .vortex-meta sidecar"
                    );
                }
            }

            self.event_bus()
                .publish(DomainEvent::DownloadRemoved { id: download.id() });
            count += 1;
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use crate::application::command_bus::CommandBus;
    use crate::application::commands::ClearDownloadsByStateCommand;
    use crate::application::error::AppError;
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
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

    struct MockDownloadRepo {
        store: Mutex<HashMap<u64, Download>>,
    }
    impl MockDownloadRepo {
        fn new() -> Self {
            Self {
                store: Mutex::new(HashMap::new()),
            }
        }
        fn with(self, dl: Download) -> Self {
            self.store.lock().unwrap().insert(dl.id().0, dl);
            self
        }
    }
    impl DownloadRepository for MockDownloadRepo {
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

    struct MockDownloadEngine;
    impl DownloadEngine for MockDownloadEngine {
        fn start(&self, _: &Download) -> Result<(), DomainError> {
            Ok(())
        }
        fn pause(&self, _: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
        fn resume(&self, _: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
        fn cancel(&self, _: DownloadId) -> Result<(), DomainError> {
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
        fn publish(&self, e: DomainEvent) {
            self.events.lock().unwrap().push(e);
        }
        fn subscribe(&self, _: Box<dyn Fn(&DomainEvent) + Send + Sync>) {}
    }

    struct MockFileStorage {
        deleted_metas: Mutex<Vec<String>>,
    }
    impl MockFileStorage {
        fn new() -> Self {
            Self {
                deleted_metas: Mutex::new(Vec::new()),
            }
        }
    }
    impl FileStorage for MockFileStorage {
        fn create_file(&self, _: &Path, _: u64) -> Result<(), DomainError> {
            Ok(())
        }
        fn write_segment(&self, _: &Path, _: u64, _: &[u8]) -> Result<(), DomainError> {
            Ok(())
        }
        fn read_meta(&self, _: &Path) -> Result<Option<DownloadMeta>, DomainError> {
            Ok(None)
        }
        fn write_meta(&self, _: &Path, _: &DownloadMeta) -> Result<(), DomainError> {
            Ok(())
        }
        fn delete_meta(&self, p: &Path) -> Result<(), DomainError> {
            self.deleted_metas
                .lock()
                .unwrap()
                .push(p.to_string_lossy().into_owned());
            Ok(())
        }
    }

    struct MockHttpClient;
    impl HttpClient for MockHttpClient {
        fn head(&self, _: &str) -> Result<HttpResponse, DomainError> {
            Ok(HttpResponse {
                status_code: 200,
                headers: HashMap::new(),
                body: vec![],
            })
        }
        fn get_range(&self, _: &str, _: u64, _: u64) -> Result<Vec<u8>, DomainError> {
            Ok(vec![])
        }
        fn supports_range(&self, _: &str) -> Result<bool, DomainError> {
            Ok(true)
        }
    }

    struct MockPluginLoader;
    impl PluginLoader for MockPluginLoader {
        fn load(&self, _: &PluginManifest) -> Result<(), DomainError> {
            Ok(())
        }
        fn unload(&self, _: &str) -> Result<(), DomainError> {
            Ok(())
        }
        fn resolve_url(&self, _: &str) -> Result<Option<PluginInfo>, DomainError> {
            Ok(None)
        }
        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
            Ok(vec![])
        }
        fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct MockConfigStore;
    impl ConfigStore for MockConfigStore {
        fn get_config(&self) -> Result<AppConfig, DomainError> {
            Ok(AppConfig::default())
        }
        fn update_config(&self, _: ConfigPatch) -> Result<AppConfig, DomainError> {
            Ok(AppConfig::default())
        }
    }

    struct MockCredentialStore;
    impl CredentialStore for MockCredentialStore {
        fn get(&self, _: &str) -> Result<Option<Credential>, DomainError> {
            Ok(None)
        }
        fn store(&self, _: &str, _: &Credential) -> Result<(), DomainError> {
            Ok(())
        }
        fn delete(&self, _: &str) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct MockClipboardObserver;
    impl ClipboardObserver for MockClipboardObserver {
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

    struct FakeArchiveExtractor;
    impl ArchiveExtractor for FakeArchiveExtractor {
        fn detect_format(
            &self,
            _: &Path,
        ) -> Result<Option<crate::domain::model::archive::ArchiveFormat>, DomainError> {
            Ok(None)
        }
        fn can_extract(&self, _: &Path) -> Result<bool, DomainError> {
            Ok(false)
        }
        fn extract(
            &self,
            _: &Path,
            _: &Path,
            _: Option<&str>,
        ) -> Result<crate::domain::model::archive::ExtractSummary, DomainError> {
            Ok(crate::domain::model::archive::ExtractSummary {
                extracted_files: 0,
                extracted_bytes: 0,
                duration_ms: 0,
                warnings: vec![],
            })
        }
        fn list_contents(
            &self,
            _: &Path,
            _: Option<&str>,
        ) -> Result<Vec<crate::domain::model::archive::ArchiveEntry>, DomainError> {
            Ok(vec![])
        }
        fn detect_segments(
            &self,
            _: &Path,
        ) -> Result<Option<Vec<std::path::PathBuf>>, DomainError> {
            Ok(None)
        }
    }

    fn completed_download(id: u64, path: &str) -> Download {
        let mut d = Download::new(
            DownloadId(id),
            Url::new("http://example.com/f.zip").unwrap(),
            format!("f{id}.zip"),
            path.to_string(),
        );
        d.start().unwrap();
        d.complete().unwrap();
        d
    }

    fn errored_download(id: u64, path: &str) -> Download {
        let mut d = Download::new(
            DownloadId(id),
            Url::new("http://example.com/f.zip").unwrap(),
            format!("f{id}.zip"),
            path.to_string(),
        );
        d.start().unwrap();
        d.fail("boom".to_string()).unwrap();
        d
    }

    struct TestHarness {
        bus: CommandBus,
        event_bus: Arc<MockEventBus>,
        file_storage: Arc<MockFileStorage>,
    }

    fn make_harness(repo: MockDownloadRepo) -> TestHarness {
        let event_bus = Arc::new(MockEventBus::new());
        let file_storage = Arc::new(MockFileStorage::new());
        let bus = CommandBus::new(
            Arc::new(repo),
            Arc::new(MockDownloadEngine),
            event_bus.clone(),
            file_storage.clone(),
            Arc::new(MockHttpClient),
            Arc::new(MockPluginLoader),
            Arc::new(MockConfigStore),
            Arc::new(MockCredentialStore),
            Arc::new(MockClipboardObserver),
            Arc::new(FakeArchiveExtractor),
            None,
        );
        TestHarness {
            bus,
            event_bus,
            file_storage,
        }
    }

    #[tokio::test]
    async fn test_clear_completed_returns_count_and_deletes_from_db() {
        let repo = MockDownloadRepo::new()
            .with(completed_download(1, "/tmp/a.zip"))
            .with(completed_download(2, "/tmp/b.zip"));
        let h = make_harness(repo);

        let cmd = ClearDownloadsByStateCommand {
            state: DownloadState::Completed,
            delete_files: false,
        };
        let count = h.bus.handle_clear_downloads_by_state(cmd).await.unwrap();

        assert_eq!(count, 2);
        assert!(
            h.bus
                .download_repo()
                .find_by_id(DownloadId(1))
                .unwrap()
                .is_none()
        );
        assert!(
            h.bus
                .download_repo()
                .find_by_id(DownloadId(2))
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_clear_failed_returns_count() {
        let repo = MockDownloadRepo::new()
            .with(errored_download(1, "/tmp/a.zip"))
            .with(completed_download(2, "/tmp/b.zip"));
        let h = make_harness(repo);

        let cmd = ClearDownloadsByStateCommand {
            state: DownloadState::Error,
            delete_files: false,
        };
        let count = h.bus.handle_clear_downloads_by_state(cmd).await.unwrap();

        assert_eq!(count, 1);
        assert!(
            h.bus
                .download_repo()
                .find_by_id(DownloadId(2))
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn test_clear_non_terminal_state_returns_validation_error() {
        let h = make_harness(MockDownloadRepo::new());
        let cmd = ClearDownloadsByStateCommand {
            state: DownloadState::Downloading,
            delete_files: false,
        };
        let err = h
            .bus
            .handle_clear_downloads_by_state(cmd)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn test_clear_emits_one_removed_event_per_cleared_download() {
        let repo = MockDownloadRepo::new()
            .with(completed_download(1, "/tmp/a.zip"))
            .with(completed_download(2, "/tmp/b.zip"));
        let h = make_harness(repo);

        let cmd = ClearDownloadsByStateCommand {
            state: DownloadState::Completed,
            delete_files: false,
        };
        h.bus.handle_clear_downloads_by_state(cmd).await.unwrap();

        let events = h.event_bus.events.lock().unwrap();
        let removed: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                DomainEvent::DownloadRemoved { id } => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(removed.len(), 2);
        assert!(removed.contains(&DownloadId(1)));
        assert!(removed.contains(&DownloadId(2)));
    }

    #[tokio::test]
    async fn test_clear_with_delete_files_calls_filestorage_delete_meta() {
        let repo = MockDownloadRepo::new().with(completed_download(1, "/tmp/a.zip"));
        let h = make_harness(repo);

        let cmd = ClearDownloadsByStateCommand {
            state: DownloadState::Completed,
            delete_files: true,
        };
        h.bus.handle_clear_downloads_by_state(cmd).await.unwrap();

        let metas = h.file_storage.deleted_metas.lock().unwrap();
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0], "/tmp/a.zip");
    }

    #[tokio::test]
    async fn test_clear_without_delete_files_skips_filestorage() {
        let repo = MockDownloadRepo::new().with(completed_download(1, "/tmp/a.zip"));
        let h = make_harness(repo);

        let cmd = ClearDownloadsByStateCommand {
            state: DownloadState::Completed,
            delete_files: false,
        };
        h.bus.handle_clear_downloads_by_state(cmd).await.unwrap();

        assert!(h.file_storage.deleted_metas.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_clear_missing_file_is_idempotent() {
        let repo = MockDownloadRepo::new().with(completed_download(
            1,
            "/nonexistent/definitely/not/here.zip",
        ));
        let h = make_harness(repo);

        let cmd = ClearDownloadsByStateCommand {
            state: DownloadState::Completed,
            delete_files: true,
        };
        let count = h.bus.handle_clear_downloads_by_state(cmd).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_clear_empty_returns_zero() {
        let h = make_harness(MockDownloadRepo::new());
        let cmd = ClearDownloadsByStateCommand {
            state: DownloadState::Completed,
            delete_files: true,
        };
        let count = h.bus.handle_clear_downloads_by_state(cmd).await.unwrap();
        assert_eq!(count, 0);
        assert!(h.event_bus.events.lock().unwrap().is_empty());
    }
}
