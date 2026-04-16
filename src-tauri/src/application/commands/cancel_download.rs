use std::path::Path;

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::download::DownloadState;

impl CommandBus {
    pub async fn handle_cancel_download(
        &self,
        cmd: super::CancelDownloadCommand,
    ) -> Result<(), AppError> {
        let download = self
            .download_repo()
            .find_by_id(cmd.id)?
            .ok_or_else(|| AppError::NotFound(format!("Download {} not found", cmd.id.0)))?;

        // Cancel engine if download is active
        let is_active = matches!(
            download.state(),
            DownloadState::Downloading | DownloadState::Waiting
        );
        if is_active {
            self.download_engine().cancel(cmd.id)?;
        }

        // Cleanup metadata file (best-effort, log on failure)
        let meta_path = format!("{}.vortex-meta", download.destination_path());
        if let Err(e) = self.file_storage().delete_meta(Path::new(&meta_path)) {
            tracing::warn!("Failed to delete meta for download {:?}: {e}", cmd.id);
        }

        // Remove from persistence
        self.download_repo().delete(cmd.id)?;

        // Only emit DownloadCancelled for active downloads.
        // QueueManager's decrement_and_schedule reacts to this event;
        // emitting it for non-active downloads would underflow active_count.
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
    use crate::application::commands::CancelDownloadCommand;
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

    struct MockFileStorage;

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

        fn delete_meta(&self, _path: &Path) -> Result<(), DomainError> {
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

        fn get_range(&self, _url: &str, start: u64, end: u64) -> Result<Vec<u8>, DomainError> {
            Ok(vec![
                0u8;
                end.saturating_sub(start).saturating_add(1) as usize
            ])
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

    fn make_command_bus(
        repo: Arc<MockDownloadRepo>,
        engine: Arc<MockDownloadEngine>,
        events: Arc<MockEventBus>,
    ) -> CommandBus {
        CommandBus::new(
            repo,
            engine,
            events,
            Arc::new(MockFileStorage),
            Arc::new(MockHttpClient),
            Arc::new(MockPluginLoader),
            Arc::new(MockConfigStore),
            Arc::new(MockCredentialStore),
            Arc::new(MockClipboardObserver),
            Arc::new(FakeArchiveExtractor),
            None,
        )
    }

    fn make_download(id: u64) -> Download {
        Download::new(
            DownloadId(id),
            Url::new("http://example.com/file.zip").unwrap(),
            "file.zip".to_string(),
            "/tmp/file.zip".to_string(),
        )
    }

    #[tokio::test]
    async fn test_cancel_download_cleans_up() {
        let repo = Arc::new(MockDownloadRepo::new());
        let engine = Arc::new(MockDownloadEngine::new());
        let events = Arc::new(MockEventBus::new());

        // Insert an active (Downloading) download
        let mut dl = make_download(1);
        dl.start().unwrap();
        repo.save(&dl).unwrap();

        let bus = make_command_bus(repo.clone(), engine.clone(), events.clone());
        let result = bus
            .handle_cancel_download(CancelDownloadCommand { id: DownloadId(1) })
            .await;

        assert!(result.is_ok());
        // Engine cancel was called
        assert_eq!(
            engine.cancelled.lock().unwrap().as_slice(),
            &[DownloadId(1)]
        );
        // Download removed from repo
        assert!(repo.find_by_id(DownloadId(1)).unwrap().is_none());
        // DownloadCancelled event emitted
        let emitted = events.events.lock().unwrap();
        assert_eq!(
            emitted.as_slice(),
            &[
                DomainEvent::DownloadCancelled { id: DownloadId(1) },
                DomainEvent::DownloadRemoved { id: DownloadId(1) },
            ]
        );
    }

    #[tokio::test]
    async fn test_cancel_not_found() {
        let repo = Arc::new(MockDownloadRepo::new());
        let engine = Arc::new(MockDownloadEngine::new());
        let events = Arc::new(MockEventBus::new());

        let bus = make_command_bus(repo, engine, events);
        let result = bus
            .handle_cancel_download(CancelDownloadCommand {
                id: DownloadId(999),
            })
            .await;

        assert!(matches!(result, Err(AppError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_cancel_queued_no_engine_cancel() {
        let repo = Arc::new(MockDownloadRepo::new());
        let engine = Arc::new(MockDownloadEngine::new());
        let events = Arc::new(MockEventBus::new());

        // Insert a Queued download (not active)
        let dl = make_download(2);
        repo.save(&dl).unwrap();

        let bus = make_command_bus(repo.clone(), engine.clone(), events.clone());
        let result = bus
            .handle_cancel_download(CancelDownloadCommand { id: DownloadId(2) })
            .await;

        assert!(result.is_ok());
        // Engine cancel NOT called for non-active state
        assert!(engine.cancelled.lock().unwrap().is_empty());
        // Download still removed
        assert!(repo.find_by_id(DownloadId(2)).unwrap().is_none());
        // No DownloadCancelled event for non-active downloads, but removal is still emitted.
        let emitted = events.events.lock().unwrap();
        assert_eq!(
            emitted.as_slice(),
            &[DomainEvent::DownloadRemoved { id: DownloadId(2) }]
        );
    }
}
