//! Repository for aggregated download statistics.
//!
//! Provides pre-computed statistics for the statistics view.

use crate::domain::error::DomainError;
use crate::domain::model::views::{ModuleStats, StatsPeriod, StatsView};

/// Reads and records aggregated download statistics.
///
/// Statistics are typically computed from history data via SQL
/// aggregations. The `record_completed` method updates running
/// totals when a download finishes.
pub trait StatsRepository: Send + Sync {
    /// Record that a download completed with the given byte count and speed.
    fn record_completed(&self, bytes: u64, avg_speed: u64) -> Result<(), DomainError>;

    /// Get aggregated statistics filtered by the given time window.
    fn get_stats(&self, period: StatsPeriod) -> Result<StatsView, DomainError>;

    /// Return the most-used resolving modules ordered by download count desc.
    fn top_modules(&self, limit: u32) -> Result<Vec<ModuleStats>, DomainError>;
}
