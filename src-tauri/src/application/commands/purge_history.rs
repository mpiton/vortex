//! Purge history entries older than a caller-computed cutoff.

use crate::application::command_bus::CommandBus;
use crate::application::commands::PurgeHistoryCommand;
use crate::application::error::AppError;

impl CommandBus {
    pub async fn handle_purge_history(&self, cmd: PurgeHistoryCommand) -> Result<u64, AppError> {
        let count = self
            .history_repo()
            .delete_older_than(cmd.before_timestamp)?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::application::test_support::{InMemoryHistoryRepo, make_history_command_bus};
    use crate::domain::model::download::DownloadId;
    use crate::domain::model::views::HistoryEntry;
    use crate::domain::ports::driven::HistoryRepository;

    #[tokio::test]
    async fn purge_history_deletes_entries_older_than_cutoff() {
        let repo = Arc::new(InMemoryHistoryRepo::new());
        for completed_at in [1_000, 2_000, 3_000] {
            repo.record(&HistoryEntry {
                id: 0,
                download_id: DownloadId(completed_at),
                file_name: "f.bin".to_string(),
                url: "https://ex.com/f".to_string(),
                total_bytes: 0,
                completed_at,
                duration_seconds: 0,
                avg_speed: 0,
                destination_path: "/tmp/f".to_string(),
            })
            .unwrap();
        }
        let bus = make_history_command_bus(repo.clone());

        let purged = bus
            .handle_purge_history(PurgeHistoryCommand {
                before_timestamp: 2_500,
            })
            .await
            .unwrap();
        assert_eq!(purged, 2);

        let remaining = repo.snapshot();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].completed_at, 3_000);
    }
}
