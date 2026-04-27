mod adapters;
mod application;
pub mod domain;

use std::sync::Arc;

use tauri::Manager;

use domain::ports::driven::{
    ArchiveExtractor, ClipboardObserver, Clock, ConfigStore, CredentialStore, DownloadEngine,
    DownloadReadRepository, DownloadRepository, EventBus, FileStorage, HistoryRepository,
    HttpClient, PluginLoader, PluginReadRepository, StatsRepository,
};

// Public API — concrete types for app wiring (main.rs, Tauri setup, integration tests)
pub use adapters::driven::clipboard::TauriClipboardObserver;
pub use adapters::driven::config::TomlConfigStore;
pub use adapters::driven::credential::KeyringCredentialStore;
pub use adapters::driven::credential::NoopCredentialStore;
pub use adapters::driven::event::TokioEventBus;
pub use adapters::driven::event::spawn_history_recorder_bridge;
pub use adapters::driven::event::spawn_stats_recorder_bridge;
pub use adapters::driven::event::spawn_tauri_event_bridge;
pub use adapters::driven::extractor::VortexArchiveExtractor;
pub use adapters::driven::filesystem::{
    FsFileStorage, SystemFileOpener, SystemUrlOpener, resolve_system_download_dir,
};
pub use adapters::driven::logging::download_log_bridge::spawn_download_log_bridge;
pub use adapters::driven::logging::download_log_store::{
    DEFAULT_MAX_ENTRIES_PER_DOWNLOAD, DownloadLogStore,
};
pub use adapters::driven::network::ReqwestHttpClient;
pub use adapters::driven::network::SegmentedDownloadEngine;
pub use adapters::driven::notification::spawn_notification_bridge;
pub use adapters::driven::plugin::builtin::HttpModule;
pub use adapters::driven::plugin::capabilities::SharedHostResources;
pub use adapters::driven::plugin::{
    ExtismPluginLoader, GithubStoreClient, PluginRegistry, PluginWatcher,
};
pub use adapters::driven::scheduler::{HISTORY_PURGE_STATE_FILE, HistoryPurgeWorker, SystemClock};
pub use adapters::driven::sqlite::connection;
pub use adapters::driven::sqlite::download_read_repo::SqliteDownloadReadRepo;
pub use adapters::driven::sqlite::download_repo::SqliteDownloadRepo;
pub use adapters::driven::sqlite::history_repo::SqliteHistoryRepo;
pub use adapters::driven::sqlite::progress_bridge::spawn_sqlite_progress_bridge;
pub use adapters::driven::sqlite::stats_repo::SqliteStatsRepo;
pub use adapters::driven::tray::{
    DEFAULT_FRAME_INTERVAL, IconSwapper, TauriIconSwapper, pulse_frames, setup_system_tray,
    spawn_tray_animator,
};
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
    self, AppState, browse_file, browse_folder, clipboard_state, clipboard_toggle,
    command_get_media_metadata, download_cancel, download_change_directory,
    download_change_directory_bulk, download_clear_completed, download_clear_failed,
    download_count_by_state, download_detail, download_list, download_logs, download_media_start,
    download_move_to_bottom, download_move_to_top, download_open_file, download_open_folder,
    download_pause, download_pause_all, download_redownload, download_remove,
    download_reorder_queue, download_resume, download_resume_all, download_retry,
    download_set_priority, download_start, download_verify_checksum, history_clear,
    history_delete_entry, history_export, history_get_by_id, history_list,
    history_purge_older_than, history_search, link_resolve, plugin_config_get,
    plugin_config_update, plugin_disable, plugin_enable, plugin_install, plugin_list,
    plugin_report_broken, plugin_store_install, plugin_store_list, plugin_store_refresh,
    plugin_store_update, plugin_uninstall, reveal_in_folder, settings_get, settings_update,
    stats_get, stats_top_modules, status_bar_get,
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
            let download_log_store =
                Arc::new(DownloadLogStore::new(DEFAULT_MAX_ENTRIES_PER_DOWNLOAD));

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
            let plugin_config_store: Arc<
                dyn crate::domain::ports::driven::PluginConfigStore,
            > = Arc::new(
                crate::adapters::driven::sqlite::plugin_config_repo::SqlitePluginConfigRepo::new(
                    db.clone(),
                ),
            );
            let plugin_loader_impl = Arc::new(
                ExtismPluginLoader::new(plugins_dir.clone(), shared_resources.clone())
                    .map_err(|e| e.to_string())?,
            );

            // Replay persisted plugin configs into the in-memory map so
            // `get_config()` calls inside loaded plugins observe the user's
            // last-saved values from the previous session, not just the
            // manifest defaults seeded by `build_host_functions`. Values
            // are inserted raw here — `build_host_functions` re-validates
            // the per-plugin map against the current schema when each
            // plugin loads (including hot-loads via the file watcher),
            // so stale entries get pruned at the right moment without
            // dropping overrides for plugins that load later in the
            // session.
            match plugin_config_store.list_all() {
                Ok(all) => {
                    for (plugin_name, kv) in all {
                        let entry = shared_resources
                            .plugin_configs()
                            .entry(plugin_name)
                            .or_default();
                        for (k, v) in kv {
                            entry.insert(k, v);
                        }
                    }
                }
                Err(e) => tracing::warn!(error = %e, "startup: failed to load plugin configs"),
            }

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
            let initial_engine_config = config_store
                .get_config()
                .unwrap_or_else(|_| crate::domain::model::config::AppConfig::default());
            let segmented_engine = Arc::new(
                SegmentedDownloadEngine::new(
                    reqwest_client,
                    file_storage.clone(),
                    event_bus.clone(),
                    4,
                )
                .with_dynamic_split(
                    initial_engine_config.dynamic_split_enabled,
                    initial_engine_config.dynamic_split_min_remaining_mb,
                ),
            );
            // Keep settings → engine bridge alive so UI changes to
            // dynamic_split_* propagate without a restart.
            application::services::subscribe_engine_to_config(
                event_bus.as_ref(),
                config_store.clone(),
                segmented_engine.clone(),
            );
            let download_engine: Arc<dyn DownloadEngine> = segmented_engine;

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
                Ok(cfg) => crate::domain::model::config::normalize_max_concurrent(
                    cfg.max_concurrent_downloads,
                ),
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "failed to read max_concurrent_downloads from config, falling back to default"
                    );
                    crate::domain::model::config::normalize_max_concurrent(
                        crate::domain::model::config::AppConfig::default().max_concurrent_downloads,
                    )
                }
            };
            let checksum_computer_for_queue: Arc<dyn crate::domain::ports::driven::ChecksumComputer> =
                Arc::new(crate::adapters::driven::network::StreamingChecksumComputer::new());
            let queue_manager = Arc::new(
                QueueManager::new(
                    download_repo.clone(),
                    download_engine.clone(),
                    event_bus.clone(),
                    initial_max_concurrent,
                )
                .with_checksum_pipeline(config_store.clone(), checksum_computer_for_queue),
            );

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
            let checksum_computer: Arc<dyn crate::domain::ports::driven::ChecksumComputer> =
                Arc::new(crate::adapters::driven::network::StreamingChecksumComputer::new());
            let file_opener: Arc<dyn crate::domain::ports::driven::FileOpener> =
                Arc::new(SystemFileOpener::new());
            let url_opener: Arc<dyn crate::domain::ports::driven::UrlOpener> =
                Arc::new(SystemUrlOpener::new());
            // Clone the Arcs the purge worker will need before the bus
            // takes ownership of `config_store`.
            let config_store_for_purge: Arc<dyn ConfigStore> = config_store.clone();
            let history_repo_for_purge: Arc<dyn HistoryRepository> = history_repo.clone();
            // History recorder bridge keeps its own handle so it survives
            // the move of `history_repo` into the query bus below.
            let history_repo_for_bridge: Arc<dyn HistoryRepository> = history_repo.clone();
            // Stats recorder bridge needs the write repo (for `find_by_id`)
            // before `CommandBus::new` takes ownership of it. Cloned twice
            // because the history recorder bridge needs the same handle.
            let download_repo_for_stats_bridge: Arc<dyn DownloadRepository> = download_repo.clone();
            let download_repo_for_history_bridge: Arc<dyn DownloadRepository> =
                download_repo.clone();
            let command_bus = Arc::new(
                CommandBus::new(
                    download_repo,
                    download_engine,
                    event_bus.clone(),
                    file_storage,
                    http_client,
                    plugin_loader.clone(),
                    config_store.clone(),
                    credential_store,
                    clipboard_observer,
                    archive_extractor.clone(),
                    history_repo.clone(),
                    Some(store_client),
                )
                .with_checksum_computer(checksum_computer)
                .with_file_opener(file_opener)
                .with_url_opener(url_opener)
                .with_plugin_config_store(plugin_config_store.clone()),
            );

            // Same pattern as the command-bus deps above: clone the stats
            // repo so the recorder bridge keeps its own handle once the
            // query bus takes ownership.
            let stats_repo_for_bridge: Arc<dyn StatsRepository> = stats_repo.clone();
            let query_bus = Arc::new(
                QueryBus::new(
                    download_read_repo.clone(),
                    history_repo,
                    stats_repo,
                    plugin_read_repo,
                    archive_extractor,
                )
                .with_plugin_loader(plugin_loader.clone())
                .with_plugin_config_store(plugin_config_store),
            );

            // ── Register AppState ───────────────────────────────────
            let app_plugin_loader: Arc<dyn PluginLoader> = plugin_loader_impl.clone();
            app.manage(AppState {
                command_bus,
                query_bus,
                download_log_store: download_log_store.clone(),
                plugin_loader: app_plugin_loader,
            });

            // ── System tray ─────────────────────────────────────────
            match setup_system_tray(app, false) {
                Ok(tray) => {
                    if let Some(static_icon) =
                        app.default_window_icon().cloned().map(|i| i.to_owned())
                    {
                        let frames = pulse_frames();
                        if let Some(swapper) = TauriIconSwapper::new(tray, static_icon, frames) {
                            let frame_count = swapper.frame_count();
                            let swapper: Arc<dyn IconSwapper> = Arc::new(swapper);
                            spawn_tray_animator(
                                event_bus.as_ref(),
                                swapper,
                                frame_count,
                                DEFAULT_FRAME_INTERVAL,
                            );
                        }
                    } else {
                        tracing::warn!(
                            "default window icon missing; tray animation disabled"
                        );
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to setup system tray: {e}");
                }
            }

            // ── Event bridges (domain events → frontend + desktop) ──
            spawn_tauri_event_bridge(app_handle.clone(), event_bus.as_ref());
            spawn_notification_bridge(
                app_handle,
                event_bus.as_ref(),
                config_store.clone(),
                download_read_repo.clone(),
            );
            spawn_download_log_bridge(event_bus.as_ref(), download_log_store);
            spawn_sqlite_progress_bridge(event_bus.as_ref(), db);
            // Project DownloadCompletedPersisted into the `statistics` table
            // so daily-volume / total-files / avg-speed KPIs match the
            // downloads table (issue #114).
            spawn_stats_recorder_bridge(
                event_bus.as_ref(),
                download_repo_for_stats_bridge,
                stats_repo_for_bridge,
            );
            // Project DownloadCompletedPersisted into the `history` table
            // so the History view (PRD §6.8), redownload-from-history
            // (P0.9) and the retention purge worker (P0.14) all see
            // completed downloads.
            spawn_history_recorder_bridge(
                event_bus.as_ref(),
                download_repo_for_history_bridge,
                history_repo_for_bridge,
            );

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

            // ── History retention purge worker ──────────────────────
            // Daily tokio task that hard-deletes history rows older than
            // `config.history_retention_days`. Shares the same Arcs the
            // CommandBus / QueryBus hold so settings mutations are visible
            // here without restart.
            let purge_worker = Arc::new(HistoryPurgeWorker::new(
                history_repo_for_purge,
                config_store_for_purge,
                Arc::new(SystemClock) as Arc<dyn Clock>,
                app_data_dir.join(HISTORY_PURGE_STATE_FILE),
            ));
            purge_worker.spawn();

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
            download_change_directory,
            download_change_directory_bulk,
            download_retry,
            download_redownload,
            download_verify_checksum,
            download_open_file,
            download_open_folder,
            download_pause_all,
            download_resume_all,
            download_set_priority,
            download_move_to_top,
            download_move_to_bottom,
            download_reorder_queue,
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
            plugin_config_get,
            plugin_config_update,
            plugin_report_broken,
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
            browse_folder,
            browse_file,
        ])
        .run(tauri::generate_context!())
        // Tauri's run() has no meaningful recovery path — panic is intentional here
        .expect("fatal: failed to start Vortex");
}
