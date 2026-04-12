//! Handler for `ExtractArchiveCommand`.
//!
//! Validates the download exists, transitions it to Extracting state,
//! calls the ArchiveExtractor port, and publishes domain events.

use std::path::PathBuf;

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::archive::ExtractSummary;
use crate::domain::model::download::DownloadState;

impl CommandBus {
    /// Extract an archive associated with a completed download.
    ///
    /// The handler:
    /// 1. Looks up the download by ID
    /// 2. Verifies it's in `Completed` state
    /// 3. Transitions it to `Extracting`
    /// 4. Calls the ArchiveExtractor port
    /// 5. Publishes `DownloadExtracting` then `DownloadCompleted` events
    pub async fn handle_extract_archive(
        &self,
        cmd: super::ExtractArchiveCommand,
    ) -> Result<ExtractSummary, AppError> {
        let mut download = self
            .download_repo()
            .find_by_id(cmd.download_id)?
            .ok_or_else(|| AppError::NotFound(format!("download {}", cmd.download_id.0)))?;

        // Verify the download is in a valid state for extraction
        if download.state() != DownloadState::Completed {
            return Err(AppError::Validation(format!(
                "download must be Completed to extract, current state: {}",
                download.state()
            )));
        }

        // Transition to Extracting
        download.start_extracting()?;
        self.download_repo().save(&download)?;
        self.event_bus().publish(DomainEvent::DownloadExtracting {
            id: cmd.download_id,
        });

        // Determine the extraction destination
        let file_path = PathBuf::from(download.destination_path());
        let dest_dir = cmd
            .dest_dir
            .unwrap_or_else(|| file_path.parent().unwrap_or(&file_path).to_path_buf());

        // Perform extraction (blocking I/O, run on blocking thread pool)
        let extractor = self.archive_extractor_arc();
        let password = cmd.password.clone();

        let result = tokio::task::spawn_blocking(move || {
            extractor.extract(&file_path, &dest_dir, password.as_deref())
        })
        .await
        .map_err(|e| AppError::Storage(format!("extraction task failed: {}", e)))?;

        match result {
            Ok(summary) => {
                // Mark as completed after successful extraction
                download.complete()?;
                self.download_repo().save(&download)?;
                self.event_bus().publish(DomainEvent::DownloadCompleted {
                    id: cmd.download_id,
                });
                Ok(summary)
            }
            Err(e) => {
                // Restore download to Error state so user can retry
                download.fail(e.to_string())?;
                self.download_repo().save(&download)?;
                self.event_bus().publish(DomainEvent::DownloadFailed {
                    id: cmd.download_id,
                    error: e.to_string(),
                });
                Err(AppError::Storage(format!("extraction failed: {}", e)))
            }
        }
    }
}
