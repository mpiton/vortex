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

    /// Unload a plugin by name.
    fn unload(&self, name: &str) -> Result<(), DomainError>;

    /// Ask loaded plugins which one can handle the given URL.
    ///
    /// Returns `None` if no plugin claims the URL (falls back to
    /// the built-in HTTP module).
    fn resolve_url(&self, url: &str) -> Result<Option<PluginInfo>, DomainError>;

    /// List all currently loaded plugins.
    fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError>;
}
