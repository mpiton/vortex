//! Implements [`PluginLoader`] using Extism and [`PluginRegistry`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::domain::error::DomainError;
use crate::domain::model::plugin::{PluginInfo, PluginManifest};
use crate::domain::ports::driven::PluginLoader;
use crate::domain::ports::driven::plugin_loader::DownloadedFileInfo;

use super::builtin::HttpModule;
use super::capabilities::{SharedHostResources, build_host_functions};
use super::manifest::{find_wasm_file, parse_manifest, parse_manifest_metadata};
use super::registry::{LoadedPlugin, PluginRegistry};

/// Per-plugin install coordination.
///
/// - `serializer` is held for the entire `load_from_dir` body so two
///   concurrent installs of the **same** plugin name can't race on the
///   staging/destination filesystem writes; the second one blocks until
///   the first completes.
/// - `count` is an independent refcount used by the watcher's
///   `is_install_in_progress` check. A refcount (rather than a boolean)
///   is needed because several installs for the same plugin can queue up
///   behind the serializer; the watcher must stay suppressed until the
///   **last** install finishes, not just the first.
struct InstallState {
    serializer: Mutex<()>,
    count: AtomicUsize,
}

impl InstallState {
    fn new() -> Self {
        Self {
            serializer: Mutex::new(()),
            count: AtomicUsize::new(0),
        }
    }
}

pub struct ExtismPluginLoader {
    registry: Arc<PluginRegistry>,
    plugins_dir: PathBuf,
    shared_resources: Arc<SharedHostResources>,
    builtin_http: HttpModule,
    /// Per-plugin install coordination. See [`InstallState`] for the
    /// two pieces of state it carries (serializer + refcount).
    installs: Arc<Mutex<HashMap<String, Arc<InstallState>>>>,
}

/// RAII guard: decrements the install refcount when dropped, so the
/// watcher's suppression window closes exactly when the install returns
/// (success, error, or panic) — never earlier, never later.
struct InstallInFlight {
    state: Arc<InstallState>,
}

impl Drop for InstallInFlight {
    fn drop(&mut self) {
        self.state.count.fetch_sub(1, Ordering::SeqCst);
    }
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
            installs: Arc::new(Mutex::new(HashMap::new())),
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

    /// Get or create the [`InstallState`] for a plugin name. The outer
    /// map mutex is held only long enough to clone the `Arc`; the
    /// returned state carries its own serializer.
    fn get_or_create_install_state(&self, name: &str) -> Arc<InstallState> {
        let mut map = self
            .installs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        map.entry(name.to_string())
            .or_insert_with(|| Arc::new(InstallState::new()))
            .clone()
    }

    /// Returns `true` if at least one install is currently in flight for
    /// `name`. The plugin watcher consults this to avoid reacting to
    /// events from the install's own filesystem writes.
    pub fn is_install_in_progress(&self, name: &str) -> bool {
        let map = self
            .installs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        map.get(name)
            .is_some_and(|s| s.count.load(Ordering::SeqCst) > 0)
    }

    /// Test-only: bump the install refcount for a plugin without going
    /// through `load_from_dir`, so watcher tests can assert that events
    /// are suppressed while an install is in flight.
    #[cfg(test)]
    pub fn mark_install_in_progress_for_testing(&self, name: &str) {
        let state = self.get_or_create_install_state(name);
        state.count.fetch_add(1, Ordering::SeqCst);
    }

