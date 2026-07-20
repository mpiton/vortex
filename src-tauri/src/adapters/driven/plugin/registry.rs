//! In-memory plugin registry backed by a [`DashMap`].

use std::sync::{Arc, Mutex};

use dashmap::DashMap;

use crate::domain::error::DomainError;
use crate::domain::model::credential::Credential;
use crate::domain::model::plugin::{PluginInfo, PluginManifest};
use crate::domain::ports::driven::PluginReadRepository;

use super::capabilities::CredentialSlot;

struct CredentialScope {
    slot: CredentialSlot,
}

impl CredentialScope {
    fn new(slot: CredentialSlot, credential: Credential) -> Result<Self, DomainError> {
        {
            let mut current = slot.lock().map_err(|_| {
                DomainError::PluginError("plugin credential slot mutex poisoned".into())
            })?;
            if current.is_some() {
                return Err(DomainError::PluginError(
                    "plugin credential slot already active".into(),
                ));
            }
            *current = Some(credential);
            slot.mark_exposed();
        }
        Ok(Self { slot })
    }
}

impl Drop for CredentialScope {
    fn drop(&mut self) {
        match self.slot.lock() {
            Ok(mut current) => *current = None,
            Err(poisoned) => *poisoned.into_inner() = None,
        }
    }
}

pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub plugin: Arc<Mutex<extism::Plugin>>,
    pub credential_slot: CredentialSlot,
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

    /// Clone the full manifest of a loaded plugin so callers (like the
    /// configuration query handler) can inspect its `[config]` schema
    /// without holding a registry reference.
    pub fn manifest(&self, name: &str) -> Option<PluginManifest> {
        self.plugins.get(name).map(|entry| entry.manifest.clone())
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

    pub fn function_exists(&self, name: &str, func: &str) -> Result<bool, DomainError> {
        let plugin_handle = {
            let entry = self
                .plugins
                .get(name)
                .ok_or_else(|| DomainError::NotFound(name.to_string()))?;
            Arc::clone(&entry.plugin)
        };
        let plugin = plugin_handle
            .lock()
            .map_err(|_| DomainError::PluginError(format!("plugin '{name}' mutex poisoned")))?;
        Ok(plugin.function_exists(func))
    }

    pub fn call_plugin(&self, name: &str, func: &str, input: &str) -> Result<String, DomainError> {
        self.call_plugin_inner(name, func, input, None, None)
    }

    pub(crate) fn call_plugin_capped(
        &self,
        name: &str,
        func: &str,
        input: &str,
        output_limit: usize,
    ) -> Result<String, DomainError> {
        self.call_plugin_inner(name, func, input, None, Some(output_limit))
    }

    pub fn call_plugin_with_credential(
        &self,
        name: &str,
        func: &str,
        input: &str,
        credential: Credential,
    ) -> Result<String, DomainError> {
        self.call_plugin_inner(name, func, input, Some(credential), None)
    }

    /// Container plugins decode binary blobs (DLC / CCF / RSDF / Metalink);
    /// shipping them as `&str` would lossy-convert non-UTF-8 bytes.
    pub fn call_plugin_bytes(
        &self,
        name: &str,
        func: &str,
        input: &[u8],
    ) -> Result<String, DomainError> {
        self.call_plugin_inner(name, func, input, None, None)
    }

    fn call_plugin_inner<'a, I>(
        &self,
        name: &str,
        func: &str,
        input: I,
        scoped_credential: Option<Credential>,
        output_limit: Option<usize>,
    ) -> Result<String, DomainError>
    where
        I: extism::convert::ToBytes<'a>,
    {
        let plugin_handle = {
            let entry = self
                .plugins
                .get(name)
                .ok_or_else(|| DomainError::NotFound(name.to_string()))?;
            if !entry.enabled {
                return Err(DomainError::NotFound(format!(
                    "plugin '{name}' is disabled"
                )));
            }
            Arc::clone(&entry.plugin)
        };
        let mut plugin = plugin_handle
            .lock()
            .map_err(|_| DomainError::PluginError(format!("plugin '{name}' mutex poisoned")))?;
        // Re-check after taking the per-plugin lock. The short registry guard
        // makes enablement and credential injection atomic without pinning a
        // DashMap shard during the WASM call.
        let _credential_scope = {
            let entry = self
                .plugins
                .get(name)
                .ok_or_else(|| DomainError::NotFound(name.to_string()))?;
            if !Arc::ptr_eq(&entry.plugin, &plugin_handle) {
                return Err(DomainError::NotFound(format!(
                    "plugin '{name}' was reloaded"
                )));
            }
            if !entry.enabled {
                return Err(DomainError::NotFound(format!(
                    "plugin '{name}' is disabled"
                )));
            }
            scoped_credential
                .map(|credential| {
                    CredentialScope::new(Arc::clone(&entry.credential_slot), credential)
                })
                .transpose()?
        };
        let fn_exists = plugin.function_exists(func);
        tracing::debug!(plugin = name, func, fn_exists, "plugin call pre-call");
        let result = plugin.call::<I, &[u8]>(func, input).map_err(|e| {
            DomainError::PluginError(format!(
                "plugin call failed (function_exists={fn_exists}): {e}"
            ))
        })?;
        // `result` still borrows guest memory. Check its length before the
        // only host allocation; CAPTCHA guests also have a runtime memory cap.
        materialize_plugin_output(result, output_limit)
    }
}

