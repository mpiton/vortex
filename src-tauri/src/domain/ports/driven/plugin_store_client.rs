//! Port for fetching the remote plugin registry and downloading plugin binaries.

use std::path::PathBuf;

use crate::domain::error::DomainError;
use crate::domain::model::plugin_store::PluginStoreEntry;

/// Store-authenticated identity and digests handed to the plugin loader.
///
/// These values originate from a fresh registry fetch, never from the
/// user-writable Store UI cache or from `plugin.toml` itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficialPluginProvenance {
    pub name: String,
    pub version: String,
    pub wasm_sha256: String,
    pub manifest_sha256: String,
}

/// Files downloaded from a freshly fetched Store entry and checksum-verified.
/// Official entries additionally carry provenance eligible for native grants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorePluginDownload {
    pub directory: PathBuf,
    pub provenance: Option<OfficialPluginProvenance>,
    pub min_vortex_version: Option<String>,
}

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

    /// Refetch the authoritative registry entry and download both assets.
    /// Only `official = true` entries receive provenance that can be persisted
    /// by the loader and later unlock a native broker.
    ///
    /// This default keeps existing Store adapters source-compatible while
    /// ensuring callers do not elevate data read from the local UI cache.
    fn download_store_plugin(&self, name: &str) -> Result<StorePluginDownload, DomainError> {
        let entry = self
            .fetch_registry()?
            .into_iter()
            .find(|entry| entry.name == name)
            .ok_or_else(|| DomainError::NotFound(format!("Store plugin '{name}'")))?;
        let provenance = if entry.official {
            let manifest_sha256 = entry.checksum_sha256_toml.clone().ok_or_else(|| {
                DomainError::PluginError(format!(
                    "official plugin '{name}' is missing checksum_sha256_toml"
                ))
            })?;
            Some(OfficialPluginProvenance {
                name: entry.name.clone(),
                version: entry.version.clone(),
                wasm_sha256: entry.checksum_sha256.clone(),
                manifest_sha256,
            })
        } else {
            None
        };
        let directory = self.download_plugin(&entry)?;
        Ok(StorePluginDownload {
            directory,
            provenance,
            min_vortex_version: entry.min_vortex_version,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::plugin::PluginCategory;
    use crate::domain::model::plugin_store::PluginStoreStatus;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct FakeStoreClient {
        download_called: AtomicBool,
        official: bool,
    }

    impl PluginStoreClient for FakeStoreClient {
        fn fetch_registry(&self) -> Result<Vec<PluginStoreEntry>, DomainError> {
            Ok(vec![PluginStoreEntry {
                name: "community-plugin".into(),
                description: "test".into(),
                author: "tester".into(),
                version: "1.0.0".into(),
                category: PluginCategory::Utility,
                repository: "https://example.test/community-plugin".into(),
                checksum_sha256: "a".repeat(64),
                checksum_sha256_toml: Some("b".repeat(64)),
                official: self.official,
                min_vortex_version: None,
                status: PluginStoreStatus::NotInstalled,
                installed_version: None,
            }])
        }

        fn download_plugin(&self, _: &PluginStoreEntry) -> Result<PathBuf, DomainError> {
            self.download_called.store(true, Ordering::SeqCst);
            Ok(PathBuf::from("/tmp/community-plugin"))
        }
    }

    #[test]
    fn test_download_store_plugin_installs_unofficial_without_provenance() {
        let client = FakeStoreClient {
            download_called: AtomicBool::new(false),
            official: false,
        };

        let result = client.download_store_plugin("community-plugin").unwrap();

        assert!(result.provenance.is_none());
        assert!(client.download_called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_download_store_plugin_returns_provenance_only_for_official_entry() {
        let client = FakeStoreClient {
            download_called: AtomicBool::new(false),
            official: true,
        };

        let result = client.download_store_plugin("community-plugin").unwrap();

        let provenance = result.provenance.expect("official provenance");
        assert_eq!(provenance.name, "community-plugin");
        assert_eq!(provenance.wasm_sha256, "a".repeat(64));
        assert_eq!(provenance.manifest_sha256, "b".repeat(64));
    }
}
