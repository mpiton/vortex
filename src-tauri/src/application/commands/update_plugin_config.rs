//! Handler for `UpdatePluginConfigCommand`.
//!
//! Validates the new value against the plugin's declared schema, persists
//! it via [`PluginConfigStore`], and updates the loader's in-memory map so
//! subsequent `get_config` calls inside the WASM plugin see the new value
//! without a reload.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::error::DomainError;

impl CommandBus {
    pub async fn handle_update_plugin_config(
        &self,
        cmd: super::UpdatePluginConfigCommand,
    ) -> Result<(), AppError> {
        let manifest = self
            .plugin_loader()
            .get_manifest(&cmd.plugin_name)?
            .ok_or_else(|| {
                AppError::Plugin(format!("plugin '{}' is not loaded", cmd.plugin_name))
            })?;

        if manifest.config_schema().is_empty() {
            return Err(AppError::Domain(DomainError::ValidationError(format!(
                "plugin '{}' has no configuration schema",
                cmd.plugin_name
            ))));
        }

        manifest
            .config_schema()
            .validate(&cmd.key, &cmd.value)
            .map_err(AppError::Domain)?;

        let store = self
            .plugin_config_store()
            .ok_or_else(|| AppError::Plugin("plugin config store not configured".into()))?;
        store
            .set_value(&cmd.plugin_name, &cmd.key, &cmd.value)
            .map_err(AppError::Domain)?;

        self.plugin_loader()
            .set_runtime_config(&cmd.plugin_name, &cmd.key, &cmd.value)
            .map_err(AppError::Domain)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use crate::application::command_bus::CommandBus;
    use crate::application::commands::UpdatePluginConfigCommand;
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::config::{AppConfig, ConfigPatch};
    use crate::domain::model::credential::Credential;
    use crate::domain::model::download::{Download, DownloadId, DownloadState};
    use crate::domain::model::http::HttpResponse;
    use crate::domain::model::meta::DownloadMeta;
    use crate::domain::model::plugin::{
        ConfigField, ConfigFieldType, PluginCategory, PluginConfigSchema, PluginInfo,
        PluginManifest,
    };
    use crate::domain::ports::driven::{
        ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine, DownloadRepository,
        EventBus, FileStorage, HttpClient, PluginConfigStore, PluginLoader,
    };

    struct MockDownloadRepo;
    impl DownloadRepository for MockDownloadRepo {
        fn find_by_id(&self, _: DownloadId) -> Result<Option<Download>, DomainError> {
            Ok(None)
        }
        fn save(&self, _: &Download) -> Result<(), DomainError> {
            Ok(())
        }
        fn delete(&self, _: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
        fn find_by_state(&self, _: DownloadState) -> Result<Vec<Download>, DomainError> {
            Ok(vec![])
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

    struct MockEventBus;
    impl EventBus for MockEventBus {
        fn publish(&self, _: DomainEvent) {}
        fn subscribe(&self, _: Box<dyn Fn(&DomainEvent) + Send + Sync>) {}
    }

    struct MockFileStorage;
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
        fn delete_meta(&self, _: &Path) -> Result<(), DomainError> {
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

    struct StubPluginLoader {
        manifest: Option<PluginManifest>,
        runtime_writes: Mutex<Vec<(String, String, String)>>,
    }

    impl StubPluginLoader {
        fn new(manifest: Option<PluginManifest>) -> Self {
            Self {
                manifest,
                runtime_writes: Mutex::new(Vec::new()),
            }
        }
    }

    impl PluginLoader for StubPluginLoader {
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
        fn get_manifest(&self, _: &str) -> Result<Option<PluginManifest>, DomainError> {
            Ok(self.manifest.clone())
        }
        fn set_runtime_config(
            &self,
            name: &str,
            key: &str,
            value: &str,
        ) -> Result<(), DomainError> {
            self.runtime_writes.lock().unwrap().push((
                name.to_string(),
                key.to_string(),
                value.to_string(),
            ));
            Ok(())
        }
    }

    struct InMemoryPluginConfigStore {
        values: Mutex<HashMap<(String, String), String>>,
    }

    impl InMemoryPluginConfigStore {
        fn new() -> Self {
            Self {
                values: Mutex::new(HashMap::new()),
            }
        }
    }

    impl PluginConfigStore for InMemoryPluginConfigStore {
        fn get_values(&self, plugin_name: &str) -> Result<HashMap<String, String>, DomainError> {
            Ok(self
                .values
                .lock()
                .unwrap()
                .iter()
                .filter(|((p, _), _)| p == plugin_name)
                .map(|((_, k), v)| (k.clone(), v.clone()))
                .collect())
        }
        fn set_value(&self, plugin_name: &str, key: &str, value: &str) -> Result<(), DomainError> {
            self.values.lock().unwrap().insert(
                (plugin_name.to_string(), key.to_string()),
                value.to_string(),
            );
            Ok(())
        }
        fn list_all(&self) -> Result<HashMap<String, HashMap<String, String>>, DomainError> {
            let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
            for ((p, k), v) in self.values.lock().unwrap().iter() {
                out.entry(p.clone())
                    .or_default()
                    .insert(k.clone(), v.clone());
            }
            Ok(out)
        }
        fn delete_all(&self, plugin_name: &str) -> Result<(), DomainError> {
            self.values
                .lock()
                .unwrap()
                .retain(|(p, _), _| p != plugin_name);
            Ok(())
        }
    }

    struct FakeArchiveExtractor;
    impl crate::domain::ports::driven::ArchiveExtractor for FakeArchiveExtractor {
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

    fn make_manifest_with_schema() -> PluginManifest {
        let info = PluginInfo::new(
            "yt".to_string(),
            "1.0.0".to_string(),
            "yt".to_string(),
            "x".to_string(),
            PluginCategory::Crawler,
        );
        let mut schema = PluginConfigSchema::new();
        schema.insert(
            "default_quality",
            ConfigField::new(ConfigFieldType::Enum)
                .with_options(vec!["360p".into(), "720p".into(), "1080p".into()])
                .with_default("720p"),
        );
        PluginManifest::new(info).with_config_schema(schema)
    }

    fn make_bus(
        loader: Arc<StubPluginLoader>,
        store: Arc<InMemoryPluginConfigStore>,
    ) -> CommandBus {
        CommandBus::new(
            Arc::new(MockDownloadRepo),
            Arc::new(MockDownloadEngine),
            Arc::new(MockEventBus),
            Arc::new(MockFileStorage),
            Arc::new(MockHttpClient),
            loader,
            Arc::new(MockConfigStore),
            Arc::new(MockCredentialStore),
            Arc::new(MockClipboardObserver),
            Arc::new(FakeArchiveExtractor),
            Arc::new(crate::application::test_support::NoopHistoryRepo),
            None,
        )
        .with_plugin_config_store(store)
    }

    #[tokio::test]
    async fn test_update_plugin_config_persists_and_propagates() {
        let loader = Arc::new(StubPluginLoader::new(Some(make_manifest_with_schema())));
        let store = Arc::new(InMemoryPluginConfigStore::new());
        let bus = make_bus(loader.clone(), store.clone());

        bus.handle_update_plugin_config(UpdatePluginConfigCommand {
            plugin_name: "yt".into(),
            key: "default_quality".into(),
            value: "1080p".into(),
        })
        .await
        .unwrap();

        assert_eq!(
            store.get_values("yt").unwrap().get("default_quality"),
            Some(&"1080p".to_string())
        );
        let writes = loader.runtime_writes.lock().unwrap().clone();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, "yt");
        assert_eq!(writes[0].1, "default_quality");
        assert_eq!(writes[0].2, "1080p");
    }

    #[tokio::test]
    async fn test_update_plugin_config_rejects_invalid_value() {
        let loader = Arc::new(StubPluginLoader::new(Some(make_manifest_with_schema())));
        let store = Arc::new(InMemoryPluginConfigStore::new());
        let bus = make_bus(loader, store.clone());

        let err = bus
            .handle_update_plugin_config(UpdatePluginConfigCommand {
                plugin_name: "yt".into(),
                key: "default_quality".into(),
                value: "8K".into(),
            })
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            crate::application::error::AppError::Domain(DomainError::ValidationError(_))
        ));
        assert!(store.get_values("yt").unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_update_plugin_config_unknown_plugin_returns_err() {
        let loader = Arc::new(StubPluginLoader::new(None));
        let store = Arc::new(InMemoryPluginConfigStore::new());
        let bus = make_bus(loader, store);

        let err = bus
            .handle_update_plugin_config(UpdatePluginConfigCommand {
                plugin_name: "ghost".into(),
                key: "k".into(),
                value: "v".into(),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            crate::application::error::AppError::Plugin(_)
        ));
    }

    #[tokio::test]
    async fn test_update_plugin_config_unknown_key_returns_not_found() {
        let loader = Arc::new(StubPluginLoader::new(Some(make_manifest_with_schema())));
        let store = Arc::new(InMemoryPluginConfigStore::new());
        let bus = make_bus(loader, store);

        let err = bus
            .handle_update_plugin_config(UpdatePluginConfigCommand {
                plugin_name: "yt".into(),
                key: "ghost".into(),
                value: "x".into(),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            crate::application::error::AppError::Domain(DomainError::NotFound(_))
        ));
    }
}
