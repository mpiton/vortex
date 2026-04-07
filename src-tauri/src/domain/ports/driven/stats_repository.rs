//! Repository for aggregated download statistics.
//!
//! Provides pre-computed statistics for the statistics view.

use crate::domain::error::DomainError;
use crate::domain::model::views::StatsView;

/// Reads and records aggregated download statistics.
///
/// Statistics are typically computed from history data via SQL
/// aggregations. The `record_completed` method updates running
/// totals when a download finishes.
pub trait StatsRepository: Send + Sync {
    /// Record that a download completed with the given byte count and speed.
    fn record_completed(&self, bytes: u64, avg_speed: u64) -> Result<(), DomainError>;

    /// Get the current aggregated statistics.
    fn get_stats(&self) -> Result<StatsView, DomainError>;
}
