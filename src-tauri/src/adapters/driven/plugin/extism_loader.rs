//! Implements [`PluginLoader`] using Extism and [`PluginRegistry`].

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::error::DomainError;
use crate::domain::model::plugin::{PluginInfo, PluginManifest};
use crate::domain::ports::driven::PluginLoader;

use super::builtin::HttpModule;
use super::capabilities::{SharedHostResources, build_host_functions};
use super::manifest::{find_wasm_file, parse_manifest};
use super::registry::{LoadedPlugin, PluginRegistry};

pub struct ExtismPluginLoader {
    registry: Arc<PluginRegistry>,
    plugins_dir: PathBuf,
    shared_resources: Arc<SharedHostResources>,
    builtin_http: HttpModule,
}

impl ExtismPluginLoader {
    pub fn new(
        plugins_dir: PathBuf,
        shared_resources: Arc<SharedHostResources>,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            registry: Arc::new(PluginRegistry::new()),
            plugins_dir,
            shared_resources,
            builtin_http: HttpModule::new()?,
        })
    }

    pub fn registry(&self) -> &Arc<PluginRegistry> {
        &self.registry
    }

    pub fn plugins_dir(&self) -> &Path {
        &self.plugins_dir
    }

    pub fn builtin_http(&self) -> &HttpModule {
        &self.builtin_http
    }
}

impl PluginLoader for ExtismPluginLoader {
    fn load(&self, manifest: &PluginManifest) -> Result<(), DomainError> {
        let name = manifest.info().name().to_string();

        // Reject names containing path separators or traversal sequences
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(DomainError::ValidationError(format!(
                "invalid plugin name: '{name}'"
            )));
        }

        // Derive wasm path directly from convention: plugins_dir/<name>/<name>.wasm
        let plugin_dir = self.plugins_dir.join(&name);
        let wasm_path = find_wasm_file(&plugin_dir)?;

        const MAX_WASM_SIZE: u64 = 100 * 1024 * 1024; // 100 MB
        let metadata = std::fs::metadata(&wasm_path).map_err(|e| {
            DomainError::PluginError(format!("failed to stat wasm {}: {e}", wasm_path.display()))
        })?;
        if metadata.len() > MAX_WASM_SIZE {
            return Err(DomainError::PluginError(format!(
                "wasm file {} exceeds 100 MB limit ({} bytes)",
                wasm_path.display(),
                metadata.len()
            )));
        }
        let wasm_bytes = std::fs::read(&wasm_path).map_err(|e| {
            DomainError::PluginError(format!("failed to read wasm {}: {e}", wasm_path.display()))
        })?;

        let extism_manifest = extism::Manifest::new([extism::Wasm::data(wasm_bytes)]);
        let host_functions = build_host_functions(manifest, &self.shared_resources);
        let plugin = extism::Plugin::new(&extism_manifest, host_functions, true)
            .map_err(|e| DomainError::PluginError(format!("failed to load plugin: {e}")))?;

        let loaded = LoadedPlugin {
            manifest: manifest.clone(),
            plugin: std::sync::Arc::new(std::sync::Mutex::new(plugin)),
            enabled: true,
        };

