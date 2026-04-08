use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::model::download::DownloadState;

impl CommandBus {
    pub async fn handle_resume_all(
        &self,
        _cmd: super::ResumeAllDownloadsCommand,
    ) -> Result<u32, AppError> {
        let downloads = self.download_repo().find_by_state(DownloadState::Paused)?;

        let mut count = 0u32;
        for mut dl in downloads {
            if let Ok(event) = dl.resume() {
                self.download_repo().save(&dl)?;
                let _ = self.download_engine().resume(dl.id());
                self.event_bus().publish(event);
                count += 1;
            }
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
        )
    }

    fn make_paused(id: u64) -> Download {
        let mut dl = Download::new(
            DownloadId(id),
            Url::new("http://example.com/file.zip").unwrap(),
            "file.zip".to_string(),
            "/tmp/file.zip".to_string(),
        );
        dl.start().unwrap();
        dl.pause().unwrap();
        dl
    }

    #[tokio::test]
    async fn test_resume_all_resumes_paused() {
        let repo = Arc::new(MockDownloadRepo::new());
        let engine = Arc::new(MockDownloadEngine::new());
        let events = Arc::new(MockEventBus::new());

        // Insert 2 paused downloads
        repo.save(&make_paused(1)).unwrap();
        repo.save(&make_paused(2)).unwrap();

        let bus = make_command_bus(repo.clone(), engine.clone(), events.clone());
        let count = bus
            .handle_resume_all(super::super::ResumeAllDownloadsCommand)
            .await
            .unwrap();

        assert_eq!(count, 2);
        // Both resumed in repo
        let d1 = repo.find_by_id(DownloadId(1)).unwrap().unwrap();
        let d2 = repo.find_by_id(DownloadId(2)).unwrap().unwrap();
        assert_eq!(d1.state(), DownloadState::Downloading);
        assert_eq!(d2.state(), DownloadState::Downloading);
        // Engine resume called for both
        let resumed = engine.resumed.lock().unwrap();
        assert_eq!(resumed.len(), 2);
        // 2 events emitted
        let emitted = events.events.lock().unwrap();
        assert_eq!(emitted.len(), 2);
    }

    #[tokio::test]
    async fn test_resume_all_empty_returns_zero() {
        let repo = Arc::new(MockDownloadRepo::new());
        let engine = Arc::new(MockDownloadEngine::new());
        let events = Arc::new(MockEventBus::new());

        let bus = make_command_bus(repo, engine, events);
        let count = bus
            .handle_resume_all(super::super::ResumeAllDownloadsCommand)
            .await
            .unwrap();

        assert_eq!(count, 0);
    }
}
