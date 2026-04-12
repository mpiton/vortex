mod adapters;
mod application;
pub mod domain;

use std::sync::Arc;

use tauri::Manager;

use domain::ports::driven::{
    ArchiveExtractor, ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine,
    DownloadReadRepository, DownloadRepository, EventBus, FileStorage, HistoryRepository,
    HttpClient, PluginLoader, PluginReadRepository, StatsRepository,
};

// Public API — concrete types for app wiring (main.rs, Tauri setup, integration tests)
pub use adapters::driven::clipboard::TauriClipboardObserver;
pub use adapters::driven::config::TomlConfigStore;
pub use adapters::driven::credential::NoopCredentialStore;
pub use adapters::driven::event::TokioEventBus;
pub use adapters::driven::event::spawn_tauri_event_bridge;
pub use adapters::driven::extractor::VortexArchiveExtractor;
pub use adapters::driven::filesystem::FsFileStorage;
pub use adapters::driven::network::ReqwestHttpClient;
pub use adapters::driven::network::SegmentedDownloadEngine;
pub use adapters::driven::notification::spawn_notification_bridge;
pub use adapters::driven::plugin::builtin::HttpModule;
pub use adapters::driven::plugin::capabilities::SharedHostResources;
pub use adapters::driven::plugin::{ExtismPluginLoader, PluginRegistry, PluginWatcher};
pub use adapters::driven::sqlite::connection;
pub use adapters::driven::sqlite::download_read_repo::SqliteDownloadReadRepo;
pub use adapters::driven::sqlite::download_repo::SqliteDownloadRepo;
pub use adapters::driven::sqlite::history_repo::SqliteHistoryRepo;
pub use adapters::driven::stats::InMemoryStatsRepository;
pub use adapters::driven::tray::setup_system_tray;
pub use application::command_bus::CommandBus;
pub use application::error::AppError;
pub use application::query_bus::QueryBus;
pub use application::read_models::{
    download_detail_view::{DownloadDetailViewDto, SegmentViewDto},
    download_view::DownloadViewDto,
    history_view::HistoryViewDto,
    plugin_view::PluginViewDto,
    stats_view::{DailyVolumeDto, HostStatsDto, StatsViewDto},
};
pub use application::services::QueueManager;
pub use domain::model::ExtractionConfig;

