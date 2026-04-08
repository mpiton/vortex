mod adapters;
mod application;
pub mod domain;

// Public API — concrete types for app wiring (main.rs, Tauri setup, integration tests)
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        // Tauri's run() has no meaningful recovery path — panic is intentional here
        .expect("fatal: failed to start Vortex");
}
