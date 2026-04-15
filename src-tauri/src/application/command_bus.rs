//! CQRS command bus — dispatches commands to their handlers.
//!
//! Holds references to all driven ports needed by command handlers.
//! Actual handler implementations will be added in tasks 11-12.

use std::sync::Arc;

use crate::domain::ports::driven::{
    ArchiveExtractor, ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine,
    DownloadRepository, EventBus, FileStorage, HttpClient, PluginLoader, PluginStoreClient,
};

/// Central dispatcher for CQRS commands.
///
/// Each driven port is injected via the constructor as `Arc<dyn Trait>`.
/// Command handler `impl` blocks will be added in later tasks.
pub struct CommandBus {
    download_repo: Arc<dyn DownloadRepository>,
    download_engine: Arc<dyn DownloadEngine>,
    event_bus: Arc<dyn EventBus>,
    file_storage: Arc<dyn FileStorage>,
    http_client: Arc<dyn HttpClient>,
    plugin_loader: Arc<dyn PluginLoader>,
    config_store: Arc<dyn ConfigStore>,
    credential_store: Arc<dyn CredentialStore>,
    clipboard_observer: Arc<dyn ClipboardObserver>,
    archive_extractor: Arc<dyn ArchiveExtractor>,
    plugin_store_client: Option<Arc<dyn PluginStoreClient>>,
}

