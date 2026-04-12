//! Handler for `ListArchiveContentsQuery`.
//!
//! Lists entries within an archive file without extracting.
//! Pure read operation — no state mutation.

use std::path::PathBuf;

use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::domain::model::archive::ArchiveEntry;

impl QueryBus {
    /// List the contents of an archive file.
    ///
    /// Returns a vector of archive entries with path, size, and metadata.
    pub async fn handle_list_archive_contents(
        &self,
        query: super::ListArchiveContentsQuery,
    ) -> Result<Vec<ArchiveEntry>, AppError> {
        let file_path = PathBuf::from(&query.file_path);
        let password = query.password.clone();
        let extractor = self.archive_extractor_arc();

        let entries = tokio::task::spawn_blocking(move || {
            extractor.list_contents(&file_path, password.as_deref())
        })
        .await
        .map_err(|e| AppError::Storage(format!("list task failed: {}", e)))??;

        Ok(entries)
    }
}
