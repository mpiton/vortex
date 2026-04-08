use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::model::queue::Priority;

impl CommandBus {
    pub async fn handle_set_priority(
        &self,
        cmd: super::SetPriorityCommand,
    ) -> Result<(), AppError> {
        let download = self
            .download_repo()
            .find_by_id(cmd.id)?
            .ok_or_else(|| AppError::NotFound(format!("Download {} not found", cmd.id.0)))?;

        let priority = Priority::new(cmd.priority)?;
        let download = download.with_priority(priority);
        self.download_repo().save(&download)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use crate::application::command_bus::CommandBus;
    use crate::application::commands::SetPriorityCommand;
    use crate::application::error::AppError;
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::config::{AppConfig, ConfigPatch};
    use crate::domain::model::credential::Credential;
    use crate::domain::model::download::{Download, DownloadId, DownloadState, Url};
    use crate::domain::model::http::HttpResponse;
    use crate::domain::model::meta::DownloadMeta;
    use crate::domain::model::plugin::{PluginInfo, PluginManifest};
    use crate::domain::model::queue::Priority;
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

    struct MockEventBus;

    impl EventBus for MockEventBus {
        fn publish(&self, _event: DomainEvent) {}
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

    fn make_command_bus(repo: MockDownloadRepo) -> CommandBus {
        CommandBus::new(
            Arc::new(repo),
            Arc::new(MockDownloadEngine),
            Arc::new(MockEventBus),
            Arc::new(MockFileStorage),
            Arc::new(MockHttpClient),
            Arc::new(MockPluginLoader),
            Arc::new(MockConfigStore),
            Arc::new(MockCredentialStore),
            Arc::new(MockClipboardObserver),
        )
    }

    #[tokio::test]
    async fn test_set_priority_updates_download() {
        let dl = make_download();
        let repo = MockDownloadRepo::new().with_download(dl);
        let bus = make_command_bus(repo);

        let cmd = SetPriorityCommand {
            id: DownloadId(1),
            priority: 8,
        };
        bus.handle_set_priority(cmd).await.unwrap();

        let updated = bus
            .download_repo()
            .find_by_id(DownloadId(1))
            .unwrap()
            .unwrap();
        assert_eq!(updated.priority(), &Priority::new(8).unwrap());
    }

    #[tokio::test]
    async fn test_set_priority_invalid_range_returns_error() {
        let dl = make_download();
        let repo = MockDownloadRepo::new().with_download(dl);
        let bus = make_command_bus(repo);

        let cmd = SetPriorityCommand {
            id: DownloadId(1),
            priority: 0,
        };
        let result = bus.handle_set_priority(cmd).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AppError::Domain(DomainError::InvalidPriority(_))
        ));
    }

    #[tokio::test]
    async fn test_set_priority_not_found() {
        let repo = MockDownloadRepo::new();
        let bus = make_command_bus(repo);

        let cmd = SetPriorityCommand {
            id: DownloadId(999),
            priority: 5,
        };
        let result = bus.handle_set_priority(cmd).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::NotFound(_)));
    }
}
