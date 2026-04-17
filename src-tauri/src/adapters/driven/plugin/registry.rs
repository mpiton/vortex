//! In-memory plugin registry backed by a [`DashMap`].

use std::sync::{Arc, Mutex};

use dashmap::DashMap;

use crate::domain::error::DomainError;
use crate::domain::model::plugin::{PluginInfo, PluginManifest};
use crate::domain::ports::driven::PluginReadRepository;

pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub plugin: Arc<Mutex<extism::Plugin>>,
    pub enabled: bool,
}

pub struct PluginRegistry {
    plugins: DashMap<String, LoadedPlugin>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: DashMap::new(),
        }
    }

    /// Unconditional insert (no duplicate check). Used in tests for setup.
    #[cfg(test)]
    pub fn insert(&self, name: String, loaded: LoadedPlugin) {
        self.plugins.insert(name, loaded);
    }

    /// Atomically insert only if the key is absent. Returns true on success.
    pub fn try_insert(&self, name: String, loaded: LoadedPlugin) -> bool {
        use dashmap::mapref::entry::Entry;
        match self.plugins.entry(name) {
            Entry::Vacant(vacant) => {
                vacant.insert(loaded);
                true
            }
            Entry::Occupied(_) => false,
        }
    }

    pub fn remove(&self, name: &str) -> Option<(String, LoadedPlugin)> {
        self.plugins.remove(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }

    /// Returns info for all plugins (enabled and disabled).
    pub fn list_info(&self) -> Vec<PluginInfo> {
        self.plugins
            .iter()
            .map(|entry| {
                let mut info = entry.manifest.info().clone();
                if !entry.enabled {
                    info.disable();
                }
                info
            })
            .collect()
    }

    pub fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), DomainError> {
        let mut entry = self
            .plugins
            .get_mut(name)
            .ok_or_else(|| DomainError::NotFound(name.to_string()))?;
        entry.enabled = enabled;
        Ok(())
    }

    pub fn call_plugin(&self, name: &str, func: &str, input: &str) -> Result<String, DomainError> {
        // Clone the Arc<Mutex<Plugin>> and drop the DashMap shard guard
        // before locking. This prevents holding the shard during slow WASM execution.
        let plugin_handle = {
            let entry = self
                .plugins
                .get(name)
                .ok_or_else(|| DomainError::NotFound(name.to_string()))?;
            Arc::clone(&entry.plugin)
        }; // DashMap shard guard dropped here
        let mut plugin = plugin_handle
            .lock()
            .map_err(|_| DomainError::PluginError(format!("plugin '{name}' mutex poisoned")))?;
        let fn_exists = plugin.function_exists(func);
        tracing::info!(plugin = name, func, fn_exists, "call_plugin pre-call");
        let result = plugin.call::<&str, &str>(func, input).map_err(|e| {
            DomainError::PluginError(format!(
                "plugin call failed (function_exists={fn_exists}): {e}"
            ))
        })?;
        Ok(result.to_string())
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginReadRepository for PluginRegistry {
    fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
        Ok(self.list_info())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::plugin::{PluginCategory, PluginInfo, PluginManifest};

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

    /// Create a minimal extism plugin from a hardcoded empty WASM module.
    fn make_extism_plugin() -> extism::Plugin {
        // Minimal valid WASM binary: magic + version (8 bytes)
        let wasm_bytes: &[u8] = &[0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
        let manifest = extism::Manifest::new([extism::Wasm::data(wasm_bytes)]);
        extism::Plugin::new(&manifest, [], true).expect("extism plugin creation failed")
    }

    fn make_loaded(name: &str) -> LoadedPlugin {
        LoadedPlugin {
            manifest: make_manifest(name),
            plugin: Arc::new(Mutex::new(make_extism_plugin())),
            enabled: true,
        }
    }

    #[test]
    fn test_insert_and_list() {
        let registry = PluginRegistry::new();
        registry.insert("plug-a".to_string(), make_loaded("plug-a"));
        registry.insert("plug-b".to_string(), make_loaded("plug-b"));

        let infos = registry.list_info();
        assert_eq!(infos.len(), 2);
        let names: Vec<&str> = infos.iter().map(|i| i.name()).collect();
        assert!(names.contains(&"plug-a"));
        assert!(names.contains(&"plug-b"));
    }

    #[test]
    fn test_remove() {
        let registry = PluginRegistry::new();
        registry.insert("plug-a".to_string(), make_loaded("plug-a"));
        assert!(registry.contains("plug-a"));

        let removed = registry.remove("plug-a");
        assert!(removed.is_some());
        assert!(!registry.contains("plug-a"));
    }

    #[test]
    fn test_contains() {
        let registry = PluginRegistry::new();
        assert!(!registry.contains("missing"));
        registry.insert("present".to_string(), make_loaded("present"));
        assert!(registry.contains("present"));
    }

    #[test]
    fn test_set_enabled() {
        let registry = PluginRegistry::new();
        registry.insert("plug-a".to_string(), make_loaded("plug-a"));

        registry.set_enabled("plug-a", false).unwrap();
        let infos = registry.list_info();
        let info = infos.iter().find(|i| i.name() == "plug-a").unwrap();
        assert!(!info.is_enabled());

        registry.set_enabled("plug-a", true).unwrap();
        let infos = registry.list_info();
        let info = infos.iter().find(|i| i.name() == "plug-a").unwrap();
        assert!(info.is_enabled());
    }

    #[test]
    fn test_set_enabled_not_found() {
        let registry = PluginRegistry::new();
        let result = registry.set_enabled("ghost", false);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DomainError::NotFound(_)));
    }

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PluginRegistry>();
    }

    #[test]
    fn test_plugin_read_repository_impl() {
        let registry = PluginRegistry::new();
        registry.insert("plug-a".to_string(), make_loaded("plug-a"));

        let result = registry.list_loaded();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }
}
