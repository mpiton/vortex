//! Port for loading and managing WASM plugins.
//!
//! Handles plugin lifecycle (load, unload) and URL resolution
//! to determine which plugin can handle a given URL.

use crate::domain::error::DomainError;
use crate::domain::model::plugin::{PluginInfo, PluginManifest};

/// Result of a `download_to_file` plugin call.
pub struct DownloadedFileInfo {
    /// Absolute path to the merged output file on the host filesystem.
    pub path: std::path::PathBuf,
    /// File size in bytes (obtained from host `std::fs::metadata`).
    pub size: u64,
}

/// Manages WASM plugin lifecycle and URL resolution.
///
/// The adapter implementation uses Extism to load `.wasm` files,
/// validate manifests, and call plugin functions. Supports
/// hot-reloading via file watcher.
pub trait PluginLoader: Send + Sync {
    /// Load a plugin from its manifest.
    fn load(&self, manifest: &PluginManifest) -> Result<(), DomainError>;

    /// Parse the manifest from a plugin directory and load the plugin.
    ///
    /// The default implementation returns an error; adapter implementations
    /// that support directory-based loading should override this method.
    fn load_from_dir(&self, _dir: &std::path::Path) -> Result<(), DomainError> {
        Err(DomainError::PluginError(
            "load_from_dir not supported by this loader".into(),
        ))
    }

    /// Unload a plugin by name.
    fn unload(&self, name: &str) -> Result<(), DomainError>;

    /// Ask loaded plugins which one can handle the given URL.
    ///
    /// Returns `None` if no plugin claims the URL (falls back to
    /// the built-in HTTP module).
    fn resolve_url(&self, url: &str) -> Result<Option<PluginInfo>, DomainError>;

    /// List all currently loaded plugins.
    fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError>;

    /// Enable or disable a loaded plugin by name.
    fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), DomainError>;

    /// Resolve a media URL to a direct CDN stream URL via the plugin that
    /// claims the URL.
    ///
    /// The plugin's `resolve_stream_url` export is called with a JSON payload
    /// `{ "url", "quality", "format", "audio_only" }`. Returns the raw CDN URL
    /// string that the download engine can fetch directly.
    ///
    /// Returns `Err(DomainError::PluginError)` if no plugin claims the URL or
    /// if the plugin's `resolve_stream_url` fails. Returns
    /// `Err(DomainError::NotFound)` when the URL is claimed by the built-in
    /// HTTP module which does not need resolution (callers should treat the
    /// original URL as already downloadable).
    ///
    /// The default implementation returns `NotFound` (treat URL as-is). Override
    /// in adapter implementations that back WASM plugins.
    fn resolve_stream_url(
        &self,
        _url: &str,
        _quality: &str,
        _format: &str,
        _audio_only: bool,
    ) -> Result<String, DomainError> {
        Err(DomainError::NotFound(
            "resolve_stream_url not supported by this loader".into(),
        ))
    }

    /// Download a video/audio file using the plugin's native download+merge
    /// pipeline (e.g. yt-dlp DASH). Used as fallback when `resolve_stream_url`
    /// returns `AdaptiveStreamOnly`.
    ///
    /// Returns `Err(DomainError::NotFound)` by default (adapters that do not
    /// support this operation rely on the default).
    fn download_to_file(
        &self,
        _url: &str,
        _quality: &str,
        _format: &str,
        _output_dir: &str,
        _audio_only: bool,
    ) -> Result<DownloadedFileInfo, DomainError> {
        Err(DomainError::NotFound(
            "download_to_file not supported by this loader".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::error::DomainError;
    use crate::domain::model::plugin::{PluginInfo, PluginManifest};

    struct MinimalLoader;
    impl PluginLoader for MinimalLoader {
        fn load(&self, _: &PluginManifest) -> Result<(), DomainError> { Ok(()) }
        fn unload(&self, _: &str) -> Result<(), DomainError> { Ok(()) }
        fn resolve_url(&self, _: &str) -> Result<Option<PluginInfo>, DomainError> { Ok(None) }
        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> { Ok(vec![]) }
        fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> { Ok(()) }
    }

    #[test]
    fn test_download_to_file_default_returns_not_found() {
        let loader = MinimalLoader;
        let result = loader.download_to_file("https://youtu.be/x", "1080p", "mp4", "/tmp", false);
        assert!(matches!(result, Err(DomainError::NotFound(_))));
    }
}
