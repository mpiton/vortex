//! CQRS command bus — dispatches commands to their handlers.
//!
//! Holds references to all driven ports needed by command handlers.
//! Actual handler implementations will be added in tasks 11-12.

use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::application::services::{AccountRotator, AccountSelector};
use crate::domain::model::config::{
    DEFAULT_LINK_CHECK_PARALLELISM, normalize_link_check_parallelism,
};
use crate::domain::ports::driven::{
    AccountCredentialStore, AccountRepository, AccountValidator, ArchiveExtractor,
    ChecksumComputer, ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine,
    DownloadRepository, EventBus, FileOpener, FileStorage, HistoryRepository, HttpClient,
    PackageRepository, PassphraseCodec, PluginConfigStore, PluginLoader, PluginStoreClient,
    UrlOpener,
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
    history_repo: Arc<dyn HistoryRepository>,
    plugin_store_client: Option<Arc<dyn PluginStoreClient>>,
    checksum_computer: Option<Arc<dyn ChecksumComputer>>,
    file_opener: Option<Arc<dyn FileOpener>>,
    url_opener: Option<Arc<dyn UrlOpener>>,
    plugin_config_store: Option<Arc<dyn PluginConfigStore>>,
    account_repo: Option<Arc<dyn AccountRepository>>,
    account_credential_store: Option<Arc<dyn AccountCredentialStore>>,
    package_repo: Option<Arc<dyn PackageRepository>>,
    account_validator: Option<Arc<dyn AccountValidator>>,
    account_selector: Option<Arc<AccountSelector>>,
    account_rotator: Option<Arc<AccountRotator>>,
    passphrase_codec: Option<Arc<dyn PassphraseCodec>>,
    /// Serializes queue-position allocation across handlers. Without this,
    /// two concurrent move-to-top/move-to-bottom/start-download calls can
    /// observe the same min/max and write colliding `queue_position`
    /// values, breaking deterministic ordering.
    queue_position_lock: tokio::sync::Mutex<()>,
    /// Shared semaphore that enforces `AppConfig::link_check_parallelism`
    /// globally across overlapping `handle_check_online` invocations.
    /// Without a shared instance, two batches launched back-to-back
    /// (e.g. paste followed by per-row retry) would each create their
    /// own per-call semaphore and the aggregate in-flight HEAD count
    /// could reach 2× the configured cap. Capacity is fixed at
    /// construction time from the persisted config; runtime changes to
    /// `link_check_parallelism` take effect on next app start.
    link_check_semaphore: Arc<Semaphore>,
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
        history_repo: Arc<dyn HistoryRepository>,
        plugin_store_client: Option<Arc<dyn PluginStoreClient>>,
    ) -> Self {
        // Seed the shared link-check semaphore from the persisted
        // config so the global concurrency cap matches the value the
        // user picked. Falls back to the PRD default when the store
        // can't be read (boot path with corrupt config) — preferable
        // to panicking and matches the runtime behaviour of
        // `normalize_link_check_parallelism`.
        let initial_parallelism = config_store
            .get_config()
            .ok()
            .map(|c| normalize_link_check_parallelism(c.link_check_parallelism))
            .unwrap_or(DEFAULT_LINK_CHECK_PARALLELISM as usize);
        let link_check_semaphore = Arc::new(Semaphore::new(initial_parallelism));
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
            history_repo,
            plugin_store_client,
            checksum_computer: None,
            file_opener: None,
            url_opener: None,
            plugin_config_store: None,
            account_repo: None,
            account_credential_store: None,
            package_repo: None,
            account_validator: None,
            account_selector: None,
            account_rotator: None,
            passphrase_codec: None,
            queue_position_lock: tokio::sync::Mutex::new(()),
            link_check_semaphore,
        }
    }

    /// Builder-style setter for the account repository. Optional so
    /// existing fixtures that never invoke account commands don't have
    /// to provide a mock.
    pub fn with_account_repo(mut self, repo: Arc<dyn AccountRepository>) -> Self {
        self.account_repo = Some(repo);
        self
    }

    /// Builder-style setter for the per-account keyring wrapper.
    pub fn with_account_credential_store(mut self, store: Arc<dyn AccountCredentialStore>) -> Self {
        self.account_credential_store = Some(store);
        self
    }

    /// Builder-style setter for the package write repository. Optional
    /// so test fixtures that never invoke package commands don't have
    /// to provide a mock.
    pub fn with_package_repo(mut self, repo: Arc<dyn PackageRepository>) -> Self {
        self.package_repo = Some(repo);
        self
    }

    pub fn package_repo(&self) -> Option<&dyn PackageRepository> {
        self.package_repo.as_deref()
    }

    pub(crate) fn package_repo_arc(&self) -> Option<Arc<dyn PackageRepository>> {
        self.package_repo.clone()
    }

    /// Builder-style setter for the account-validation port (delegates
    /// to the matching hoster / debrid plugin).
    pub fn with_account_validator(mut self, validator: Arc<dyn AccountValidator>) -> Self {
        self.account_validator = Some(validator);
        self
    }

    /// Builder-style setter for the auto-selecting account dispatcher.
    /// Optional so tests that don't exercise the dispatcher can omit it.
    pub fn with_account_selector(mut self, selector: Arc<AccountSelector>) -> Self {
        self.account_selector = Some(selector);
        self
    }

    /// Builder-style setter for the quota-aware [`AccountRotator`].
    /// Optional — when omitted, callers fall back to the plain
    /// `account_selector()` (no rotation, no exhaustion tracking).
    pub fn with_account_rotator(mut self, rotator: Arc<AccountRotator>) -> Self {
        self.account_rotator = Some(rotator);
        self
    }

    /// Builder-style setter for the passphrase codec used by the
    /// import / export commands.
    pub fn with_passphrase_codec(mut self, codec: Arc<dyn PassphraseCodec>) -> Self {
        self.passphrase_codec = Some(codec);
        self
    }

    pub fn account_repo(&self) -> Option<&dyn AccountRepository> {
        self.account_repo.as_deref()
    }

    pub fn account_credential_store(&self) -> Option<&dyn AccountCredentialStore> {
        self.account_credential_store.as_deref()
    }

    pub fn account_validator(&self) -> Option<&dyn AccountValidator> {
        self.account_validator.as_deref()
    }

    pub fn account_selector(&self) -> Option<&AccountSelector> {
        self.account_selector.as_deref()
    }

    pub fn account_rotator(&self) -> Option<&AccountRotator> {
        self.account_rotator.as_deref()
    }

    pub fn passphrase_codec(&self) -> Option<&dyn PassphraseCodec> {
        self.passphrase_codec.as_deref()
    }

    /// Builder-style setter for the plugin configuration persistence port.
    /// Optional so existing test fixtures don't have to construct one when
    /// they don't exercise the plugin-config commands.
    pub fn with_plugin_config_store(mut self, store: Arc<dyn PluginConfigStore>) -> Self {
        self.plugin_config_store = Some(store);
        self
    }

    pub fn plugin_config_store(&self) -> Option<&dyn PluginConfigStore> {
        self.plugin_config_store.as_deref()
    }

    /// Acquire the application-wide lock that serializes queue-position
    /// allocation. Held by handlers that read the current min/max and
    /// then persist a new `queue_position`, so the read+write is atomic
    /// with respect to other queue mutations.
    pub(crate) async fn lock_queue_positions(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.queue_position_lock.lock().await
    }

    /// Shared semaphore that bounds in-flight `link_check_online`
    /// probes globally across overlapping invocations.
    pub(crate) fn link_check_semaphore(&self) -> Arc<Semaphore> {
        Arc::clone(&self.link_check_semaphore)
    }

    /// Builder-style setter for the checksum computer port. Kept optional so
    /// existing test fixtures don't have to construct one when they don't
    /// exercise the verify-checksum path.
    pub fn with_checksum_computer(mut self, computer: Arc<dyn ChecksumComputer>) -> Self {
        self.checksum_computer = Some(computer);
        self
    }

    /// Builder-style setter for the file-opener port. Optional so existing
    /// test fixtures that never invoke the open-file/open-folder handlers
    /// don't have to provide a mock.
    pub fn with_file_opener(mut self, opener: Arc<dyn FileOpener>) -> Self {
        self.file_opener = Some(opener);
        self
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

    pub(crate) fn http_client_arc(&self) -> Arc<dyn HttpClient> {
        Arc::clone(&self.http_client)
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

    pub fn history_repo(&self) -> &dyn HistoryRepository {
        self.history_repo.as_ref()
    }

    pub(crate) fn download_repo_arc(&self) -> Arc<dyn DownloadRepository> {
        Arc::clone(&self.download_repo)
    }

    pub(crate) fn event_bus_arc(&self) -> Arc<dyn EventBus> {
        Arc::clone(&self.event_bus)
    }

    pub(crate) fn checksum_computer_arc(&self) -> Option<Arc<dyn ChecksumComputer>> {
        self.checksum_computer.clone()
    }

    pub fn file_opener(&self) -> Option<&dyn FileOpener> {
        self.file_opener.as_deref()
    }

    pub(crate) fn file_opener_arc(&self) -> Option<Arc<dyn FileOpener>> {
        self.file_opener.clone()
    }

    /// Builder-style setter for the URL-opener port. Optional so existing
    /// test fixtures that never invoke the report-broken-plugin handler
    /// don't have to provide a mock.
    pub fn with_url_opener(mut self, opener: Arc<dyn UrlOpener>) -> Self {
        self.url_opener = Some(opener);
        self
    }

    pub fn url_opener(&self) -> Option<&dyn UrlOpener> {
        self.url_opener.as_deref()
    }

    pub(crate) fn url_opener_arc(&self) -> Option<Arc<dyn UrlOpener>> {
        self.url_opener.clone()
    }

    /// Convenience entry-point for the link-grabber and download flows.
    ///
    /// When an `AccountSelector` is wired AND the configured strategy is
    /// honoured, returns the chosen account for `service_name`. When no
    /// selector is wired (e.g. test fixtures) returns `Ok(None)`. PRD
    /// §6.4 — the strategy is read from the live `AppConfig` at every
    /// call so a runtime change to `account_selection_strategy` is
    /// honoured without restart.
    pub fn resolve_account_for(
        &self,
        service_name: &str,
    ) -> Result<Option<crate::domain::model::account::Account>, crate::application::error::AppError>
    {
        let selector = match self.account_selector() {
            Some(s) => s,
            None => return Ok(None),
        };
        let strategy = self.config_store.get_config()?.account_selection_strategy;
        selector.select_best(service_name, strategy)
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

        fn extract_links(&self, _url: &str) -> Result<String, DomainError> {
            Err(DomainError::NotFound(
                "extract_links not mocked".to_string(),
            ))
        }

        fn get_media_variants(&self, _url: &str) -> Result<String, DomainError> {
            Err(DomainError::NotFound(
                "get_media_variants not mocked".to_string(),
            ))
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
            Arc::new(crate::application::test_support::NoopHistoryRepo),
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

    /// `resolve_account_for` must propagate `ConfigStore::get_config()`
    /// failures instead of silently falling back to the default strategy.
    /// A corrupt or unreadable config previously forced `BestTraffic`
    /// even when the user had picked `RoundRobin` / `Manual`. The fix:
    /// surface the error to the caller via `?`.
    #[test]
    fn test_resolve_account_for_propagates_config_store_failure() {
        use crate::application::services::AccountSelector;
        use crate::domain::model::account::{Account, AccountId, AccountSelectionStrategy};
        use crate::domain::ports::driven::AccountRepository;
        use crate::domain::ports::driven::clock::Clock;
        use crate::domain::ports::driven::event_bus::EventBus;

        // Bus + clock + repo stand-ins — none of them are exercised
        // because the failing config store short-circuits the call.
        struct StubBus;
        impl EventBus for StubBus {
            fn publish(&self, _: DomainEvent) {}
            fn subscribe(&self, _: Box<dyn Fn(&DomainEvent) + Send + Sync + 'static>) {}
        }
        struct ZeroClock;
        impl Clock for ZeroClock {
            fn now_unix_secs(&self) -> u64 {
                0
            }
        }
        struct EmptyRepo;
        impl AccountRepository for EmptyRepo {
            fn find_by_id(&self, _: &AccountId) -> Result<Option<Account>, DomainError> {
                Ok(None)
            }
            fn save(&self, _: &Account) -> Result<(), DomainError> {
                Ok(())
            }
            fn list(&self) -> Result<Vec<Account>, DomainError> {
                Ok(vec![])
            }
            fn list_by_service(&self, _: &str) -> Result<Vec<Account>, DomainError> {
                Ok(vec![])
            }
            fn delete(&self, _: &AccountId) -> Result<(), DomainError> {
                Ok(())
            }
        }

        struct FailingConfigStore;
        impl ConfigStore for FailingConfigStore {
            fn get_config(&self) -> Result<AppConfig, DomainError> {
                Err(DomainError::ValidationError("config corrupted".into()))
            }
            fn update_config(&self, _: ConfigPatch) -> Result<AppConfig, DomainError> {
                Err(DomainError::ValidationError("config corrupted".into()))
            }
        }

        let bus: Arc<dyn EventBus> = Arc::new(StubBus);
        let clock: Arc<dyn Clock> = Arc::new(ZeroClock);
        let repo: Arc<dyn AccountRepository> = Arc::new(EmptyRepo);
        let selector = AccountSelector::new(repo, bus, clock);

        let command_bus = CommandBus::new(
            Arc::new(MockDownloadRepo::new()),
            Arc::new(MockDownloadEngine::new()),
            Arc::new(MockEventBus::new()),
            Arc::new(MockFileStorage::new()),
            Arc::new(MockHttpClient),
            Arc::new(MockPluginLoader::new()),
            Arc::new(FailingConfigStore),
            Arc::new(MockCredentialStore::new()),
            Arc::new(MockClipboardObserver::new()),
            Arc::new(FakeArchiveExtractor),
            Arc::new(crate::application::test_support::NoopHistoryRepo),
            None,
        )
        .with_account_selector(selector);

        // `_` keeps the strategy enum in scope for the contract proof —
        // even when the user picked anything, the failing store must
        // bubble up before the strategy is read.
        let _ = AccountSelectionStrategy::RoundRobin;

        let err = command_bus
            .resolve_account_for("Uploaded")
            .expect_err("config-store failure must propagate");
        assert!(matches!(
            err,
            crate::application::error::AppError::Domain(DomainError::ValidationError(_))
        ));
    }
}
