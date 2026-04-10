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

pub use adapters::driven::plugin::builtin::HttpModule;
pub use adapters::driven::plugin::capabilities::SharedHostResources;
pub use adapters::driven::plugin::{ExtismPluginLoader, PluginRegistry, PluginWatcher};
pub use adapters::driving::tauri_ipc::{
    self, AppState, download_cancel, download_count_by_state, download_detail, download_list,
    download_pause, download_pause_all, download_remove, download_resume, download_resume_all,
    download_retry, download_set_priority, download_start, link_resolve, plugin_disable,
    plugin_enable, plugin_install, plugin_list, plugin_uninstall,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            // TODO(task-16): construct AppState from real adapters and call
            // app.manage(state). IPC handlers require State<'_, AppState>;
            // until wired, the app starts but IPC calls will fail.
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
        ])
        .run(tauri::generate_context!())
        // Tauri's run() has no meaningful recovery path — panic is intentional here
        .expect("fatal: failed to start Vortex");
}
