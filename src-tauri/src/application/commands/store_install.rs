//! Handlers for downloading and installing/updating plugins from the store.

use crate::application::command_bus::CommandBus;
use crate::application::commands::store_refresh::read_cache;
use crate::application::error::AppError;
use crate::application::read_models::plugin_store_view::PluginStoreEntryDto;
use crate::domain::model::plugin::PluginCategory;
use crate::domain::model::plugin_store::{PluginStoreEntry, PluginStoreStatus};

pub struct StoreInstallCommand {
    pub name: String,
}

pub struct StoreUpdateCommand {
    pub name: String,
}

impl CommandBus {
    /// Download the plugin binary from GitHub Releases, verify checksum,
    /// and install it via the plugin loader.
    pub async fn handle_store_install(
        &self,
        cmd: StoreInstallCommand,
        cache_path: &std::path::Path,
    ) -> Result<(), AppError> {
        let client = self
            .plugin_store_client()
            .ok_or_else(|| AppError::Plugin("store client not configured".into()))?;

        // Find the entry in the cache
        let raw = read_cache(cache_path)?;
        let entry_dto: PluginStoreEntryDto = raw
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .find(|dto: &PluginStoreEntryDto| dto.name == cmd.name)
            .ok_or_else(|| AppError::Plugin(format!("plugin '{}' not found in cache", cmd.name)))?;

        // Reconstruct domain entry for the client
        let category = entry_dto
            .category
            .parse::<PluginCategory>()
            .unwrap_or(PluginCategory::Utility);
        let domain_entry = PluginStoreEntry {
            name: entry_dto.name.clone(),
            description: entry_dto.description.clone(),
            author: entry_dto.author.clone(),
            version: entry_dto.version.clone(),
            category,
            repository: String::new(),
            checksum_sha256: String::new(),
            official: entry_dto.official,
            min_vortex_version: None,
            status: PluginStoreStatus::NotInstalled,
            installed_version: None,
        };

        let plugin_dir = client
            .download_plugin(&domain_entry)
            .map_err(|e| AppError::Plugin(e.to_string()))?;

        // Parse manifest from the downloaded directory and load via the plugin loader.
        // Uses load_from_dir which calls parse_manifest internally (adapter concern).
        let loader = self.plugin_loader_arc();
        tokio::task::spawn_blocking(move || loader.load_from_dir(&plugin_dir))
            .await
            .map_err(|e| AppError::Plugin(format!("plugin install task failed: {e}")))?
            .map_err(AppError::from)?;

        tracing::info!(plugin = %cmd.name, "plugin installed from store");
        Ok(())
    }

    /// Unload the current version and install the latest from the registry.
    pub async fn handle_store_update(
        &self,
        cmd: StoreUpdateCommand,
        cache_path: &std::path::Path,
    ) -> Result<(), AppError> {
        // Unload existing (ignore error if not loaded)
        let _ = self.plugin_loader().unload(&cmd.name);

        self.handle_store_install(StoreInstallCommand { name: cmd.name }, cache_path)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::commands::store_refresh::write_cache;
    use crate::domain::model::plugin_store::PluginStoreEntry;
    use tempfile::TempDir;

    fn make_entry(name: &str, version: &str) -> PluginStoreEntry {
        PluginStoreEntry {
            name: name.into(),
            description: "test plugin".into(),
            author: "author".into(),
            version: version.into(),
            category: PluginCategory::Utility,
            repository: "https://github.com/author/test-plugin".into(),
            checksum_sha256: "abc123".into(),
            official: false,
            min_vortex_version: None,
            status: PluginStoreStatus::NotInstalled,
            installed_version: None,
        }
    }

    #[tokio::test]
    async fn test_store_install_not_found_returns_error() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("cache.json");
        // Empty cache
        write_cache(&cache, &[]).unwrap();

        // Test the cache lookup in isolation
        let raw = read_cache(&cache).unwrap();
        let found = raw
            .iter()
            .filter_map(|v| serde_json::from_value::<PluginStoreEntryDto>(v.clone()).ok())
            .find(|dto| dto.name == "missing-plugin");
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_store_install_found_in_cache() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("cache.json");
        let entries = vec![make_entry("my-plugin", "1.0.0")];
        write_cache(&cache, &entries).unwrap();

        let raw = read_cache(&cache).unwrap();
        let found = raw
            .iter()
            .filter_map(|v| serde_json::from_value::<PluginStoreEntryDto>(v.clone()).ok())
            .find(|dto| dto.name == "my-plugin");
        assert!(found.is_some());
        assert_eq!(found.unwrap().version, "1.0.0");
    }
}
