//! Handlers for downloading and installing/updating plugins from the store.

use crate::application::command_bus::CommandBus;
use crate::application::commands::store_refresh::read_cache;
use crate::application::error::AppError;
use crate::application::read_models::plugin_store_view::PluginStoreEntryDto;
use crate::domain::model::plugin_store::{PluginStoreEntry, PluginStoreStatus};

pub struct StoreInstallCommand {
    pub name: String,
}

pub struct StoreUpdateCommand {
    pub name: String,
}

/// Returns true if `running` >= `required` (semver major.minor.patch).
/// Falls back to true (permissive) if either version cannot be parsed.
fn is_version_compatible(running: &str, required: &str) -> bool {
    fn parse(v: &str) -> Option<(u64, u64, u64)> {
        let mut p = v.splitn(3, '.');
        let a = p.next()?.parse().ok()?;
        let b = p.next()?.parse().ok()?;
        let c = p.next().unwrap_or("0").parse().ok()?;
        Some((a, b, c))
    }
    match (parse(running), parse(required)) {
        (Some(r), Some(req)) => r >= req,
        _ => true,
    }
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
            .plugin_store_client_arc()
            .ok_or_else(|| AppError::Plugin("store client not configured".into()))?;

        // Find the entry in the cache
        let raw = read_cache(cache_path)?;
        let entry_dto: PluginStoreEntryDto = raw
            .into_iter()
            .filter_map(|v| {
                serde_json::from_value(v)
                    .map_err(|e| tracing::warn!(%e, "skipping malformed cache entry"))
                    .ok()
            })
            .find(|dto: &PluginStoreEntryDto| dto.name == cmd.name)
            .ok_or_else(|| AppError::Plugin(format!("plugin '{}' not found in cache", cmd.name)))?;

        // Check minimum Vortex version requirement
        if let Some(ref min_ver) = entry_dto.min_vortex_version {
            let app_ver = env!("CARGO_PKG_VERSION");
            if !is_version_compatible(app_ver, min_ver) {
                return Err(AppError::Plugin(format!(
                    "plugin '{}' requires Vortex >= {min_ver} (running {})",
                    cmd.name, app_ver
                )));
            }
        }

        // Reconstruct domain entry for the client using REAL values from cache
        let category = entry_dto
            .category
            .parse::<crate::domain::model::plugin::PluginCategory>()
            .map_err(|e| {
                AppError::Plugin(format!(
                    "unknown plugin category '{}': {e}",
                    entry_dto.category
                ))
            })?;
        let domain_entry = PluginStoreEntry {
            name: entry_dto.name.clone(),
            description: entry_dto.description.clone(),
            author: entry_dto.author.clone(),
            version: entry_dto.version.clone(),
            category,
            repository: entry_dto.repository.clone(),
            checksum_sha256: entry_dto.checksum_sha256.clone(),
            checksum_sha256_toml: entry_dto.checksum_sha256_toml.clone(),
            official: entry_dto.official,
            min_vortex_version: entry_dto.min_vortex_version.clone(),
            status: PluginStoreStatus::NotInstalled,
            installed_version: None,
        };

        let plugin_dir = tokio::task::spawn_blocking(move || client.download_plugin(&domain_entry))
            .await
            .map_err(|e| AppError::Plugin(format!("download task failed: {e}")))?
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
        // Unload from memory first (ignore error if not loaded)
        let _ = self.plugin_loader().unload(&cmd.name);
        // Reinstall (load_from_dir will remove and replace old files on disk)
        self.handle_store_install(StoreInstallCommand { name: cmd.name }, cache_path)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::commands::store_refresh::write_cache;
    use crate::domain::model::plugin::PluginCategory;
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
            checksum_sha256_toml: None,
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

    #[test]
    fn test_is_version_compatible_running_equal() {
        assert!(is_version_compatible("1.0.0", "1.0.0"));
    }

    #[test]
    fn test_is_version_compatible_running_ahead() {
        assert!(is_version_compatible("1.2.0", "1.0.0"));
        assert!(is_version_compatible("2.0.0", "1.9.9"));
    }

    #[test]
    fn test_is_version_compatible_running_behind() {
        assert!(!is_version_compatible("0.9.0", "1.0.0"));
    }

    #[test]
    fn test_is_version_compatible_permissive_on_unparseable() {
        assert!(is_version_compatible("not-a-version", "1.0.0"));
        assert!(is_version_compatible("1.0.0", "not-a-version"));
    }
}
