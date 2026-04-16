//! Port for loading and managing WASM plugins.
//!
//! Handles plugin lifecycle (load, unload) and URL resolution
//! to determine which plugin can handle a given URL.

use crate::domain::error::DomainError;
use crate::domain::model::plugin::{PluginInfo, PluginManifest};

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
}
