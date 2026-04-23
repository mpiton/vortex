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
pub use adapters::driven::credential::KeyringCredentialStore;
pub use adapters::driven::credential::NoopCredentialStore;
pub use adapters::driven::event::TokioEventBus;
pub use adapters::driven::event::spawn_tauri_event_bridge;
pub use adapters::driven::extractor::VortexArchiveExtractor;
pub use adapters::driven::filesystem::{FsFileStorage, resolve_system_download_dir};
pub use adapters::driven::logging::download_log_bridge::spawn_download_log_bridge;
pub use adapters::driven::logging::download_log_store::DownloadLogStore;
pub use adapters::driven::network::ReqwestHttpClient;
pub use adapters::driven::network::SegmentedDownloadEngine;
pub use adapters::driven::notification::spawn_notification_bridge;
pub use adapters::driven::plugin::builtin::HttpModule;
pub use adapters::driven::plugin::capabilities::SharedHostResources;
pub use adapters::driven::plugin::{
    ExtismPluginLoader, GithubStoreClient, PluginRegistry, PluginWatcher,
};
pub use adapters::driven::sqlite::connection;
pub use adapters::driven::sqlite::download_read_repo::SqliteDownloadReadRepo;
pub use adapters::driven::sqlite::download_repo::SqliteDownloadRepo;
pub use adapters::driven::sqlite::history_repo::SqliteHistoryRepo;
pub use adapters::driven::sqlite::progress_bridge::spawn_sqlite_progress_bridge;
pub use adapters::driven::sqlite::stats_repo::SqliteStatsRepo;
pub use adapters::driven::tray::setup_system_tray;
pub use application::command_bus::CommandBus;
pub use application::commands::store_refresh::{read_cache, write_cache};
pub use application::error::AppError;
pub use application::query_bus::QueryBus;
pub use application::read_models::{
    download_detail_view::{DownloadDetailViewDto, SegmentViewDto},
    download_view::DownloadViewDto,
    history_view::HistoryViewDto,
    plugin_view::PluginViewDto,
    stats_view::{DailyVolumeDto, HostStatsDto, ModuleStatsDto, StatsViewDto},
};
pub use application::services::QueueManager;
pub use domain::model::ExtractionConfig;

