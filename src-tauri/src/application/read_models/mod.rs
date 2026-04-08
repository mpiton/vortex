//! Application-layer read model DTOs with serde serialization.

pub mod download_detail_view;
pub mod download_view;
pub mod history_view;
pub mod plugin_view;
pub mod stats_view;

// Re-exports consumed once query handlers return these DTOs.
#[allow(unused_imports)]
pub use download_detail_view::{DownloadDetailViewDto, SegmentViewDto};
#[allow(unused_imports)]
pub use download_view::DownloadViewDto;
#[allow(unused_imports)]
pub use history_view::HistoryViewDto;
#[allow(unused_imports)]
pub use plugin_view::PluginViewDto;
#[allow(unused_imports)]
pub use stats_view::{DailyVolumeDto, HostStatsDto, StatsViewDto};
