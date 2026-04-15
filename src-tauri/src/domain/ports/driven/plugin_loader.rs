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
}