impl CommandBus {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        download_repo: Arc<dyn DownloadRepository>,
        download_engine: Arc<dyn DownloadEngine>,
        event_bus: Arc<dyn EventBus>,
        file_storage: Arc<dyn FileStorage>,
        http_client: Arc<dyn HttpClient>,
        plugin_loader: Arc<dyn PluginLoader>,
        config_store: Arc<dyn ConfigStore>,
        credential_store: Arc<dyn CredentialStore>,
        clipboard_observer: Arc<dyn ClipboardObserver>,
        archive_extractor: Arc<dyn ArchiveExtractor>,
        plugin_store_client: Option<Arc<dyn PluginStoreClient>>,
    ) -> Self {
        Self {
            download_repo,
            download_engine,
            event_bus,
            file_storage,
            http_client,
            plugin_loader,
            config_store,
            credential_store,
            clipboard_observer,
            archive_extractor,
            plugin_store_client,
        }
    }

    pub fn download_repo(&self) -> &dyn DownloadRepository {
        self.download_repo.as_ref()
    }

    pub fn download_engine(&self) -> &dyn DownloadEngine {
        self.download_engine.as_ref()
    }

    pub fn event_bus(&self) -> &dyn EventBus {
        self.event_bus.as_ref()
    }

    pub fn file_storage(&self) -> &dyn FileStorage {
        self.file_storage.as_ref()
    }

    pub fn http_client(&self) -> &dyn HttpClient {
        self.http_client.as_ref()
    }

    pub fn plugin_loader(&self) -> &dyn PluginLoader {
        self.plugin_loader.as_ref()
    }

    pub fn config_store(&self) -> &dyn ConfigStore {
        self.config_store.as_ref()
    }

    pub fn credential_store(&self) -> &dyn CredentialStore {
        self.credential_store.as_ref()
    }

    pub fn clipboard_observer(&self) -> &dyn ClipboardObserver {
        self.clipboard_observer.as_ref()
    }

    pub(crate) fn plugin_loader_arc(&self) -> Arc<dyn PluginLoader> {
        Arc::clone(&self.plugin_loader)
    }

    pub fn archive_extractor(&self) -> &dyn ArchiveExtractor {
        self.archive_extractor.as_ref()
    }

    pub(crate) fn archive_extractor_arc(&self) -> Arc<dyn ArchiveExtractor> {
        Arc::clone(&self.archive_extractor)
    }

    pub fn plugin_store_client(&self) -> Option<&dyn PluginStoreClient> {
        self.plugin_store_client.as_deref()
    }

    pub(crate) fn plugin_store_client_arc(&self) -> Option<Arc<dyn PluginStoreClient>> {
        self.plugin_store_client.clone()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Mutex;

    use super::*;
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::config::{AppConfig, ConfigPatch};
    use crate::domain::model::credential::Credential;
    use crate::domain::model::download::{Download, DownloadId, DownloadState};
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
        started: Mutex<Vec<DownloadId>>,
    }

    impl MockDownloadEngine {
        fn new() -> Self {
            Self {
                started: Mutex::new(Vec::new()),
            }
        }
    }

    impl DownloadEngine for MockDownloadEngine {
        fn start(&self, download: &Download) -> Result<(), DomainError> {
            self.started.lock().unwrap().push(download.id());
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

    struct MockFileStorage {
        files: Mutex<HashMap<String, Vec<u8>>>,
        metas: Mutex<HashMap<String, DownloadMeta>>,
    }

    impl MockFileStorage {
        fn new() -> Self {
            Self {
                files: Mutex::new(HashMap::new()),
                metas: Mutex::new(HashMap::new()),
            }
        }
    }

    impl FileStorage for MockFileStorage {
        fn create_file(&self, path: &Path, size: u64) -> Result<(), DomainError> {
            self.files.lock().unwrap().insert(
                path.to_string_lossy().into_owned(),
                vec![0u8; size as usize],
            );
            Ok(())
        }

        fn write_segment(&self, path: &Path, offset: u64, data: &[u8]) -> Result<(), DomainError> {
            let key = path.to_string_lossy().into_owned();
            let mut files = self.files.lock().unwrap();
            if let Some(file) = files.get_mut(&key) {
                let start = offset as usize;
                let end = start + data.len();
                if end <= file.len() {
                    file[start..end].copy_from_slice(data);
                }
            }
            Ok(())
        }

        fn read_meta(&self, path: &Path) -> Result<Option<DownloadMeta>, DomainError> {
            Ok(self
                .metas
                .lock()
                .unwrap()
                .get(&path.to_string_lossy().into_owned())
                .cloned())
        }

        fn write_meta(&self, path: &Path, meta: &DownloadMeta) -> Result<(), DomainError> {
            self.metas
                .lock()
                .unwrap()
                .insert(path.to_string_lossy().into_owned(), meta.clone());
            Ok(())
        }

        fn delete_meta(&self, path: &Path) -> Result<(), DomainError> {
            self.metas
                .lock()
                .unwrap()
                .remove(&path.to_string_lossy().into_owned());
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
        plugins: Mutex<HashMap<String, PluginInfo>>,
    }

    impl MockPluginLoader {
        fn new() -> Self {
            Self {
                plugins: Mutex::new(HashMap::new()),
            }
        }
    }

    impl PluginLoader for MockPluginLoader {
        fn load(&self, manifest: &PluginManifest) -> Result<(), DomainError> {
            let info = manifest.info().clone();
            self.plugins
                .lock()
                .unwrap()
                .insert(info.name().to_string(), info);
            Ok(())
        }

        fn unload(&self, name: &str) -> Result<(), DomainError> {
            self.plugins.lock().unwrap().remove(name);
            Ok(())
        }

        fn resolve_url(&self, _url: &str) -> Result<Option<PluginInfo>, DomainError> {
            Ok(None)
        }

        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
            Ok(self.plugins.lock().unwrap().values().cloned().collect())
        }

        fn set_enabled(&self, _name: &str, _enabled: bool) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct MockConfigStore {
        config: Mutex<AppConfig>,
    }

    impl MockConfigStore {
        fn new() -> Self {
            Self {
                config: Mutex::new(AppConfig::default()),
            }
        }
    }

    impl ConfigStore for MockConfigStore {
        fn get_config(&self) -> Result<AppConfig, DomainError> {
            Ok(self.config.lock().unwrap().clone())
        }

        fn update_config(&self, patch: ConfigPatch) -> Result<AppConfig, DomainError> {
            let mut config = self.config.lock().unwrap();
            crate::domain::model::config::apply_patch(&mut config, &patch);
            Ok(config.clone())
        }
    }

    struct MockCredentialStore {
        store: Mutex<HashMap<String, Credential>>,
    }

    impl MockCredentialStore {
        fn new() -> Self {
            Self {
                store: Mutex::new(HashMap::new()),
            }
        }
    }

    impl CredentialStore for MockCredentialStore {
        fn get(&self, service: &str) -> Result<Option<Credential>, DomainError> {
            Ok(self.store.lock().unwrap().get(service).cloned())
        }

        fn store(&self, service: &str, credential: &Credential) -> Result<(), DomainError> {
            self.store
                .lock()
                .unwrap()
                .insert(service.to_string(), credential.clone());
            Ok(())
        }

        fn delete(&self, service: &str) -> Result<(), DomainError> {
            self.store.lock().unwrap().remove(service);
            Ok(())
        }
    }

    struct MockClipboardObserver {
        running: Mutex<bool>,
    }

    impl MockClipboardObserver {
        fn new() -> Self {
            Self {
                running: Mutex::new(false),
            }
        }
    }

    impl ClipboardObserver for MockClipboardObserver {
        fn start(&self) -> Result<(), DomainError> {
            *self.running.lock().unwrap() = true;
            Ok(())
        }

        fn stop(&self) -> Result<(), DomainError> {
            *self.running.lock().unwrap() = false;
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

    fn make_command_bus() -> CommandBus {
        CommandBus::new(
            Arc::new(MockDownloadRepo::new()),
            Arc::new(MockDownloadEngine::new()),
            Arc::new(MockEventBus::new()),
            Arc::new(MockFileStorage::new()),
            Arc::new(MockHttpClient),
            Arc::new(MockPluginLoader::new()),
            Arc::new(MockConfigStore::new()),
            Arc::new(MockCredentialStore::new()),
            Arc::new(MockClipboardObserver::new()),
            Arc::new(FakeArchiveExtractor),
            None,
        )
    }

    #[test]
    fn test_command_bus_new_compiles() {
        let _bus = make_command_bus();
    }

    #[test]
    fn test_command_bus_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CommandBus>();
    }
}