pub use adapters::driving::tauri_ipc::{
    self, AppState, clipboard_state, clipboard_toggle, command_get_media_metadata, download_cancel,
    download_clear_completed, download_clear_failed, download_count_by_state, download_detail,
    download_list, download_logs, download_media_start, download_pause, download_pause_all,
    download_remove, download_resume, download_resume_all, download_retry, download_set_priority,
    download_start, history_clear, history_delete_entry, history_export, history_get_by_id,
    history_list, history_purge_older_than, history_search, link_resolve, plugin_disable,
    plugin_enable, plugin_install, plugin_list, plugin_store_install, plugin_store_list,
    plugin_store_refresh, plugin_store_update, plugin_uninstall, reveal_in_folder, settings_get,
    settings_update, stats_get, stats_top_modules, status_bar_get,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
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
            let config_store: Arc<dyn ConfigStore> = Arc::new(TomlConfigStore::new(
                config_path,
                resolve_system_download_dir(),
                Some(uuid::Uuid::new_v4().to_string()),
            ));
            let credential_store: Arc<dyn CredentialStore> = Arc::new(KeyringCredentialStore);
            let clipboard_observer: Arc<dyn ClipboardObserver> =
                Arc::new(TauriClipboardObserver::new(app_handle.clone()));
            let archive_extractor: Arc<dyn ArchiveExtractor> =
                Arc::new(VortexArchiveExtractor::new(ExtractionConfig::default()));
            let download_log_store = Arc::new(DownloadLogStore::new(256));

            // ── SQLite repositories ─────────────────────────────────
            let download_repo: Arc<dyn DownloadRepository> =
                Arc::new(SqliteDownloadRepo::new(db.clone()));
            let download_read_repo: Arc<dyn DownloadReadRepository> =
                Arc::new(SqliteDownloadReadRepo::new(db.clone()));
            let history_repo: Arc<dyn HistoryRepository> =
                Arc::new(SqliteHistoryRepo::new(db.clone()));
            let stats_repo: Arc<dyn StatsRepository> = Arc::new(SqliteStatsRepo::new(db.clone()));

            // ── Plugin system ───────────────────────────────────────
            let shared_resources = Arc::new(SharedHostResources::new());
            let plugin_loader_impl = Arc::new(
                ExtismPluginLoader::new(plugins_dir.clone(), shared_resources)
                    .map_err(|e| e.to_string())?,
            );

            // Scan existing plugin directories and load them at startup.
            // The PluginWatcher reacts only to file-system events, so plugins
            // already present on disk before the watcher starts would otherwise
            // be silently skipped until a file-change event arrives.
            if let Ok(entries) = std::fs::read_dir(&plugins_dir) {
                for entry in entries.flatten() {
                    let dir = entry.path();
                    if dir.is_dir() {
                        match adapters::driven::plugin::manifest::parse_manifest(&dir) {
                            Ok((manifest, _)) => {
                                let name = manifest.info().name();
                                match plugin_loader_impl.load(&manifest) {
                                    Ok(()) => tracing::info!("startup: loaded plugin '{name}'"),
                                    Err(e) => tracing::warn!(
                                        "startup: failed to load plugin '{name}': {e}"
                                    ),
                                }
                            }
                            Err(e) => tracing::debug!("startup: skipping {}: {e}", dir.display()),
                        }
                    }
                }
            }

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

            // ── Startup recovery ────────────────────────────────────
            // Orphaned downloads (Downloading/Waiting/Checking/Extracting
            // in SQLite but no engine task) are marked Error so the user
            // can retry.  Runs early, before anything subscribes to events.
            match application::services::startup_recovery::recover_orphaned_downloads(
                download_repo.as_ref(),
            ) {
                Ok(0) => {}
                Ok(n) => tracing::info!("Recovered {n} orphaned download(s) from previous session"),
                Err(e) => tracing::error!("Startup recovery failed: {e}"),
            }

            // ── Queue manager ──────────────────────────────────────
            // Listens to domain events and auto-schedules queued downloads.
            // `max_concurrent` is seeded from the persisted config and then
            // kept in sync via the queue_config_bridge subscriber below.
            let initial_max_concurrent = match config_store.get_config() {
                Ok(cfg) => cfg.max_concurrent_downloads as usize,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "failed to read max_concurrent_downloads from config, falling back to default"
                    );
                    crate::domain::model::config::AppConfig::default().max_concurrent_downloads
                        as usize
                }
            };
            let queue_manager = Arc::new(QueueManager::new(
                download_repo.clone(),
                download_engine.clone(),
                event_bus.clone(),
                initial_max_concurrent,
            ));

            // Propagate future settings updates (UI → command bus) to the
            // running queue manager without requiring a restart.
            application::services::subscribe_queue_to_config(
                event_bus.as_ref(),
                config_store.clone(),
                queue_manager.clone(),
            );

            // ── Plugin store client ─────────────────────────────────
            let registry_url =
                "https://raw.githubusercontent.com/mpiton/vortex/main/registry/registry.toml";
            // Staging MUST live outside plugins_dir so the recursive plugin
            // watcher doesn't fire events during an in-progress install.
            // Those events used to race the install's own load_from_dir
            // call and could leave the plugin unloaded in memory even after
            // a successful install (toast said "updated", but the UI then
            // reported the plugin as not installed).
            let store_staging_dir = app_data_dir.join("plugin-staging");
            // One-shot cleanup of the legacy `.staging/` that used to live
            // inside `plugins_dir`. Harmless if already absent.
            let legacy_staging = plugins_dir.join(".staging");
            if legacy_staging.exists()
                && let Err(e) = std::fs::remove_dir_all(&legacy_staging)
            {
                tracing::warn!(
                    path = %legacy_staging.display(),
                    error = %e,
                    "failed to clean up legacy plugin staging dir",
                );
            }
            let store_client: Arc<dyn crate::domain::ports::driven::PluginStoreClient> =
                Arc::new(GithubStoreClient::new(registry_url, store_staging_dir));

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
                history_repo.clone(),
                Some(store_client),
            ));

            let query_bus = Arc::new(QueryBus::new(
                download_read_repo,
                history_repo,
                stats_repo,
                plugin_read_repo,
                archive_extractor,
            ));

            // ── Register AppState ───────────────────────────────────
            let app_plugin_loader: Arc<dyn PluginLoader> = plugin_loader_impl.clone();
            app.manage(AppState {
                command_bus,
                query_bus,
                download_log_store: download_log_store.clone(),
                plugin_loader: app_plugin_loader,
            });

            // ── System tray ─────────────────────────────────────────
            if let Err(e) = setup_system_tray(app, false) {
                tracing::error!("Failed to setup system tray: {e}");
            }

            // ── Event bridges (domain events → frontend + desktop) ──
            spawn_tauri_event_bridge(app_handle.clone(), event_bus.as_ref());
            spawn_notification_bridge(app_handle, event_bus.as_ref());
            spawn_download_log_bridge(event_bus.as_ref(), download_log_store);
            spawn_sqlite_progress_bridge(event_bus.as_ref(), db);

            // ── Queue manager event listener ────────────────────────
            queue_manager.clone().start_listening();

            // Re-schedule any Queued/Retry downloads that survived the
            // previous session (their engine tasks are gone).
            let qm_startup = queue_manager.clone();
            tokio::spawn(async move {
                if let Err(e) = qm_startup.on_slot_freed().await {
                    tracing::warn!("Startup scheduling failed: {e}");
                }
            });

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
            download_clear_completed,
            download_clear_failed,
            download_list,
            download_detail,
            download_logs,
            download_count_by_state,
            plugin_install,
            plugin_uninstall,
            plugin_enable,
            plugin_disable,
            plugin_list,
            plugin_store_list,
            plugin_store_refresh,
            plugin_store_install,
            plugin_store_update,
            link_resolve,
            clipboard_toggle,
            clipboard_state,
            settings_get,
            settings_update,
            status_bar_get,
            command_get_media_metadata,
            download_media_start,
            history_list,
            history_search,
            history_get_by_id,
            history_export,
            history_delete_entry,
            history_clear,
            history_purge_older_than,
            reveal_in_folder,
            stats_get,
            stats_top_modules,
        ])
        .run(tauri::generate_context!())
        // Tauri's run() has no meaningful recovery path — panic is intentional here
        .expect("fatal: failed to start Vortex");
}