    /// Test-only mirror of [`Self::mark_install_in_progress_for_testing`]
    /// used to exercise the refcount's "last one out wins" behaviour.
    #[cfg(test)]
    pub fn unmark_install_in_progress_for_testing(&self, name: &str) {
        let map = self
            .installs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(state) = map.get(name) {
            state.count.fetch_sub(1, Ordering::SeqCst);
        }
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

    fn call_url_plugin_function(&self, url: &str, func: &str) -> Result<String, DomainError> {
        let info = self.resolve_wasm_plugin(url)?;
        if !self.registry.function_exists(info.name(), func)? {
            return Err(DomainError::NotFound(format!(
                "plugin '{}' does not export '{func}'",
                info.name()
            )));
        }

        self.registry
            .call_plugin(info.name(), func, url)
            .map_err(|e| {
                DomainError::PluginError(format!("plugin '{}' {func} failed: {e}", info.name()))
            })
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

    fn find_installed_manifest(&self, name: &str) -> Result<Option<PluginInfo>, DomainError> {
        // Reject any name that could escape `plugins_dir/` so a hostile
        // caller can't read foreign manifests by passing `../etc/passwd`.
        if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(DomainError::ValidationError(format!(
                "invalid plugin name: '{name}'"
            )));
        }
        let dir = self.plugins_dir.join(name);
        if !dir.is_dir() {
            return Ok(None);
        }

        // Symlink containment: even though `name` itself is sanitized,
        // `plugins_dir/<name>` could be a symlink pointing outside the
        // root. We canonicalize both sides and require the resolved
        // plugin directory to live inside the resolved plugins root
        // before reading anything from it.
        let canon_root = self.plugins_dir.canonicalize().map_err(|e| {
            DomainError::PluginError(format!(
                "failed to canonicalize plugins_dir '{}': {e}",
                self.plugins_dir.display()
            ))
        })?;
        let canon_dir = dir.canonicalize().map_err(|e| {
            DomainError::PluginError(format!(
                "failed to canonicalize plugin dir '{}': {e}",
                dir.display()
            ))
        })?;
        if !canon_dir.starts_with(&canon_root) {
            return Err(DomainError::ValidationError(format!(
                "invalid plugin path outside plugins_dir: '{}'",
                dir.display()
            )));
        }

        // Use the metadata-only parser so a missing/corrupt `.wasm`
        // file doesn't hide the very plugin the user wants to report.
        match parse_manifest_metadata(&canon_dir) {
            Ok(manifest) => Ok(Some(manifest.info().clone())),
            Err(DomainError::PluginError(msg)) => {
                tracing::debug!("find_installed_manifest('{name}'): manifest unreadable: {msg}");
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), DomainError> {
        self.registry.set_enabled(name, enabled)
    }

    fn get_manifest(&self, name: &str) -> Result<Option<PluginManifest>, DomainError> {
        Ok(self.registry.manifest(name))
    }

    fn set_runtime_config(&self, name: &str, key: &str, value: &str) -> Result<(), DomainError> {
        self.shared_resources
            .plugin_configs()
            .entry(name.to_string())
            .or_default()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn extract_links(&self, url: &str) -> Result<String, DomainError> {
        self.call_url_plugin_function(url, "extract_links")
    }

    fn get_media_variants(&self, url: &str) -> Result<String, DomainError> {
        self.call_url_plugin_function(url, "get_media_variants")
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

    fn decrypt_container(&self, bytes: &[u8]) -> Result<String, DomainError> {
        // Sort by name so a deterministic plugin wins when several
        // container forks are loaded side-by-side.
        let mut infos: Vec<_> = self
            .registry
            .list_info()
            .into_iter()
            .filter(|i| i.is_enabled())
            .filter(|i| i.category() == crate::domain::model::plugin::PluginCategory::Container)
            .collect();
        infos.sort_by(|a, b| a.name().cmp(b.name()));
        let mut probe_error: Option<DomainError> = None;

        for info in &infos {
            match self.registry.function_exists(info.name(), "decrypt") {
                Ok(true) => {
                    return self
                        .registry
                        .call_plugin_bytes(info.name(), "decrypt", bytes)
                        .map_err(|e| {
                            DomainError::PluginError(format!(
                                "plugin '{}' decrypt failed: {e}",
                                info.name()
                            ))
                        });
                }
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(plugin = info.name(), error = %e, "decrypt probe failed");
                    if probe_error.is_none() {
                        probe_error = Some(DomainError::PluginError(format!(
                            "plugin '{}' decrypt probe failed: {e}",
                            info.name()
                        )));
                    }
                }
            }
        }

        if let Some(err) = probe_error {
            return Err(err);
        }
        Err(DomainError::NotFound("no container plugin loaded".into()))
    }

    fn load_from_dir(&self, dir: &std::path::Path) -> Result<(), DomainError> {
        let (manifest, _wasm_path) = parse_manifest(dir)?;
        let name = manifest.info().name().to_string();

        // Per-plugin install coordination:
        //
        //   1. Bump the refcount up-front so the watcher starts skipping
        //      events *immediately*, before any filesystem mutation.
        //   2. Take the per-plugin serializer lock so concurrent installs
        //      of the same plugin name can't interleave on the staging
        //      and destination directories.
        //
        // The `InstallInFlight` guard decrements the refcount on drop; the
        // local `_serializer_guard` releases the lock on drop. Both run
        // even on error or panic, so the state never leaks.
        let state = self.get_or_create_install_state(&name);
        state.count.fetch_add(1, Ordering::SeqCst);
        let _in_flight = InstallInFlight {
            state: state.clone(),
        };
        let _serializer_guard = state
            .serializer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Ensure any prior in-memory instance is cleared before we touch
        // the filesystem. Doing this inside the suppression window (after
        // the refcount bump) closes the gap that would otherwise let a
        // delayed watcher event re-insert the plugin between an external
        // `unload()` call and the start of this function, causing the
        // final `self.load()` below to fail with `AlreadyExists`.
        //
        // Only swallow `NotFound` — other errors (poisoned mutex, etc.)
        // must abort the install to avoid leaving the in-memory state
        // half-mutated.
        match self.unload(&name) {
            Ok(()) | Err(DomainError::NotFound(_)) => {}
            Err(e) => return Err(e),
        }

        // Copy staged files to the permanent plugins directory
        let dest_dir = self.plugins_dir.join(&name);
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
    fn test_overlapping_installs_keep_suppression_active_until_last_drop() {
        // The reason we track a refcount rather than a boolean flag: two
        // concurrent installs of the same plugin must both hold the
        // watcher's suppression active. If the first install's guard
        // cleared the flag while the second is still running, watcher
        // events would resume processing and could race the second
        // install's final `self.load()`.
        let tmp = TempDir::new().unwrap();
        let loader = ExtismPluginLoader::new(
            tmp.path().to_path_buf(),
            Arc::new(SharedHostResources::new()),
        )
        .unwrap();

        loader.mark_install_in_progress_for_testing("my-plugin");
        loader.mark_install_in_progress_for_testing("my-plugin");
        assert!(loader.is_install_in_progress("my-plugin"));

        // First install completes.
        loader.unmark_install_in_progress_for_testing("my-plugin");
        assert!(
            loader.is_install_in_progress("my-plugin"),
            "suppression must stay active while a second install is still running"
        );

        // Second install completes — suppression clears.
        loader.unmark_install_in_progress_for_testing("my-plugin");
        assert!(!loader.is_install_in_progress("my-plugin"));
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
    fn test_decrypt_container_returns_not_found_when_no_plugin_loaded() {
        let tmp = TempDir::new().unwrap();
        let loader = ExtismPluginLoader::new(
            tmp.path().to_path_buf(),
            Arc::new(SharedHostResources::new()),
        )
        .unwrap();

        let result = loader.decrypt_container(b"DLC\x00random");
        assert!(matches!(result, Err(DomainError::NotFound(_))));
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
        assert!(!is_adaptive_stream_error(
            "no format matches requested quality"
        ));
        assert!(!is_adaptive_stream_error(
            "yt-dlp failed (exit code 1): video unavailable"
        ));
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
