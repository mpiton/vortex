//! Handler for refreshing the plugin store registry from GitHub.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::model::plugin_store::PluginStoreEntry;

impl CommandBus {
    /// Fetch the remote registry, enrich statuses, write to disk cache.
    pub async fn handle_store_refresh(&self, cache_path: &std::path::Path) -> Result<(), AppError> {
        let client = self
            .plugin_store_client_arc()
            .ok_or_else(|| AppError::Plugin("store client not configured".into()))?;

        let mut entries = tokio::task::spawn_blocking(move || client.fetch_registry())
            .await
            .map_err(|e| AppError::Plugin(format!("registry fetch task failed: {e}")))?
            .map_err(|e| AppError::Plugin(e.to_string()))?;

        // Enrich statuses with installed versions
        let installed = self
            .plugin_loader()
            .list_loaded()
            .map_err(|e| AppError::Plugin(e.to_string()))?;

        entries = entries
            .into_iter()
            .map(|e| {
                let installed_version = installed
                    .iter()
                    .find(|i| i.name() == e.name)
                    .map(|i| i.version().to_string());
                e.with_status(installed_version.as_deref())
            })
            .collect();

        write_cache(cache_path, &entries)?;
        Ok(())
    }
}

pub fn write_cache(path: &std::path::Path, entries: &[PluginStoreEntry]) -> Result<(), AppError> {
    use crate::application::read_models::plugin_store_view::PluginStoreEntryDto;

    let dtos: Vec<PluginStoreEntryDto> = entries.iter().cloned().map(Into::into).collect();
    let payload = serde_json::json!({
        "fetched_at": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        "plugins": dtos,
    });
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Plugin(e.to_string()))?;
    }
    let json =
        serde_json::to_string_pretty(&payload).map_err(|e| AppError::Plugin(e.to_string()))?;
    std::fs::write(path, json).map_err(|e| AppError::Plugin(e.to_string()))?;
    Ok(())
}

pub fn read_cache(path: &std::path::Path) -> Result<Vec<serde_json::Value>, AppError> {
    let content = std::fs::read_to_string(path).map_err(|e| AppError::Plugin(e.to_string()))?;
    let parsed: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| AppError::Plugin(e.to_string()))?;
    let plugins = parsed["plugins"].as_array().cloned().unwrap_or_default();
    Ok(plugins)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::plugin::PluginCategory;
    use crate::domain::model::plugin_store::{PluginStoreEntry, PluginStoreStatus};
    use tempfile::TempDir;

    fn make_entry(name: &str, version: &str) -> PluginStoreEntry {
        PluginStoreEntry {
            name: name.into(),
            description: "test".into(),
            author: "author".into(),
            version: version.into(),
            category: PluginCategory::Utility,
            repository: "https://github.com/a/b".into(),
            checksum_sha256: "abc".into(),
            checksum_sha256_toml: None,
            official: false,
            min_vortex_version: None,
            status: PluginStoreStatus::NotInstalled,
            installed_version: None,
        }
    }

    #[test]
    fn test_write_cache_creates_file_with_expected_structure() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("plugin-registry-cache.json");
        let entries = vec![make_entry("vortex-mod-gallery", "1.0.0")];

        write_cache(&cache, &entries).unwrap();
        assert!(cache.exists());

        let content = std::fs::read_to_string(&cache).unwrap();
        assert!(content.contains("vortex-mod-gallery"));
        assert!(content.contains("fetched_at"));
        assert!(content.contains("plugins"));
    }

    #[test]
    fn test_read_cache_returns_plugins_array() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("cache.json");
        let entries = vec![make_entry("plugin-a", "1.0.0")];
        write_cache(&cache, &entries).unwrap();

        let result = read_cache(&cache).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["name"], "plugin-a");
    }
}