pub use adapters::driving::tauri_ipc::{
    self, AppState, clipboard_state, clipboard_toggle, download_cancel, download_count_by_state,
    download_detail, download_list, download_pause, download_pause_all, download_remove,
    download_resume, download_resume_all, download_retry, download_set_priority, download_start,
    link_resolve, plugin_disable, plugin_enable, plugin_install, plugin_list, plugin_uninstall,
    settings_get, settings_update,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build());
    #[cfg(all(debug_assertions, unix))]
    {
        builder = builder.plugin(tauri_plugin_pilot::init());
    }
    builder
        .setup(|app| {
            let app_handle = app.handle().clone();

            // ── Paths ───────────────────────────────────────────────
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;
            let db_path = app_data_dir.join("vortex.db");
            let config_path = app_data_dir.join("config.toml");
            let plugins_dir = app_data_dir.join("plugins");
            std::fs::create_dir_all(&plugins_dir)?;

            // ── Database ────────────────────────────────────────────
            // block_on enters the Tauri tokio runtime for async DB setup.
            // We also capture the Handle so we can keep the runtime context
            // active for the rest of setup (needed by tokio::spawn in
            // event bus subscribers).
            let (db, rt_handle) = tauri::async_runtime::block_on(async {
                let handle = tokio::runtime::Handle::current();
                let db = connection::establish_connection(&db_path)
                    .await
                    .map_err(|e| e.to_string())?;
                connection::run_migrations(&db)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok::<_, String>((db, handle))
            })?;
            let _rt_guard = rt_handle.enter();

            // ── Driven adapters ─────────────────────────────────────
            let event_bus: Arc<dyn EventBus> = Arc::new(TokioEventBus::new(256));
            let file_storage: Arc<dyn FileStorage> = Arc::new(FsFileStorage::new());
            let reqwest_client = reqwest::Client::builder()
                .user_agent("Vortex/0.1")
                .connect_timeout(std::time::Duration::from_secs(30))
                .build()
                .map_err(|e| e.to_string())?;
            let http_client: Arc<dyn HttpClient> =
                Arc::new(ReqwestHttpClient::with_client(reqwest_client.clone()));
            let config_store: Arc<dyn ConfigStore> = Arc::new(TomlConfigStore::new(config_path));
            // TODO: replace with keyring-rs CredentialStore once implemented
            let credential_store: Arc<dyn CredentialStore> = Arc::new(NoopCredentialStore);
            let clipboard_observer: Arc<dyn ClipboardObserver> =
                Arc::new(TauriClipboardObserver::new(app_handle.clone()));
            let archive_extractor: Arc<dyn ArchiveExtractor> =
                Arc::new(VortexArchiveExtractor::new(ExtractionConfig::default()));

            // ── SQLite repositories ─────────────────────────────────
            let download_repo: Arc<dyn DownloadRepository> =
                Arc::new(SqliteDownloadRepo::new(db.clone()));
            let download_read_repo: Arc<dyn DownloadReadRepository> =
                Arc::new(SqliteDownloadReadRepo::new(db.clone()));
            let history_repo: Arc<dyn HistoryRepository> = Arc::new(SqliteHistoryRepo::new(db));
            // TODO: replace with SqliteStatsRepo once implemented
            let stats_repo: Arc<dyn StatsRepository> = Arc::new(InMemoryStatsRepository::new());

            // ── Plugin system ───────────────────────────────────────
            let shared_resources = Arc::new(SharedHostResources::new());
            let plugin_loader_impl = Arc::new(
                ExtismPluginLoader::new(plugins_dir.clone(), shared_resources)
                    .map_err(|e| e.to_string())?,
            );
            let plugin_read_repo: Arc<dyn PluginReadRepository> =
                plugin_loader_impl.registry().clone();
            let plugin_loader: Arc<dyn PluginLoader> = plugin_loader_impl.clone();

            // ── Download engine ─────────────────────────────────────
            let download_engine: Arc<dyn DownloadEngine> = Arc::new(SegmentedDownloadEngine::new(
                reqwest_client,
                file_storage.clone(),
                event_bus.clone(),
                4,
            ));

            // ── Queue manager ──────────────────────────────────────
            // Listens to domain events and auto-schedules queued downloads.
            let queue_manager = Arc::new(QueueManager::new(
                download_repo.clone(),
                download_engine.clone(),
                event_bus.clone(),
                4, // TODO: read max_concurrent from config
            ));

            // ── CQRS buses ──────────────────────────────────────────
            let command_bus = Arc::new(CommandBus::new(
                download_repo,
                download_engine,
                event_bus.clone(),
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

            // ── Register AppState ───────────────────────────────────
            app.manage(AppState {
                command_bus,
                query_bus,
            });

            // ── System tray ─────────────────────────────────────────
            if let Err(e) = setup_system_tray(app, false) {
                tracing::error!("Failed to setup system tray: {e}");
            }

            // ── Event bridges (domain events → frontend + desktop) ──
            spawn_tauri_event_bridge(app_handle.clone(), event_bus.as_ref());
            spawn_notification_bridge(app_handle, event_bus.as_ref());

            // ── Queue manager event listener ────────────────────────
            queue_manager.clone().start_listening();
            app.manage(queue_manager);

            // ── Plugin hot-reload watcher ────────────────────────────
            // Kept alive by moving into a managed resource; dropped on app exit.
            match PluginWatcher::start(plugins_dir, plugin_loader_impl) {
                Ok(watcher) => {
                    app.manage(watcher);
                }
                Err(e) => {
                    tracing::warn!("Plugin watcher failed to start: {e}");
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            download_start,
            download_pause,
            download_resume,
            download_cancel,
            download_retry,
            download_pause_all,
            download_resume_all,
            download_set_priority,
            download_remove,
            download_list,
            download_detail,
            download_count_by_state,
            plugin_install,
            plugin_uninstall,
            plugin_enable,
            plugin_disable,
            plugin_list,
            link_resolve,
            clipboard_toggle,
            clipboard_state,
            settings_get,
            settings_update,
        ])
        .run(tauri::generate_context!())
        // Tauri's run() has no meaningful recovery path — panic is intentional here
        .expect("fatal: failed to start Vortex");
}
