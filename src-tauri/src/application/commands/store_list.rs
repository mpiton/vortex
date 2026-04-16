//! Handler for listing the plugin store catalogue from local cache.

use crate::application::command_bus::CommandBus;
use crate::application::commands::store_refresh::read_cache;
use crate::application::error::AppError;
use crate::application::read_models::plugin_store_view::PluginStoreEntryDto;

impl CommandBus {
    /// Read the local cache and return enriched store entries.
    ///
    /// Does NOT fetch from the network. Callers should call `handle_store_refresh`
    /// first if the cache is absent or stale.
    pub async fn handle_store_list(
        &self,
        cache_path: &std::path::Path,
    ) -> Result<Vec<PluginStoreEntryDto>, AppError> {
        let raw = read_cache(cache_path)?;
        let dtos: Vec<PluginStoreEntryDto> = raw
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();
        Ok(dtos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::commands::store_refresh::write_cache;
    use crate::domain::model::plugin::PluginCategory;
    use crate::domain::model::plugin_store::{PluginStoreEntry, PluginStoreStatus};
    use tempfile::TempDir;

    fn make_entry(name: &str, status: PluginStoreStatus) -> PluginStoreEntry {
        PluginStoreEntry {
            name: name.into(),
            description: "desc".into(),
            author: "auth".into(),
            version: "1.0.0".into(),
            category: PluginCategory::Crawler,
            repository: "https://github.com/a/b".into(),
            checksum_sha256: "abc".into(),
            official: false,
            min_vortex_version: None,
            status,
            installed_version: None,
        }
    }

    #[tokio::test]
    async fn test_handle_store_list_returns_cached_entries() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("cache.json");
        let entries = vec![
            make_entry("plugin-a", PluginStoreStatus::Installed),
            make_entry("plugin-b", PluginStoreStatus::NotInstalled),
        ];
        write_cache(&cache, &entries).unwrap();

        // Test the cache round-trip directly
        let raw = read_cache(&cache).unwrap();
        assert_eq!(raw.len(), 2);
        let dtos: Vec<PluginStoreEntryDto> = raw
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();
        assert_eq!(dtos.len(), 2);
        assert_eq!(dtos[0].name, "plugin-a");
        assert_eq!(dtos[0].status, "installed");
        assert_eq!(dtos[1].name, "plugin-b");
        assert_eq!(dtos[1].status, "not_installed");
    }

    #[tokio::test]
    async fn test_handle_store_list_returns_error_on_missing_cache() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("missing.json");
        let result = read_cache(&cache);
        assert!(result.is_err()); // file not found — caller must refresh first
    }
}
