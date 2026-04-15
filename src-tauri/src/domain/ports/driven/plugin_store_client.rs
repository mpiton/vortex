//! Port for fetching the remote plugin registry and downloading plugin binaries.

use std::path::PathBuf;

use crate::domain::error::DomainError;
use crate::domain::model::plugin_store::PluginStoreEntry;

/// Reads the remote plugin catalogue and downloads plugin assets.
pub trait PluginStoreClient: Send + Sync {
    /// Fetch and parse the central `registry.toml` from GitHub Raw.
    ///
    /// Returns the list of declared plugins with `status = NotInstalled`
    /// (callers are responsible for enriching statuses via `with_status`).
    fn fetch_registry(&self) -> Result<Vec<PluginStoreEntry>, DomainError>;

    /// Download `{name}.wasm` and `plugin.toml` from GitHub Releases,
    /// verify the sha256 checksum of the wasm binary, write both files
    /// into a temporary directory, and return its path.
    ///
    /// Errors:
    /// - `DomainError::PluginError("checksum mismatch")` if sha256 does not match
    /// - `DomainError::PluginError("download failed: ...")` on network errors
    fn download_plugin(&self, entry: &PluginStoreEntry) -> Result<PathBuf, DomainError>;
}
