use std::path::Path;

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::download::DownloadState;

impl CommandBus {
    pub async fn handle_remove_download(
        &self,
        cmd: super::RemoveDownloadCommand,
    ) -> Result<(), AppError> {
        let download = self
            .download_repo()
            .find_by_id(cmd.id)?
            .ok_or_else(|| AppError::NotFound(format!("Download {} not found", cmd.id.0)))?;

        let is_active = matches!(
            download.state(),
            DownloadState::Downloading | DownloadState::Waiting
        );

        if is_active {
            let _ = self.download_engine().cancel(cmd.id);
        }

        if cmd.delete_files {
            // Remove the downloaded content file
            let dest = Path::new(download.destination_path());
            if dest.exists() {
                let _ = std::fs::remove_file(dest);
            }
            // Remove the .vortex-meta sidecar
            let meta_path = format!("{}.vortex-meta", download.destination_path());
            let _ = self.file_storage().delete_meta(Path::new(&meta_path));
        }

        self.download_repo().delete(cmd.id)?;

        // Only emit DownloadCancelled for active downloads.
        // QueueManager's decrement_and_schedule reacts to this event;
        // emitting for non-active downloads would underflow active_count.
        if is_active {
            self.event_bus()
                .publish(DomainEvent::DownloadCancelled { id: cmd.id });
        }

        self.event_bus()
            .publish(DomainEvent::DownloadRemoved { id: cmd.id });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use crate::application::command_bus::CommandBus;
    use crate::application::commands::RemoveDownloadCommand;
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
        ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine, DownloadRepository,
        EventBus, FileStorage, HttpClient, PluginLoader,
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

        fn with_download(self, dl: Download) -> Self {
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

    struct MockDownloadEngine {
        cancelled: Mutex<Vec<DownloadId>>,
    }

    impl MockDownloadEngine {
        fn new() -> Self {
            Self {
                cancelled: Mutex::new(Vec::new()),
            }
        }
    }

    impl DownloadEngine for MockDownloadEngine {
        fn start(&self, _download: &Download) -> Result<(), DomainError> {
            Ok(())
        }
        fn pause(&self, _id: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
        fn resume(&self, _id: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
        fn cancel(&self, id: DownloadId) -> Result<(), DomainError> {
            self.cancelled.lock().unwrap().push(id);
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
        fn subscribe(&self, _handler: Box<dyn Fn(&DomainEvent) + Send + Sync>) {}
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
        fn create_file(&self, _path: &Path, _size: u64) -> Result<(), DomainError> {
            Ok(())
        }
        fn write_segment(
            &self,
            _path: &Path,
            _offset: u64,
            _data: &[u8],
        ) -> Result<(), DomainError> {
            Ok(())
        }
        fn read_meta(&self, _path: &Path) -> Result<Option<DownloadMeta>, DomainError> {
            Ok(None)
        }
        fn write_meta(&self, _path: &Path, _meta: &DownloadMeta) -> Result<(), DomainError> {
            Ok(())
        }
        fn delete_meta(&self, path: &Path) -> Result<(), DomainError> {
            self.deleted_metas
                .lock()
                .unwrap()
                .push(path.to_string_lossy().into_owned());
            Ok(())
        }
    }

    struct MockHttpClient;

    impl HttpClient for MockHttpClient {
        fn head(&self, _url: &str) -> Result<HttpResponse, DomainError> {
            Ok(HttpResponse {
                status_code: 200,
                headers: HashMap::new(),
                body: vec![],
            })
        }
        fn get_range(&self, _url: &str, _start: u64, _end: u64) -> Result<Vec<u8>, DomainError> {
            Ok(vec![])
        }
        fn supports_range(&self, _url: &str) -> Result<bool, DomainError> {
            Ok(true)
        }
    }

    struct MockPluginLoader;

    impl PluginLoader for MockPluginLoader {
        fn load(&self, _manifest: &PluginManifest) -> Result<(), DomainError> {
            Ok(())
        }
        fn unload(&self, _name: &str) -> Result<(), DomainError> {
            Ok(())
        }
        fn resolve_url(&self, _url: &str) -> Result<Option<PluginInfo>, DomainError> {
            Ok(None)
        }
        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
            Ok(vec![])
        }
        fn set_enabled(&self, _name: &str, _enabled: bool) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct MockConfigStore;

    impl ConfigStore for MockConfigStore {
        fn get_config(&self) -> Result<AppConfig, DomainError> {
            Ok(AppConfig::default())
        }
        fn update_config(&self, _patch: ConfigPatch) -> Result<AppConfig, DomainError> {
            Ok(AppConfig::default())
        }
    }

    struct MockCredentialStore;

    impl CredentialStore for MockCredentialStore {
        fn get(&self, _service: &str) -> Result<Option<Credential>, DomainError> {
            Ok(None)
        }
        fn store(&self, _service: &str, _credential: &Credential) -> Result<(), DomainError> {
            Ok(())
        }
        fn delete(&self, _service: &str) -> Result<(), DomainError> {
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

    fn make_download() -> Download {
        Download::new(
            DownloadId(1),
            Url::new("http://example.com/f.zip").unwrap(),
            "f.zip".to_string(),
            "/tmp/f.zip".to_string(),
        )
    }

    fn make_active_download() -> Download {
        let mut dl = make_download();
        dl.start().unwrap();
        dl
    }

    struct TestHarness {
        bus: CommandBus,
        engine: Arc<MockDownloadEngine>,
        event_bus: Arc<MockEventBus>,
        file_storage: Arc<MockFileStorage>,
    }

    struct FakeArchiveExtractor;
    impl crate::domain::ports::driven::ArchiveExtractor for FakeArchiveExtractor {
        fn detect_format(
            &self,
            _file_path: &std::path::Path,
        ) -> Result<Option<crate::domain::model::archive::ArchiveFormat>, DomainError> {
            Ok(None)
        }
        fn can_extract(&self, _file_path: &std::path::Path) -> Result<bool, DomainError> {
            Ok(false)
        }
        fn extract(
            &self,
            _file_path: &std::path::Path,
            _dest_dir: &std::path::Path,
            _password: Option<&str>,
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
            _file_path: &std::path::Path,
            _password: Option<&str>,
        ) -> Result<Vec<crate::domain::model::archive::ArchiveEntry>, DomainError> {
            Ok(vec![])
        }
        fn detect_segments(
            &self,
            _file_path: &std::path::Path,
        ) -> Result<Option<Vec<std::path::PathBuf>>, DomainError> {
            Ok(None)
        }
    }

    fn make_harness(repo: MockDownloadRepo) -> TestHarness {
        let engine = Arc::new(MockDownloadEngine::new());
        let event_bus = Arc::new(MockEventBus::new());
        let file_storage = Arc::new(MockFileStorage::new());

        let bus = CommandBus::new(
            Arc::new(repo),
            engine.clone(),
            event_bus.clone(),
            file_storage.clone(),
            Arc::new(MockHttpClient),
            Arc::new(MockPluginLoader),
            Arc::new(MockConfigStore),
            Arc::new(MockCredentialStore),
            Arc::new(MockClipboardObserver),
            Arc::new(FakeArchiveExtractor),
        );

        TestHarness {
            bus,
            engine,
            event_bus,
            file_storage,
        }
    }

    #[tokio::test]
    async fn test_remove_deletes_from_db() {
        let dl = make_download();
        let repo = MockDownloadRepo::new().with_download(dl);
        let harness = make_harness(repo);

        let cmd = RemoveDownloadCommand {
            id: DownloadId(1),
            delete_files: false,
        };
        harness.bus.handle_remove_download(cmd).await.unwrap();

        let result = harness
            .bus
            .download_repo()
            .find_by_id(DownloadId(1))
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_remove_active_cancels_engine() {
        let dl = make_active_download();
        let repo = MockDownloadRepo::new().with_download(dl);
        let harness = make_harness(repo);

        let cmd = RemoveDownloadCommand {
            id: DownloadId(1),
            delete_files: false,
        };
        harness.bus.handle_remove_download(cmd).await.unwrap();

        let cancelled = harness.engine.cancelled.lock().unwrap();
        assert_eq!(cancelled.len(), 1);
        assert_eq!(cancelled[0], DownloadId(1));

        let events = harness.event_bus.events.lock().unwrap();
        assert!(events.contains(&DomainEvent::DownloadCancelled { id: DownloadId(1) }));
    }

    #[tokio::test]
    async fn test_remove_not_found() {
        let repo = MockDownloadRepo::new();
        let harness = make_harness(repo);

        let cmd = RemoveDownloadCommand {
            id: DownloadId(999),
            delete_files: false,
        };
        let result = harness.bus.handle_remove_download(cmd).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_remove_with_file_cleanup() {
        let dl = make_download();
        let repo = MockDownloadRepo::new().with_download(dl);
        let harness = make_harness(repo);

        let cmd = RemoveDownloadCommand {
            id: DownloadId(1),
            delete_files: true,
        };
        harness.bus.handle_remove_download(cmd).await.unwrap();

        let deleted = harness.file_storage.deleted_metas.lock().unwrap();
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0], "/tmp/f.zip.vortex-meta");
    }

    #[tokio::test]
    async fn test_remove_emits_download_removed_event() {
        let dl = make_download();
        let repo = MockDownloadRepo::new().with_download(dl);
        let harness = make_harness(repo);

        let cmd = RemoveDownloadCommand {
            id: DownloadId(1),
            delete_files: false,
        };
        harness.bus.handle_remove_download(cmd).await.unwrap();

        let events = harness.event_bus.events.lock().unwrap();
        assert!(events.contains(&DomainEvent::DownloadRemoved { id: DownloadId(1) }));
    }
}
