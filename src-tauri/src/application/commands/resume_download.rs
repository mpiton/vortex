//! Handler for `ResumeDownloadCommand`.
//!
//! Transitions a paused download back to downloading,
//! tells the engine to resume, persists the change, and emits
//! `DownloadResumed`.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;

impl CommandBus {
    pub async fn handle_resume_download(
        &self,
        cmd: super::ResumeDownloadCommand,
    ) -> Result<(), AppError> {
        let mut download = self
            .download_repo()
            .find_by_id(cmd.id)?
            .ok_or_else(|| AppError::NotFound(format!("Download {} not found", cmd.id.0)))?;

        let event = download.resume()?;
        self.download_engine().resume(cmd.id)?;
        self.download_repo().save(&download)?;
        self.event_bus().publish(event);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use crate::application::command_bus::CommandBus;
    use crate::application::commands::ResumeDownloadCommand;
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

        fn with_download(download: Download) -> Self {
            let repo = Self::new();
            repo.store.lock().unwrap().insert(download.id().0, download);
            repo
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
        resumed: Mutex<Vec<DownloadId>>,
    }

    impl MockDownloadEngine {
        fn new() -> Self {
            Self {
                resumed: Mutex::new(Vec::new()),
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
        fn resume(&self, id: DownloadId) -> Result<(), DomainError> {
            self.resumed.lock().unwrap().push(id);
            Ok(())
        }
        fn cancel(&self, _id: DownloadId) -> Result<(), DomainError> {
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

    fn make_paused_download(id: u64) -> Download {
        let mut dl = Download::new(
            DownloadId(id),
            Url::new("https://example.com/file.zip").unwrap(),
            "file.zip".to_string(),
            "/tmp/file.zip".to_string(),
        );
        dl.start().unwrap();
        dl.pause().unwrap();
        dl
    }

    fn make_command_bus(
        repo: Arc<MockDownloadRepo>,
    ) -> (CommandBus, Arc<MockDownloadEngine>, Arc<MockEventBus>) {
        let engine = Arc::new(MockDownloadEngine::new());
        let event_bus = Arc::new(MockEventBus::new());
        let bus = CommandBus::new(
            repo,
            engine.clone(),
            event_bus.clone(),
            Arc::new(MockFileStorage),
            Arc::new(MockHttpClient),
            Arc::new(MockPluginLoader),
            Arc::new(MockConfigStore),
            Arc::new(MockCredentialStore),
            Arc::new(MockClipboardObserver),
        );
        (bus, engine, event_bus)
    }

    #[tokio::test]
    async fn test_resume_download_restarts_engine() {
        let dl = make_paused_download(1);
        let repo = Arc::new(MockDownloadRepo::with_download(dl));
        let (bus, engine, event_bus) = make_command_bus(repo.clone());

        let cmd = ResumeDownloadCommand { id: DownloadId(1) };
        bus.handle_resume_download(cmd).await.unwrap();

        // Verify state persisted as Downloading
        let saved = repo.store.lock().unwrap().get(&1).cloned().unwrap();
        assert_eq!(saved.state(), DownloadState::Downloading);

        // Verify engine was told to resume
        let resumed = engine.resumed.lock().unwrap();
        assert_eq!(resumed.len(), 1);
        assert_eq!(resumed[0], DownloadId(1));

        // Verify event emitted
        let events = event_bus.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            DomainEvent::DownloadResumed { id: DownloadId(1) }
        );
    }

    #[tokio::test]
    async fn test_resume_not_paused_returns_error() {
        // Download is in Queued state (not paused)
        let dl = Download::new(
            DownloadId(1),
            Url::new("https://example.com/file.zip").unwrap(),
            "file.zip".to_string(),
            "/tmp/file.zip".to_string(),
        );
        let repo = Arc::new(MockDownloadRepo::with_download(dl));
        let (bus, _, _) = make_command_bus(repo);

        let cmd = ResumeDownloadCommand { id: DownloadId(1) };
        let result = bus.handle_resume_download(cmd).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_resume_download_not_found() {
        let repo = Arc::new(MockDownloadRepo::new());
        let (bus, _, _) = make_command_bus(repo);

        let cmd = ResumeDownloadCommand {
            id: DownloadId(999),
        };
        let result = bus.handle_resume_download(cmd).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            crate::application::error::AppError::NotFound(_)
        ));
    }
}