fn materialize_plugin_output(output: &[u8], limit: Option<usize>) -> Result<String, DomainError> {
    if limit.is_some_and(|limit| output.len() > limit) {
        return Err(DomainError::PluginError(
            "plugin output exceeds safety limit".into(),
        ));
    }
    std::str::from_utf8(output)
        .map(str::to_owned)
        .map_err(|_| DomainError::PluginError("plugin output is not valid UTF-8".into()))
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
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use dashmap::try_result::TryResult;

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
            credential_slot: Arc::new(super::super::capabilities::CredentialSlotState::default()),
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
    fn test_scoped_credential_is_cleared_when_plugin_call_fails() {
        use crate::domain::model::credential::Credential;

        let registry = PluginRegistry::new();
        registry.insert("plug-a".to_string(), make_loaded("plug-a"));
        let slot = Arc::clone(
            &registry
                .plugins
                .get("plug-a")
                .expect("loaded plugin")
                .credential_slot,
        );

        let error = registry
            .call_plugin_with_credential(
                "plug-a",
                "missing",
                "",
                Credential::new("alice", "secret"),
            )
            .expect_err("missing export");
        assert!(matches!(error, DomainError::PluginError(_)));
        assert!(slot.lock().unwrap().is_none());
    }

    #[test]
    fn disabled_plugin_is_rejected_before_credential_injection() {
        let registry = PluginRegistry::new();
        registry.insert("plug-a".to_string(), make_loaded("plug-a"));
        let slot = Arc::clone(
            &registry
                .plugins
                .get("plug-a")
                .expect("loaded plugin")
                .credential_slot,
        );
        registry.set_enabled("plug-a", false).unwrap();

        let error = registry
            .call_plugin_with_credential(
                "plug-a",
                "missing",
                "",
                Credential::new("alice", "secret"),
            )
            .expect_err("disabled plugin");

        assert!(matches!(error, DomainError::NotFound(message) if message.contains("disabled")));
        assert!(slot.lock().unwrap().is_none());
        assert!(!slot.has_been_exposed());
    }

    #[test]
    fn disabling_plugin_does_not_wait_for_blocked_plugin_call() {
        let registry = Arc::new(PluginRegistry::new());
        registry.insert("plug-a".to_string(), make_loaded("plug-a"));
        let plugin_handle = Arc::clone(
            &registry
                .plugins
                .get("plug-a")
                .expect("loaded plugin")
                .plugin,
        );
        let plugin_guard = plugin_handle.lock().unwrap();

        let caller_registry = Arc::clone(&registry);
        let caller = thread::spawn(move || caller_registry.call_plugin("plug-a", "missing", ""));

        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let registry_guard_held =
                matches!(registry.plugins.try_get_mut("plug-a"), TryResult::Locked);
            let plugin_handle_cloned = Arc::strong_count(&plugin_handle) >= 3;
            if registry_guard_held || plugin_handle_cloned {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "plugin call did not reach the plugin mutex"
            );
            thread::yield_now();
        }

        let (disable_tx, disable_rx) = mpsc::channel();
        let disabler_registry = Arc::clone(&registry);
        let disabler = thread::spawn(move || {
            disable_tx
                .send(disabler_registry.set_enabled("plug-a", false))
                .expect("disable receiver remains connected");
        });

        let prompt_disable = disable_rx.recv_timeout(Duration::from_secs(1));
        let completed_while_call_blocked = prompt_disable.is_ok();
        drop(plugin_guard);

        let disable_result = match prompt_disable {
            Ok(result) => result,
            Err(_) => disable_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("disable completes after plugin call unblocks"),
        };
        disable_result.expect("disable succeeds");
        disabler.join().expect("disable thread does not panic");
        let call_error = caller
            .join()
            .expect("call thread does not panic")
            .expect_err("disabled call is rejected");

        assert!(
            completed_while_call_blocked,
            "disable must not wait for a blocked plugin call"
        );
        assert!(
            matches!(call_error, DomainError::NotFound(message) if message.contains("disabled"))
        );
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

    #[test]
    fn test_function_exists_returns_false_for_missing_export() {
        let registry = PluginRegistry::new();
        registry.insert("plug-a".to_string(), make_loaded("plug-a"));

        let result = registry.function_exists("plug-a", "extract_links");
        assert!(!result.unwrap());
    }

    #[test]
    fn capped_output_is_rejected_before_materialization() {
        let output = vec![b'x'; 9];

        let error = materialize_plugin_output(&output, Some(8))
            .expect_err("oversized output must be rejected");

        assert!(
            matches!(error, DomainError::PluginError(message) if message.contains("safety limit"))
        );
    }
}
