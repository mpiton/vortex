//! Handlers for `EnablePluginCommand` and `DisablePluginCommand`.
//!
//! Toggles the enabled state of a loaded plugin via `PluginLoader::set_enabled`.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;

impl CommandBus {
    pub async fn handle_enable_plugin(
        &self,
        cmd: super::EnablePluginCommand,
    ) -> Result<(), AppError> {
        self.plugin_loader().set_enabled(&cmd.name, true)?;
        Ok(())
    }

    pub async fn handle_disable_plugin(
        &self,
        cmd: super::DisablePluginCommand,
    ) -> Result<(), AppError> {
        self.plugin_loader().set_enabled(&cmd.name, false)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Arc;

    use crate::application::command_bus::CommandBus;
    use crate::application::commands::{DisablePluginCommand, EnablePluginCommand};
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
        plugins: Vec<PluginInfo>,
    }

    impl MockPluginLoader {
        fn empty() -> Self {
            Self { plugins: vec![] }
        }

        fn with_plugin(name: &str) -> Self {
            let info = PluginInfo::new(
                name.to_string(),
                "1.0.0".to_string(),
                "desc".to_string(),
                "author".to_string(),
                PluginCategory::Utility,
            );
            Self {
                plugins: vec![info],
            }
        }
    }

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
            Ok(self.plugins.clone())
        }
        fn set_enabled(&self, name: &str, _enabled: bool) -> Result<(), DomainError> {
            if self.plugins.iter().any(|p| p.name() == name) {
                Ok(())
            } else {
                Err(DomainError::NotFound(name.to_string()))
            }
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

    fn make_bus(plugin_loader: Arc<dyn PluginLoader>) -> CommandBus {
        CommandBus::new(
            Arc::new(MockDownloadRepo),
            Arc::new(MockDownloadEngine),
            Arc::new(MockEventBus),
            Arc::new(MockFileStorage),
            Arc::new(MockHttpClient),
            plugin_loader,
            Arc::new(MockConfigStore),
            Arc::new(MockCredentialStore),
            Arc::new(MockClipboardObserver),
            Arc::new(FakeArchiveExtractor),
        )
    }

    #[tokio::test]
    async fn test_enable_plugin_returns_ok_when_plugin_loaded() {
        let bus = make_bus(Arc::new(MockPluginLoader::with_plugin("my-plugin")));
        let cmd = EnablePluginCommand {
            name: "my-plugin".to_string(),
        };
        assert!(bus.handle_enable_plugin(cmd).await.is_ok());
    }

    #[tokio::test]
    async fn test_enable_plugin_returns_not_found_when_plugin_absent() {
        let bus = make_bus(Arc::new(MockPluginLoader::empty()));
        let cmd = EnablePluginCommand {
            name: "missing-plugin".to_string(),
        };
        let result = bus.handle_enable_plugin(cmd).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(
                err,
                crate::application::error::AppError::Domain(
                    crate::domain::error::DomainError::NotFound(_)
                )
            ),
            "expected NotFound, got {err:?}"
        );
    }

    #[tokio::test]
    async fn test_disable_plugin_returns_ok_when_plugin_loaded() {
        let bus = make_bus(Arc::new(MockPluginLoader::with_plugin("my-plugin")));
        let cmd = DisablePluginCommand {
            name: "my-plugin".to_string(),
        };
        assert!(bus.handle_disable_plugin(cmd).await.is_ok());
    }

    #[tokio::test]
    async fn test_disable_plugin_returns_not_found_when_plugin_absent() {
        let bus = make_bus(Arc::new(MockPluginLoader::empty()));
        let cmd = DisablePluginCommand {
            name: "missing-plugin".to_string(),
        };
        let result = bus.handle_disable_plugin(cmd).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(
                err,
                crate::application::error::AppError::Domain(
                    crate::domain::error::DomainError::NotFound(_)
                )
            ),
            "expected NotFound, got {err:?}"
        );
    }
}
