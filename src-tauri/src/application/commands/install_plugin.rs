//! Handler for `InstallPluginCommand`.
//!
//! Loads a pre-parsed plugin manifest via the PluginLoader port
//! and emits a `PluginLoaded` domain event on success.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;

impl CommandBus {
    pub async fn handle_install_plugin(
        &self,
        cmd: super::InstallPluginCommand,
    ) -> Result<(), AppError> {
        self.plugin_loader().load(&cmd.manifest)?;

        self.event_bus().publish(DomainEvent::PluginLoaded {
            name: cmd.manifest.info().name().to_string(),
            version: cmd.manifest.info().version().to_string(),
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use crate::application::command_bus::CommandBus;
    use crate::application::commands::InstallPluginCommand;
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::config::{AppConfig, ConfigPatch};
    use crate::domain::model::credential::Credential;
    use crate::domain::model::download::{Download, DownloadId, DownloadState};
    use crate::domain::model::http::HttpResponse;
    use crate::domain::model::meta::DownloadMeta;
    use crate::domain::model::plugin::{PluginCategory, PluginInfo, PluginManifest};
    use crate::domain::ports::driven::{
        ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine, DownloadRepository,
        EventBus, FileStorage, HttpClient, PluginLoader,
    };

    struct MockDownloadRepo;
    impl DownloadRepository for MockDownloadRepo {
        fn find_by_id(&self, _id: DownloadId) -> Result<Option<Download>, DomainError> {
            Ok(None)
        }
        fn save(&self, _d: &Download) -> Result<(), DomainError> {
            Ok(())
        }
        fn delete(&self, _id: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
        fn find_by_state(&self, _s: DownloadState) -> Result<Vec<Download>, DomainError> {
            Ok(vec![])
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

    struct MockPluginLoader {
        loaded: Mutex<Vec<String>>,
        should_fail: bool,
    }

    impl MockPluginLoader {
        fn new() -> Self {
            Self {
                loaded: Mutex::new(Vec::new()),
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                loaded: Mutex::new(Vec::new()),
                should_fail: true,
            }
        }
    }

    impl PluginLoader for MockPluginLoader {
        fn load(&self, manifest: &PluginManifest) -> Result<(), DomainError> {
            if self.should_fail {
                return Err(DomainError::PluginError("load failed".to_string()));
            }
            self.loaded
                .lock()
                .unwrap()
                .push(manifest.info().name().to_string());
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

    fn make_manifest(name: &str, version: &str) -> PluginManifest {
        let info = PluginInfo::new(
            name.to_string(),
            version.to_string(),
            "A test plugin".to_string(),
            "test-author".to_string(),
            PluginCategory::Hoster,
        );
        PluginManifest::new(info)
    }

    fn make_bus(plugin_loader: Arc<dyn PluginLoader>, event_bus: Arc<MockEventBus>) -> CommandBus {
        CommandBus::new(
            Arc::new(MockDownloadRepo),
            Arc::new(MockDownloadEngine),
            event_bus,
            Arc::new(MockFileStorage),
            Arc::new(MockHttpClient),
            plugin_loader,
            Arc::new(MockConfigStore),
            Arc::new(MockCredentialStore),
            Arc::new(MockClipboardObserver),
        )
    }

    #[tokio::test]
    async fn test_install_plugin_loads_and_emits_event() {
        let loader = Arc::new(MockPluginLoader::new());
        let event_bus = Arc::new(MockEventBus::new());
        let bus = make_bus(loader.clone(), event_bus.clone());

        let manifest = make_manifest("my-plugin", "1.0.0");
        let cmd = InstallPluginCommand {
            manifest: manifest.clone(),
        };

        let result = bus.handle_install_plugin(cmd).await;
        assert!(result.is_ok());

        let loaded = loader.loaded.lock().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], "my-plugin");

        let events = event_bus.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            DomainEvent::PluginLoaded {
                name: "my-plugin".to_string(),
                version: "1.0.0".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn test_install_plugin_loader_error_returns_err() {
        let loader = Arc::new(MockPluginLoader::failing());
        let event_bus = Arc::new(MockEventBus::new());
        let bus = make_bus(loader, event_bus.clone());

        let cmd = InstallPluginCommand {
            manifest: make_manifest("bad-plugin", "0.1.0"),
        };

        let result = bus.handle_install_plugin(cmd).await;
        assert!(result.is_err());

        let events = event_bus.events.lock().unwrap();
        assert!(events.is_empty(), "no event should be emitted on failure");
    }
}