        // Atomic insert-if-absent via DashMap::entry()
        if self.registry.try_insert(name.clone(), loaded) {
            Ok(())
        } else {
            Err(DomainError::AlreadyExists(name))
        }
    }

    fn unload(&self, name: &str) -> Result<(), DomainError> {
        self.registry
            .remove(name)
            .map(|_| ())
            .ok_or_else(|| DomainError::NotFound(name.to_string()))
    }

    fn resolve_url(&self, url: &str) -> Result<Option<PluginInfo>, DomainError> {
        let mut infos: Vec<_> = self
            .registry
            .list_info()
            .into_iter()
            .filter(|i| i.is_enabled())
            .collect();
        infos.sort_by(|a, b| a.name().cmp(b.name()));
        for info in infos {
            let name = info.name().to_string();
            match self.registry.call_plugin(&name, "can_handle", url) {
                Ok(result) if result.trim() == "true" => return Ok(Some(info)),
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("plugin '{name}' failed can_handle call: {e}");
                }
            }
        }
        // Fallback: built-in HTTP module handles http://, https://
        if HttpModule::can_handle(url) {
            return Ok(Some(HttpModule::plugin_info()));
        }
        Ok(None)
    }

    fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
        Ok(self.registry.list_info())
    }

    fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), DomainError> {
        self.registry.set_enabled(name, enabled)
    }

    fn resolve_stream_url(
        &self,
        url: &str,
        quality: &str,
        format: &str,
        audio_only: bool,
    ) -> Result<String, DomainError> {
        // Find the plugin that claims this URL.
        let info = self
            .resolve_url(url)?
            .ok_or_else(|| DomainError::PluginError(format!("no plugin can handle URL: {url}")))?;

        // The built-in HTTP module handles direct URLs — no resolution needed.
        if info.name() == "builtin-http" {
            return Err(DomainError::NotFound("builtin-http".into()));
        }

        let input = serde_json::json!({
            "url": url,
            "quality": quality,
            "format": format,
            "audio_only": audio_only,
        })
        .to_string();

        self.registry
            .call_plugin(info.name(), "resolve_stream_url", &input)
            .map_err(|e| {
                DomainError::PluginError(format!(
                    "plugin '{}' resolve_stream_url failed: {e}",
                    info.name()
                ))
            })
    }

    fn load_from_dir(&self, dir: &std::path::Path) -> Result<(), DomainError> {
        let (manifest, _wasm_path) = parse_manifest(dir)?;
        let name = manifest.info().name();

        // Copy staged files to the permanent plugins directory
        let dest_dir = self.plugins_dir.join(name);
        if dest_dir.exists() {
            std::fs::remove_dir_all(&dest_dir).map_err(|e| {
                DomainError::PluginError(format!(
                    "failed to remove existing plugin dir '{}': {e}",
                    dest_dir.display()
                ))
            })?;
        }
        std::fs::create_dir_all(&dest_dir)
            .map_err(|e| DomainError::PluginError(format!("failed to create plugin dir: {e}")))?;

        for entry in std::fs::read_dir(dir)
            .map_err(|e| DomainError::PluginError(format!("failed to read staging dir: {e}")))?
        {
            let entry = entry
                .map_err(|e| DomainError::PluginError(format!("staging dir entry error: {e}")))?;
            let src = entry.path();
            if src.is_file() {
                let dest = dest_dir.join(entry.file_name());
                std::fs::copy(&src, &dest).map_err(|e| {
                    DomainError::PluginError(format!(
                        "failed to copy {} → {}: {e}",
                        src.display(),
                        dest.display()
                    ))
                })?;
            }
        }

        self.load(&manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::plugin::{PluginCategory, PluginInfo, PluginManifest};
    use std::io::Write;
    use tempfile::TempDir;

    fn make_manifest(name: &str) -> PluginManifest {
        let info = PluginInfo::new(
            name.to_string(),
            "1.0.0".to_string(),
            "Test plugin".to_string(),
            "tester".to_string(),
            PluginCategory::Utility,
        );
        PluginManifest::new(info)
    }

    fn setup_plugin_dir(plugins_dir: &Path, name: &str) {
        let plugin_dir = plugins_dir.join(name);
        std::fs::create_dir_all(&plugin_dir).unwrap();

        let toml_content = format!(
            r#"[plugin]
name = "{name}"
version = "1.0.0"
category = "utility"
author = "tester"
description = "Test plugin"
"#
        );
        let mut f = std::fs::File::create(plugin_dir.join("plugin.toml")).unwrap();
        f.write_all(toml_content.as_bytes()).unwrap();

        // Write minimal valid WASM binary
        let wasm_bytes: &[u8] = &[0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
        let mut wf = std::fs::File::create(plugin_dir.join(format!("{name}.wasm"))).unwrap();
        wf.write_all(wasm_bytes).unwrap();
    }

    #[test]
    fn test_load_nonexistent_wasm() {
        let tmp = TempDir::new().unwrap();
        let loader = ExtismPluginLoader::new(
            tmp.path().to_path_buf(),
            Arc::new(SharedHostResources::new()),
        )
        .unwrap();
        let manifest = make_manifest("ghost-plugin");

        // Plugin dir doesn't exist — should fail
        let result = loader.load(&manifest);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DomainError::PluginError(_)));
    }

    #[test]
    fn test_unload_not_found() {
        let tmp = TempDir::new().unwrap();
        let loader = ExtismPluginLoader::new(
            tmp.path().to_path_buf(),
            Arc::new(SharedHostResources::new()),
        )
        .unwrap();

        let result = loader.unload("nonexistent");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DomainError::NotFound(_)));
    }

    #[test]
    fn test_resolve_url_no_plugins_returns_none() {
        let tmp = TempDir::new().unwrap();
        let loader = ExtismPluginLoader::new(
            tmp.path().to_path_buf(),
            Arc::new(SharedHostResources::new()),
        )
        .unwrap();

        // magnet: scheme is not handled by any built-in module
        let result = loader.resolve_url("magnet:?xt=urn:btih:abc123");
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_resolve_url_builtin_http_fallback() {
        let tmp = TempDir::new().unwrap();
        let loader = ExtismPluginLoader::new(
            tmp.path().to_path_buf(),
            Arc::new(SharedHostResources::new()),
        )
        .unwrap();

        let result = loader.resolve_url("https://example.com/file.zip");
        assert!(result.is_ok());
        let info = result.unwrap().expect("expected Some(PluginInfo)");
        assert_eq!(info.name(), "builtin-http");
    }

    #[test]
    fn test_resolve_url_ftp_scheme_returns_none() {
        let tmp = TempDir::new().unwrap();
        let loader = ExtismPluginLoader::new(
            tmp.path().to_path_buf(),
            Arc::new(SharedHostResources::new()),
        )
        .unwrap();

        let result = loader.resolve_url("ftp://ftp.example.com/file.tar.gz");
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_list_loaded_empty() {
        let tmp = TempDir::new().unwrap();
        let loader = ExtismPluginLoader::new(
            tmp.path().to_path_buf(),
            Arc::new(SharedHostResources::new()),
        )
        .unwrap();

        let result = loader.list_loaded();
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_load_already_loaded_returns_error() {
        let tmp = TempDir::new().unwrap();
        setup_plugin_dir(tmp.path(), "dup-plugin");
        let loader = ExtismPluginLoader::new(
            tmp.path().to_path_buf(),
            Arc::new(SharedHostResources::new()),
        )
        .unwrap();
        let manifest = make_manifest("dup-plugin");

        loader.load(&manifest).unwrap();
        let result = loader.load(&manifest);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DomainError::AlreadyExists(_)));
    }

    #[test]
    fn test_unload_after_load() {
        let tmp = TempDir::new().unwrap();
        setup_plugin_dir(tmp.path(), "removable-plugin");
        let loader = ExtismPluginLoader::new(
            tmp.path().to_path_buf(),
            Arc::new(SharedHostResources::new()),
        )
        .unwrap();
        let manifest = make_manifest("removable-plugin");

        loader.load(&manifest).unwrap();
        assert_eq!(loader.list_loaded().unwrap().len(), 1);

        loader.unload("removable-plugin").unwrap();
        assert_eq!(loader.list_loaded().unwrap().len(), 0);
    }

    #[test]
    fn test_load_from_dir_copies_staging_files() {
        let tmp = TempDir::new().unwrap();
        let plugins_dir = tmp.path().join("plugins");
        let staging_dir = tmp.path().join("staging");
        std::fs::create_dir_all(&staging_dir).unwrap();

        let loader =
            ExtismPluginLoader::new(plugins_dir.clone(), Arc::new(SharedHostResources::new()))
                .unwrap();

        // Set up the staged plugin directory
        setup_plugin_dir(&staging_dir, "test-plugin");
        let staged = staging_dir.join("test-plugin");

        // load_from_dir should copy to plugins_dir/test-plugin/ and then load
        // (Loading will fail due to minimal WASM — but the copy should succeed)
        let _ = loader.load_from_dir(&staged);

        // Verify files were copied to the permanent plugins directory
        assert!(plugins_dir.join("test-plugin").join("plugin.toml").exists());
        assert!(
            plugins_dir
                .join("test-plugin")
                .join("test-plugin.wasm")
                .exists()
        );
    }
}
