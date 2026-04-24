//! Port for launching files and their enclosing folders via the host OS.
//!
//! Implementations delegate to the platform's native file association or
//! file-manager "reveal" action. Both calls are expected to return quickly
//! — the OS takes over once the target has been handed off.

use std::path::Path;

use crate::domain::error::DomainError;

pub trait FileOpener: Send + Sync {
    /// Launch `path` with the OS default application.
    ///
    /// Fails with `DomainError::NotFound` when the file is missing, and with
    /// `DomainError::StorageError` when the platform launcher refuses or the
    /// child process exits non-zero.
    fn open_file(&self, path: &Path) -> Result<(), DomainError>;

    /// Open the folder containing `path`, selecting the file itself when the
    /// platform file manager supports it (Windows `explorer /select`, macOS
    /// `open -R`). Linux falls back to opening the parent directory.
    fn reveal_file(&self, path: &Path) -> Result<(), DomainError>;
}
