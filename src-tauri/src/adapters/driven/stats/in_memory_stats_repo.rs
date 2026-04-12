//! In-memory statistics repository stub.
//!
//! This is a simple accumulator-based implementation for testing and early development.
//! In production, statistics are typically computed from history data via SQLite aggregations.

use std::sync::Mutex;

use crate::domain::error::DomainError;
use crate::domain::model::views::StatsView;
use crate::domain::ports::driven::stats_repository::StatsRepository;

struct StatsAccumulator {
    total_bytes: u64,
    total_files: u64,
    total_speed_sum: u64,
    peak_speed: u64,
}

/// In-memory repository for aggregated download statistics.
///
/// Accumulates total bytes, file count, and speed metrics without persistence.
/// Useful for testing command handlers and services without SQLite.
pub struct InMemoryStatsRepository {
    accumulator: Mutex<StatsAccumulator>,
}

impl InMemoryStatsRepository {
    /// Create a new empty in-memory statistics repository.
    pub fn new() -> Self {
        Self {
            accumulator: Mutex::new(StatsAccumulator {
                total_bytes: 0,
                total_files: 0,
                total_speed_sum: 0,
                peak_speed: 0,
            }),
        }
    }
}

impl Default for InMemoryStatsRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl StatsRepository for InMemoryStatsRepository {
    fn record_completed(&self, bytes: u64, avg_speed: u64) -> Result<(), DomainError> {
        let mut accumulator = match self.accumulator.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        accumulator.total_bytes = accumulator.total_bytes.saturating_add(bytes);
        accumulator.total_files = accumulator.total_files.saturating_add(1);
        accumulator.total_speed_sum = accumulator.total_speed_sum.saturating_add(avg_speed);
        accumulator.peak_speed = accumulator.peak_speed.max(avg_speed);

        Ok(())
    }

    fn get_stats(&self) -> Result<StatsView, DomainError> {
        let accumulator = match self.accumulator.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let avg_speed = if accumulator.total_files > 0 {
            accumulator.total_speed_sum / accumulator.total_files
        } else {
            0
        };

        Ok(StatsView {
            total_downloaded_bytes: accumulator.total_bytes,
            total_files: accumulator.total_files,
            avg_speed,
            peak_speed: accumulator.peak_speed,
            // TODO: track failures to compute a real 0.0–1.0 fraction
            success_rate: 0.0,
            daily_volumes: vec![],
            top_hosts: vec![],
        })
    }
}
