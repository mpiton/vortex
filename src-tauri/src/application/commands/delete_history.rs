//! Delete a single history entry or clear the whole history.

use crate::application::command_bus::CommandBus;
use crate::application::commands::{ClearHistoryCommand, DeleteHistoryEntryCommand};
use crate::application::error::AppError;
use crate::domain::error::DomainError;

impl CommandBus {
    pub async fn handle_delete_history_entry(
        &self,
        cmd: DeleteHistoryEntryCommand,
    ) -> Result<(), AppError> {
        let removed = self.history_repo().delete_by_id(cmd.id)?;
        if !removed {
            return Err(AppError::Domain(DomainError::NotFound(format!(
                "history entry {}",
                cmd.id
            ))));
        }
        Ok(())
    }

    pub async fn handle_clear_history(&self, _cmd: ClearHistoryCommand) -> Result<u64, AppError> {
        let count = self.history_repo().delete_all()?;
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

    fn seed(repo: &InMemoryHistoryRepo, count: u64) {
        for i in 1..=count {
            repo.record(&HistoryEntry {
                id: 0,
                download_id: DownloadId(i),
                file_name: format!("f{i}.bin"),
                url: format!("https://ex.com/f{i}"),
                total_bytes: 10,
                completed_at: i * 1_000,
                duration_seconds: 1,
                avg_speed: 10,
                destination_path: format!("/tmp/f{i}"),
            })
            .unwrap();
        }
    }

    #[tokio::test]
    async fn delete_history_entry_removes_the_row() {
        let repo = Arc::new(InMemoryHistoryRepo::new());
        seed(&repo, 2);
        let bus = make_history_command_bus(repo.clone());

        let id = repo.snapshot().first().unwrap().id;
        bus.handle_delete_history_entry(DeleteHistoryEntryCommand { id })
            .await
            .unwrap();
        assert_eq!(repo.snapshot().len(), 1);
    }

    #[tokio::test]
    async fn delete_history_entry_returns_not_found_for_missing_id() {
        let repo = Arc::new(InMemoryHistoryRepo::new());
        let bus = make_history_command_bus(repo);

        let err = bus
            .handle_delete_history_entry(DeleteHistoryEntryCommand { id: 999 })
            .await
            .unwrap_err();
        assert!(
            matches!(err, AppError::Domain(DomainError::NotFound(_))),
            "expected NotFound error, got {err:?}"
        );
    }

    #[tokio::test]
    async fn clear_history_returns_count_and_drains_repo() {
        let repo = Arc::new(InMemoryHistoryRepo::new());
        seed(&repo, 3);
        let bus = make_history_command_bus(repo.clone());

        let removed = bus.handle_clear_history(ClearHistoryCommand).await.unwrap();
        assert_eq!(removed, 3);
        assert!(repo.snapshot().is_empty());
    }
}
