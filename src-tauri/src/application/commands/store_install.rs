//! Handlers for downloading and installing/updating plugins from the store.

use crate::application::command_bus::CommandBus;
use crate::application::commands::store_refresh::read_cache;
use crate::application::error::AppError;
use crate::application::read_models::plugin_store_view::PluginStoreEntryDto;

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
        let _entry_dto: PluginStoreEntryDto = raw
            .into_iter()
            .filter_map(|v| {
                serde_json::from_value(v)
                    .map_err(|e| tracing::warn!(%e, "skipping malformed cache entry"))
                    .ok()
            })
            .find(|dto: &PluginStoreEntryDto| dto.name == cmd.name)
            .ok_or_else(|| AppError::Plugin(format!("plugin '{}' not found in cache", cmd.name)))?;

        // Refetch the authoritative registry entry. The cache above is only a
        // UI index and must never be the source of `official` or checksum
        // trust decisions because it is user-writable.
        let plugin_name = cmd.name.clone();
        let download =
            tokio::task::spawn_blocking(move || client.download_store_plugin(&plugin_name))
                .await
                .map_err(|e| AppError::Plugin(format!("download task failed: {e}")))?
                .map_err(|e| AppError::Plugin(e.to_string()))?;
        let plugin_dir = download.directory;

        if let Some(ref min_ver) = download.min_vortex_version {
            let app_ver = env!("CARGO_PKG_VERSION");
            if !is_version_compatible(app_ver, min_ver) {
                let _ = std::fs::remove_dir_all(&plugin_dir);
                return Err(AppError::Plugin(format!(
                    "plugin '{}' requires Vortex >= {min_ver} (running {})",
                    cmd.name, app_ver
                )));
            }
        }

        // Parse manifest from the downloaded directory and load via the
        // plugin loader. `load_from_dir` performs its own idempotent
        // unload inside the install-in-progress suppression window, so
        // the handler no longer does an explicit pre-unload here — doing
        // it outside the window would leave a narrow gap during which a
        // delayed watcher event could re-insert the plugin and cause the
        // final `load()` to fail with `AlreadyExists`.
        let loader = self.plugin_loader_arc();
        let staging_for_cleanup = plugin_dir.clone();
        let install_result = tokio::task::spawn_blocking(move || match download.provenance {
            Some(provenance) => loader.load_official_from_dir(&plugin_dir, &provenance),
            None => loader.load_from_dir(&plugin_dir),
        })
        .await
        .map_err(|e| AppError::Plugin(format!("plugin install task failed: {e}")))
        .and_then(|result| result.map_err(AppError::from));

        // Staging is only meaningful until `load_from_dir` has copied the
        // files to the permanent plugins directory. After that the staged
        // copy is dead weight that would otherwise accumulate one
        // subdirectory per install. Best-effort — a leftover here doesn't
        // break future installs (the store client overwrites on next
        // download) so we log and continue.
        if let Err(e) = std::fs::remove_dir_all(&staging_for_cleanup) {
            tracing::warn!(
                plugin = %cmd.name,
                path = %staging_for_cleanup.display(),
                error = %e,
                "failed to clean up plugin staging dir after install",
            );
        }

        install_result?;

        tracing::info!(plugin = %cmd.name, "plugin installed from store");
        Ok(())
    }

    /// Unload the current version and install the latest from the registry.
    pub async fn handle_store_update(
        &self,
        cmd: StoreUpdateCommand,
        cache_path: &std::path::Path,
    ) -> Result<(), AppError> {
        // `handle_store_install` already unloads the previous instance
        // before loading, so update is just a re-install with a clearer
        // intent-telegraphing name.
        self.handle_store_install(StoreInstallCommand { name: cmd.name }, cache_path)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::commands::store_refresh::write_cache;
    use crate::domain::error::DomainError;
    use crate::domain::model::plugin::{PluginCategory, PluginInfo, PluginManifest};
    use crate::domain::model::plugin_store::{PluginStoreEntry, PluginStoreStatus};
    use crate::domain::ports::driven::plugin_store_client::StorePluginDownload;
    use crate::domain::ports::driven::{PluginLoader, PluginStoreClient};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use tempfile::TempDir;

    struct StagingStoreClient {
        directory: PathBuf,
    }

    impl PluginStoreClient for StagingStoreClient {
        fn fetch_registry(&self) -> Result<Vec<PluginStoreEntry>, DomainError> {
            Ok(Vec::new())
        }

        fn download_plugin(&self, _entry: &PluginStoreEntry) -> Result<PathBuf, DomainError> {
            Ok(self.directory.clone())
        }

        fn download_store_plugin(&self, _name: &str) -> Result<StorePluginDownload, DomainError> {
            Ok(StorePluginDownload {
                directory: self.directory.clone(),
                provenance: None,
                min_vortex_version: None,
            })
        }
    }

    enum LoaderFailure {
        Error,
        Panic,
    }

    struct FailingDirectoryLoader {
        failure: LoaderFailure,
    }

    impl PluginLoader for FailingDirectoryLoader {
        fn load(&self, _manifest: &PluginManifest) -> Result<(), DomainError> {
            Ok(())
        }

        fn load_from_dir(&self, _dir: &Path) -> Result<(), DomainError> {
            match self.failure {
                LoaderFailure::Error => Err(DomainError::PluginError("load failed".into())),
                LoaderFailure::Panic => panic!("loader panicked"),
            }
        }

        fn unload(&self, _name: &str) -> Result<(), DomainError> {
            Ok(())
        }

        fn resolve_url(&self, _url: &str) -> Result<Option<PluginInfo>, DomainError> {
            Ok(None)
        }

        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
            Ok(Vec::new())
        }

        fn set_enabled(&self, _name: &str, _enabled: bool) -> Result<(), DomainError> {
            Ok(())
        }
    }

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

    async fn install_with_failure(failure: LoaderFailure) -> AppError {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("cache.json");
        write_cache(&cache, &[make_entry("my-plugin", "1.0.0")]).unwrap();

        let staging = tmp.path().join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("plugin.wasm"), b"wasm").unwrap();

        let bus = crate::application::test_support::make_store_command_bus(
            Arc::new(FailingDirectoryLoader { failure }),
            Arc::new(StagingStoreClient {
                directory: staging.clone(),
            }),
        );

        let result = bus
            .handle_store_install(
                StoreInstallCommand {
                    name: "my-plugin".into(),
                },
                &cache,
            )
            .await;

        let error = result.expect_err("installation must propagate the loader failure");
        assert!(
            !staging.exists(),
            "staging directory must be removed before the install error propagates"
        );
        error
    }

    #[tokio::test]
    async fn test_store_install_cleans_staging_after_loader_error() {
        let error = install_with_failure(LoaderFailure::Error).await;
        assert!(matches!(
            error,
            AppError::Domain(DomainError::PluginError(message)) if message == "load failed"
        ));
    }

    #[tokio::test]
    async fn test_store_install_cleans_staging_after_join_error() {
        let error = install_with_failure(LoaderFailure::Panic).await;
        assert!(matches!(
            error,
            AppError::Plugin(message) if message.starts_with("plugin install task failed:")
        ));
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
