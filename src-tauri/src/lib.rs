mod adapters;
mod application;
pub mod domain;

// Public API — concrete types for app wiring (main.rs, Tauri setup, integration tests)
pub use adapters::driven::event::TokioEventBus;
pub use adapters::driven::event::spawn_tauri_event_bridge;
pub use adapters::driven::filesystem::FsFileStorage;
pub use adapters::driven::network::ReqwestHttpClient;
pub use adapters::driven::network::SegmentedDownloadEngine;
pub use adapters::driven::sqlite::connection;
pub use adapters::driven::sqlite::download_read_repo::SqliteDownloadReadRepo;
pub use adapters::driven::sqlite::download_repo::SqliteDownloadRepo;
pub use adapters::driven::sqlite::history_repo::SqliteHistoryRepo;
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

pub use adapters::driving::tauri_ipc::{
    self, AppState, download_cancel, download_count_by_state, download_detail, download_list,
    download_pause, download_pause_all, download_remove, download_resume, download_resume_all,
    download_retry, download_set_priority, download_start,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // AppState wiring: construct all driven-port adapters and inject
            // into CommandBus / QueryBus. Full implementation deferred to
            // task 16 (frontend layout) when the frontend connects.
            //
            // Required adapters:
            //   let db = connection::connect(&app_data_dir).await?;
            //   let event_bus = Arc::new(TokioEventBus::new(1024));
            //   let http_client = Arc::new(ReqwestHttpClient::new());
            //   let file_storage = Arc::new(FsFileStorage::new());
            //   let engine = Arc::new(SegmentedDownloadEngine::new(...));
            //   let download_repo = Arc::new(SqliteDownloadRepo::new(db));
            //   ... (plugin_loader, config_store, credential_store, clipboard)
            //   let command_bus = Arc::new(CommandBus::new(...));
            //   let query_bus = Arc::new(QueryBus::new(...));
            //   app.manage(AppState { command_bus, query_bus });
            let _ = app;
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
        ])
        .run(tauri::generate_context!())
        // Tauri's run() has no meaningful recovery path — panic is intentional here
        .expect("fatal: failed to start Vortex");
}
