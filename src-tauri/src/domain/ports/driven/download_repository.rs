//! Write repository for the `Download` aggregate (CQRS write side).
//!
//! Manipulates domain entities directly. Used by command handlers
//! to load, persist, and delete downloads.

use crate::domain::error::DomainError;
use crate::domain::model::account::AccountId;
use crate::domain::model::download::{Download, DownloadId, DownloadState};

/// Persists and retrieves `Download` aggregates.
///
/// This is the **write** repository in the CQRS pattern. It works with
/// full domain entities, not flattened views. For read-optimized queries,
/// see `DownloadReadRepository`.
pub trait DownloadRepository: Send + Sync {
    /// Find a download by its unique identifier.
    fn find_by_id(&self, id: DownloadId) -> Result<Option<Download>, DomainError>;

    /// Persist a download (insert or update).
    fn save(&self, download: &Download) -> Result<(), DomainError>;

    /// Persist a batch of downloads atomically.
    ///
    /// Default implementation iterates `save`; adapters that support
    /// transactions should override to commit all writes together so a
    /// mid-batch failure does not leave partial state.
    fn save_batch(&self, downloads: &[Download]) -> Result<(), DomainError> {
        for d in downloads {
            self.save(d)?;
        }
        Ok(())
    }

    /// Persist a failed download and store its raw backend error string.
    fn save_failed(&self, download: &Download, _error_message: &str) -> Result<(), DomainError> {
        self.save(download)
    }

    /// Delete a download by its identifier.
    fn delete(&self, id: DownloadId) -> Result<(), DomainError>;

    /// Change only the account association of an existing download.
    ///
    /// Implementations must not insert a missing row or overwrite any other
    /// aggregate field. The boolean reports whether the expected reference
    /// matched and was replaced atomically.
    fn compare_and_set_account_reference(
        &self,
        _id: DownloadId,
        _expected: &AccountId,
        _replacement: &AccountId,
    ) -> Result<bool, DomainError> {
        Err(DomainError::StorageError(
            "atomic account reference updates are unavailable".into(),
        ))
    }

    /// Find all downloads in a given state.
    fn find_by_state(&self, state: DownloadState) -> Result<Vec<Download>, DomainError>;

    /// Whether any persisted download still depends on an account.
    /// Implementations must query independently of mutable download state so
    /// a concurrent state transition cannot create a false negative.
    #[cfg(not(test))]
    fn has_account_reference(&self, account_id: &AccountId) -> Result<bool, DomainError>;

    /// Unit-test fakes are allowed a compatibility scan so every focused
    /// command test does not need an unrelated persistence primitive.
    #[cfg(test)]
    fn has_account_reference(&self, account_id: &AccountId) -> Result<bool, DomainError> {
        const STATES: [DownloadState; 9] = [
            DownloadState::Queued,
            DownloadState::Downloading,
            DownloadState::Paused,
            DownloadState::Waiting,
            DownloadState::Retry,
            DownloadState::Error,
            DownloadState::Extracting,
            DownloadState::Completed,
            DownloadState::Checking,
        ];
        for state in STATES {
            if self
                .find_by_state(state)?
                .iter()
                .any(|download| download.account_id() == Some(account_id))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
