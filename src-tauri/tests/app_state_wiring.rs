//! Integration test verifying that all adapters can be constructed and wired
//! into CommandBus + QueryBus without a running Tauri app.

use std::sync::Arc;

use vortex_lib::domain::ports::driven::{
    ArchiveExtractor, ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine,
    DownloadReadRepository, DownloadRepository, EventBus, FileStorage, HistoryRepository,
    HttpClient, PluginLoader, PluginReadRepository, StatsRepository,
};
use vortex_lib::{
    CommandBus, ExtismPluginLoader, ExtractionConfig, FsFileStorage, InMemoryStatsRepository,
    NoopCredentialStore, QueryBus, ReqwestHttpClient, SegmentedDownloadEngine, SharedHostResources,
    SqliteDownloadReadRepo, SqliteDownloadRepo, SqliteHistoryRepo, TokioEventBus, TomlConfigStore,
    VortexArchiveExtractor, connection,
};

/// Verifies that all driven adapters satisfy their port traits and that
/// CommandBus + QueryBus can be constructed end-to-end.
///
/// Uses a sync test with an explicit runtime to avoid "Cannot drop a runtime
/// in a context where blocking is not allowed" panics from adapters that
/// internally use `block_on`.
#[test]
fn test_appstate_wiring_with_in_memory_db() {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let _guard = rt.enter();

    // Database (only async step)
    let db = rt
        .block_on(connection::setup_test_db())
        .expect("in-memory DB setup");

    // Driven adapters
    let event_bus: Arc<dyn EventBus> = Arc::new(TokioEventBus::new(64));
    let file_storage: Arc<dyn FileStorage> = Arc::new(FsFileStorage::new());
    let reqwest_client = reqwest::Client::builder()
        .user_agent("Vortex-Test/0.1")
        .build()
        .expect("reqwest client");
    let http_client: Arc<dyn HttpClient> =
        Arc::new(ReqwestHttpClient::with_client(reqwest_client.clone()));
    let config_dir = tempfile::tempdir().expect("temp dir");
    let config_store: Arc<dyn ConfigStore> =
        Arc::new(TomlConfigStore::new(config_dir.path().join("config.toml")));
    let credential_store: Arc<dyn CredentialStore> = Arc::new(NoopCredentialStore);
    let archive_extractor: Arc<dyn ArchiveExtractor> =
        Arc::new(VortexArchiveExtractor::new(ExtractionConfig::default()));

    // SQLite repos
    let download_repo: Arc<dyn DownloadRepository> = Arc::new(SqliteDownloadRepo::new(db.clone()));
    let download_read_repo: Arc<dyn DownloadReadRepository> =
        Arc::new(SqliteDownloadReadRepo::new(db.clone()));
    let history_repo: Arc<dyn HistoryRepository> = Arc::new(SqliteHistoryRepo::new(db));
    let stats_repo: Arc<dyn StatsRepository> = Arc::new(InMemoryStatsRepository::new());

    // Plugin system
    let plugins_dir = tempfile::tempdir().expect("plugins dir");
    let shared_resources = Arc::new(SharedHostResources::new());
    let plugin_loader_impl = Arc::new(
        ExtismPluginLoader::new(plugins_dir.path().to_path_buf(), shared_resources)
            .expect("plugin loader"),
    );
    let plugin_read_repo: Arc<dyn PluginReadRepository> = plugin_loader_impl.registry().clone();
    let plugin_loader: Arc<dyn PluginLoader> = plugin_loader_impl;

    // Download engine
    let download_engine: Arc<dyn DownloadEngine> = Arc::new(SegmentedDownloadEngine::new(
        reqwest_client,
        file_storage.clone(),
        event_bus.clone(),
        4,
    ));

    // Clipboard stub (no Tauri AppHandle in tests)
    let clipboard_observer: Arc<dyn ClipboardObserver> = Arc::new(StubClipboardObserver);

    // CQRS buses
    let command_bus = Arc::new(CommandBus::new(
        download_repo,
        download_engine,
        event_bus,
        file_storage,
        http_client,
        plugin_loader,
        config_store,
        credential_store,
        clipboard_observer,
        archive_extractor.clone(),
    ));

    let query_bus = Arc::new(QueryBus::new(
        download_read_repo,
        history_repo,
        stats_repo,
        plugin_read_repo,
        archive_extractor,
    ));

    // Verify command bus is wired (exercise a read through it)
    let _config = command_bus
        .config_store()
        .get_config()
        .expect("config load");

    // Verify query bus can execute a read query (empty DB → empty results)
    let downloads = query_bus
        .download_read_repo()
        .find_downloads(None, None, Some(10), None)
        .expect("download list query");
    assert!(downloads.is_empty());

    // Verify stats repo returns zero-state
    let stats = query_bus.stats_repo().get_stats().expect("stats query");
    assert_eq!(stats.total_files, 0);
    assert_eq!(stats.total_downloaded_bytes, 0);
}

/// Minimal clipboard observer stub for tests without a Tauri runtime.
struct StubClipboardObserver;

impl ClipboardObserver for StubClipboardObserver {
    fn start(&self) -> Result<(), vortex_lib::domain::error::DomainError> {
        Ok(())
    }

    fn stop(&self) -> Result<(), vortex_lib::domain::error::DomainError> {
        Ok(())
    }

    fn get_urls(&self) -> Result<Vec<String>, vortex_lib::domain::error::DomainError> {
        Ok(vec![])
    }
}
