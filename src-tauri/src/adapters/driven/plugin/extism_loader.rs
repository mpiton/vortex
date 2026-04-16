//! Implements [`PluginLoader`] using Extism and [`PluginRegistry`].

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::error::DomainError;
use crate::domain::model::plugin::{PluginInfo, PluginManifest};
use crate::domain::ports::driven::plugin_loader::DownloadedFileInfo;
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

    fn resolve_wasm_plugin(&self, url: &str) -> Result<PluginInfo, DomainError> {
        let info = self
            .resolve_url(url)?
            .ok_or_else(|| DomainError::PluginError(format!("no plugin can handle URL: {url}")))?;
        if info.name() == "builtin-http" {
            return Err(DomainError::NotFound("builtin-http".into()));
        }
        Ok(info)
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
        let info = self.resolve_wasm_plugin(url)?;

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
                let msg = e.to_string();
                if is_adaptive_stream_error(&msg) {
                    DomainError::AdaptiveStreamOnly
                } else {
                    DomainError::PluginError(format!(
                        "plugin '{}' resolve_stream_url failed: {msg}",
                        info.name()
                    ))
                }
            })
    }

    fn download_to_file(
        &self,
        url: &str,
        quality: &str,
        format: &str,
        output_dir: &str,
        audio_only: bool,
    ) -> Result<DownloadedFileInfo, DomainError> {
        let info = self.resolve_wasm_plugin(url)?;

        let input = serde_json::json!({
            "url": url,
            "quality": quality,
            "format": format,
            "output_dir": output_dir,
            "audio_only": audio_only,
        })
        .to_string();

        let path_str = self
            .registry
            .call_plugin(info.name(), "download_to_file", &input)
            .map_err(|e| {
                DomainError::PluginError(format!(
                    "plugin '{}' download_to_file failed: {e}",
                    info.name()
                ))
            })?;

        let path = std::path::PathBuf::from(path_str.trim());

        // Validate the returned path is within output_dir (path traversal protection).
        let canon_output = std::path::Path::new(output_dir)
            .canonicalize()
            .map_err(|e| DomainError::StorageError(format!("output_dir invalid: {e}")))?;
        let canon_path = path
            .canonicalize()
            .map_err(|e| DomainError::StorageError(format!("returned path invalid: {e}")))?;
        if !canon_path.starts_with(&canon_output) {
            return Err(DomainError::ValidationError(format!(
                "plugin returned path outside output_dir: {}",
                path.display()
            )));
        }

        let size = std::fs::metadata(&canon_path)
            .map_err(|e| DomainError::StorageError(format!("failed to stat downloaded file: {e}")))?
            .len();

        Ok(DownloadedFileInfo {
            path: canon_path,
            size,
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

/// Returns `true` if the plugin error message indicates an adaptive-only stream.
///
/// ⚠ **Fragile coupling**: this is a human-readable-string contract with plugin
/// authors. It matches the substring `"adaptive stream (HLS/DASH)"` emitted by
/// `vortex-mod-youtube ≥ 1.2.0`'s `PluginError::AdaptiveStreamOnly`. If the
/// plugin wording drifts or another plugin reuses `PluginError::AdaptiveStreamOnly`
/// with different text, the 1080p DASH fallback silently breaks (error maps to
/// `PluginError` instead of `DomainError::AdaptiveStreamOnly` and
/// `download_media_start` never invokes `download_to_file`).
///
/// A structured sentinel (e.g. plugin returns `{"error_code":"adaptive_stream_only"}`)
/// would be a more robust contract — tracked for a future plugin API iteration.
/// For now, the parenthesised `(HLS/DASH)` qualifier is matched instead of the
/// bare `"adaptive stream"` token to reduce the risk of false positives from
/// unrelated error messages.
fn is_adaptive_stream_error(msg: &str) -> bool {
    msg.contains("adaptive stream (HLS/DASH)")
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
    fn test_resolve_stream_url_maps_adaptive_stream_error() {
        let msg = "video is only available as an adaptive stream (HLS/DASH) at this quality; try 360p or 480p for a direct download";
        assert!(is_adaptive_stream_error(msg));
    }

    #[test]
    fn test_resolve_stream_url_does_not_map_other_errors() {
        assert!(!is_adaptive_stream_error("no format matches requested quality"));
        assert!(!is_adaptive_stream_error("yt-dlp failed (exit code 1): video unavailable"));
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
