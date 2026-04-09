use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;

impl CommandBus {
    pub async fn handle_retry_download(
        &self,
        cmd: super::RetryDownloadCommand,
    ) -> Result<(), AppError> {
        let mut download = self
            .download_repo()
            .find_by_id(cmd.id)?
            .ok_or_else(|| AppError::NotFound(format!("Download {} not found", cmd.id.0)))?;

        let event = download.retry()?;
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
    use crate::application::commands::RetryDownloadCommand;
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

    struct MockDownloadEngine;

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

    fn make_command_bus(repo: Arc<MockDownloadRepo>, events: Arc<MockEventBus>) -> CommandBus {
        CommandBus::new(
            repo,
            Arc::new(MockDownloadEngine),
            events,
            Arc::new(MockFileStorage),
            Arc::new(MockHttpClient),
            Arc::new(MockPluginLoader),
            Arc::new(MockConfigStore),
            Arc::new(MockCredentialStore),
            Arc::new(MockClipboardObserver),
        )
    }

    fn make_download_in_error(id: u64) -> Download {
        let mut dl = Download::new(
            DownloadId(id),
            Url::new("http://example.com/file.zip").unwrap(),
            "file.zip".to_string(),
            "/tmp/file.zip".to_string(),
        );
        dl.start().unwrap();
        dl.fail("test error".to_string()).unwrap();
        dl
    }

    #[tokio::test]
    async fn test_retry_transitions_to_retry_state() {
        let repo = Arc::new(MockDownloadRepo::new());
        let events = Arc::new(MockEventBus::new());

        let dl = make_download_in_error(1);
        repo.save(&dl).unwrap();

        let bus = make_command_bus(repo.clone(), events.clone());
        let result = bus
            .handle_retry_download(RetryDownloadCommand { id: DownloadId(1) })
            .await;

        assert!(result.is_ok());
        // Download is now in Retry state
        let saved = repo.find_by_id(DownloadId(1)).unwrap().unwrap();
        assert_eq!(saved.state(), DownloadState::Retry);
        assert_eq!(saved.retry_count(), 1);
        // DownloadRetrying event emitted
        let emitted = events.events.lock().unwrap();
        assert_eq!(
            emitted.as_slice(),
            &[DomainEvent::DownloadRetrying {
                id: DownloadId(1),
                attempt: 1
            }]
        );
    }

    #[tokio::test]
    async fn test_retry_max_exceeded_returns_error() {
        let repo = Arc::new(MockDownloadRepo::new());
        let events = Arc::new(MockEventBus::new());

        // Create download that has exhausted retries
        let mut dl = Download::new(
            DownloadId(2),
            Url::new("http://example.com/file.zip").unwrap(),
            "file.zip".to_string(),
            "/tmp/file.zip".to_string(),
        )
        .with_max_retries(0);
        dl.start().unwrap();
        dl.fail("error".to_string()).unwrap();
        repo.save(&dl).unwrap();

        let bus = make_command_bus(repo, events);
        let result = bus
            .handle_retry_download(RetryDownloadCommand { id: DownloadId(2) })
            .await;

        assert!(matches!(
            result,
            Err(AppError::Domain(DomainError::MaxRetriesExceeded { .. }))
        ));
    }

    #[tokio::test]
    async fn test_retry_not_error_state() {
        let repo = Arc::new(MockDownloadRepo::new());
        let events = Arc::new(MockEventBus::new());

        // Queued download cannot be retried
        let dl = Download::new(
            DownloadId(3),
            Url::new("http://example.com/file.zip").unwrap(),
            "file.zip".to_string(),
            "/tmp/file.zip".to_string(),
        );
        repo.save(&dl).unwrap();

        let bus = make_command_bus(repo, events);
        let result = bus
            .handle_retry_download(RetryDownloadCommand { id: DownloadId(3) })
            .await;

        assert!(matches!(
            result,
            Err(AppError::Domain(DomainError::InvalidTransition { .. }))
        ));
    }
}
