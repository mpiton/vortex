//! Time-driven background workers.
//!
//! Hosts daemon tasks that run on wall-clock schedules rather than in
//! response to domain events. Each worker owns a `tokio` task spawned
//! from app setup and shuts down when the runtime stops.

mod history_purge_worker;
mod system_clock;

pub use history_purge_worker::{HISTORY_PURGE_STATE_FILE, HistoryPurgeWorker};
pub use system_clock::SystemClock;
