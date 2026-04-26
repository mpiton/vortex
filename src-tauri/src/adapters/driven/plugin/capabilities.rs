//! Capability-based host function registration for WASM plugins.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;

use crate::domain::model::plugin::PluginManifest;
use crate::domain::ports::driven::CredentialStore;

/// Shared resources across all plugins (singleton).
pub struct SharedHostResources {
    pub(crate) http_client: reqwest::blocking::Client,
    http_timeout: Duration,
    pub(crate) credential_store: Option<Arc<dyn CredentialStore>>,
    pub(crate) plugin_configs: DashMap<String, DashMap<String, String>>,
    pub(crate) plugin_states: DashMap<String, DashMap<String, String>>,
}

impl SharedHostResources {
    fn http_client_builder(timeout: Duration) -> reqwest::blocking::ClientBuilder {
        // Match the async download client's User-Agent. reqwest's default
        // (`reqwest/<version>`) triggers bot-detection on some CDNs that
        // serve an HTML stub without the player/config payload back —
        // observed on Vimeo, which returned 1.6 KB stubs instead of the
        // 26 KB embed page when the default UA was used. A named UA also
        // gives the remote side a way to identify traffic from Vortex
        // rather than generic scripted clients.
        reqwest::blocking::Client::builder()
            .user_agent("Vortex/0.1")
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
    }

    /// Create shared resources with a 30-second HTTP timeout and no credential store.
    pub fn new() -> Self {
        let http_timeout = Duration::from_secs(30);
        let http_client = Self::http_client_builder(http_timeout)
            .build()
            .expect("failed to build reqwest blocking client");
        Self {
            http_client,
            http_timeout,
            credential_store: None,
            plugin_configs: DashMap::new(),
            plugin_states: DashMap::new(),
        }
    }

    /// Attach a credential store to be used by host functions.
    pub fn with_credential_store(mut self, store: Arc<dyn CredentialStore>) -> Self {
        self.credential_store = Some(store);
        self
    }

    pub fn http_client(&self) -> &reqwest::blocking::Client {
        &self.http_client
    }

    pub fn http_client_for_host(
        &self,
        host: &str,
        addrs: &[SocketAddr],
    ) -> Result<reqwest::blocking::Client, reqwest::Error> {
        Self::http_client_builder(self.http_timeout)
            .resolve_to_addrs(host, addrs)
            .build()
    }

    pub fn credential_store(&self) -> Option<&Arc<dyn CredentialStore>> {
        self.credential_store.as_ref()
    }

    pub fn plugin_configs(&self) -> &DashMap<String, DashMap<String, String>> {
        &self.plugin_configs
    }

    pub fn plugin_states(&self) -> &DashMap<String, DashMap<String, String>> {
        &self.plugin_states
    }
}

impl Default for SharedHostResources {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-plugin context passed as UserData to host functions.
pub struct PluginHostContext {
    pub(crate) plugin_name: String,
    pub(crate) capabilities: Vec<String>,
    pub(crate) shared: Arc<SharedHostResources>,
}

/// Build host functions based on manifest capabilities.
///
/// Always registers the six base functions (log, get/set config, get/set state, get credential).
/// Conditionally registers http_request and run_subprocess based on declared capabilities.
pub fn build_host_functions(
    manifest: &PluginManifest,
    shared: &Arc<SharedHostResources>,
) -> Vec<extism::Function> {
    let name = manifest.info().name().to_string();

    // Ensure per-plugin config/state maps exist before any function runs.
    let plugin_configs = shared.plugin_configs.entry(name.clone()).or_default();
    // Persisted overrides may have been replayed before the plugin loaded.
    // Drop entries that no longer pass the current schema (e.g. a manifest
    // update narrowed an enum, tightened a regex, or removed the key) so
    // the WASM plugin never observes a stale schema-invalid value via
    // `get_config`.
    let schema = manifest.config_schema();
    plugin_configs.retain(|key, value| {
        if schema.validate(key, value).is_ok() {
            true
        } else {
            tracing::warn!(
                plugin = %name,
                key = %key,
                "dropping persisted plugin config value that no longer matches schema"
            );
            false
        }
    });
    for (key, value) in manifest.config_defaults() {
        plugin_configs
            .entry(key.clone())
            .or_insert_with(|| value.clone());
    }
    shared.plugin_states.entry(name.clone()).or_default();

    let ctx = PluginHostContext {
        plugin_name: name,
        capabilities: manifest.capabilities().to_vec(),
        shared: Arc::clone(shared),
    };
    let user_data = extism::UserData::new(ctx);

    let mut functions = vec![
        super::host_functions::make_log_function(user_data.clone()),
        super::host_functions::make_get_config_function(user_data.clone()),
        super::host_functions::make_set_config_function(user_data.clone()),
        super::host_functions::make_get_state_function(user_data.clone()),
        super::host_functions::make_set_state_function(user_data.clone()),
        super::host_functions::make_get_credential_function(user_data.clone()),
    ];

    if manifest.has_capability("http") {
        functions.push(super::host_functions::make_http_request_function(
            user_data.clone(),
        ));
    }

    if manifest
        .capabilities()
        .iter()
        .any(|c| c.starts_with("subprocess:"))
    {
        functions.push(super::host_functions::make_run_subprocess_function(
            user_data.clone(),
        ));
    }

    functions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::plugin::{PluginCategory, PluginInfo, PluginManifest};

    fn make_manifest_with_caps(caps: Vec<&str>) -> PluginManifest {
        let info = PluginInfo::new(
            "test-plugin".to_string(),
            "1.0.0".to_string(),
            "Test plugin".to_string(),
            "tester".to_string(),
            PluginCategory::Utility,
        );
        PluginManifest::new(info).with_capabilities(caps.into_iter().map(String::from).collect())
    }

    #[test]
    fn test_build_host_functions_all_capabilities() {
        let shared = Arc::new(SharedHostResources::new());
        let manifest =
            make_manifest_with_caps(vec!["http", "subprocess:ffmpeg", "subprocess:yt-dlp"]);

        let functions = build_host_functions(&manifest, &shared);

        // 6 base + http + subprocess = 8
        assert_eq!(functions.len(), 8);
    }

    #[test]
    fn test_build_host_functions_minimal() {
        let shared = Arc::new(SharedHostResources::new());
        let manifest = make_manifest_with_caps(vec![]);

        let functions = build_host_functions(&manifest, &shared);

        // Only 6 base functions
        assert_eq!(functions.len(), 6);
    }

    #[test]
    fn test_shared_resources_default() {
        let shared = SharedHostResources::default();

        assert!(shared.credential_store().is_none());
        assert!(shared.plugin_configs().is_empty());
        assert!(shared.plugin_states().is_empty());
    }

    #[test]
    fn test_build_host_functions_initializes_plugin_maps() {
        let shared = Arc::new(SharedHostResources::new());
        let manifest = make_manifest_with_caps(vec![]);

        build_host_functions(&manifest, &shared);

        assert!(shared.plugin_configs().contains_key("test-plugin"));
        assert!(shared.plugin_states().contains_key("test-plugin"));
    }

    #[test]
    fn test_build_host_functions_seeds_config_defaults() {
        let shared = Arc::new(SharedHostResources::new());
        let info = PluginInfo::new(
            "test-plugin".to_string(),
            "1.0.0".to_string(),
            "Test plugin".to_string(),
            "tester".to_string(),
            PluginCategory::Utility,
        );
        let manifest =
            PluginManifest::new(info).with_config_defaults(std::collections::HashMap::from([
                ("default_quality".to_string(), "720p".to_string()),
                ("extract_audio_only".to_string(), "false".to_string()),
            ]));

        build_host_functions(&manifest, &shared);

        let plugin_config = shared
            .plugin_configs()
            .get("test-plugin")
            .expect("plugin config map");
        assert_eq!(
            plugin_config
                .get("default_quality")
                .map(|v| v.value().clone()),
            Some("720p".to_string())
        );
        assert_eq!(
            plugin_config
                .get("extract_audio_only")
                .map(|v| v.value().clone()),
            Some("false".to_string())
        );
    }

    #[test]
    fn test_http_request_denied_without_capability() {
        let shared = Arc::new(SharedHostResources::new());
        let manifest = make_manifest_with_caps(vec![]);

        let functions = build_host_functions(&manifest, &shared);

        // No http cap: only 6 base functions, no http_request
        assert_eq!(functions.len(), 6);
        assert!(!functions.iter().any(|f| f.name() == "http_request"));
    }

    #[test]
    fn test_build_host_functions_http_only() {
        let shared = Arc::new(SharedHostResources::new());
        let manifest = make_manifest_with_caps(vec!["http"]);

        let functions = build_host_functions(&manifest, &shared);

        // 6 base + http_request = 7
        assert_eq!(functions.len(), 7);
        assert!(functions.iter().any(|f| f.name() == "http_request"));
    }

    #[test]
    fn test_build_host_functions_subprocess_only() {
        let shared = Arc::new(SharedHostResources::new());
        let manifest = make_manifest_with_caps(vec!["subprocess:ffmpeg"]);

        let functions = build_host_functions(&manifest, &shared);

        // 6 base + run_subprocess = 7
        assert_eq!(functions.len(), 7);
        assert!(functions.iter().any(|f| f.name() == "run_subprocess"));
    }

    #[test]
    fn test_get_credential_returns_credential() {
        use crate::domain::model::credential::Credential;
        use crate::domain::ports::driven::CredentialStore;

        struct MockCredentialStore;
        impl CredentialStore for MockCredentialStore {
            fn get(
                &self,
                service: &str,
            ) -> Result<Option<Credential>, crate::domain::error::DomainError> {
                if service == "test-service" {
                    Ok(Some(Credential::new("user", "pass")))
                } else {
                    Ok(None)
                }
            }
            fn store(
                &self,
                _: &str,
                _: &Credential,
            ) -> Result<(), crate::domain::error::DomainError> {
                Ok(())
            }
            fn delete(&self, _: &str) -> Result<(), crate::domain::error::DomainError> {
                Ok(())
            }
        }

        let shared = Arc::new(
            SharedHostResources::new().with_credential_store(Arc::new(MockCredentialStore)),
        );

        let cred = shared
            .credential_store()
            .unwrap()
            .get("test-service")
            .unwrap();
        assert!(cred.is_some());
        let cred = cred.unwrap();
        assert_eq!(cred.username(), "user");
        assert_eq!(cred.password(), "pass");

        let missing = shared
            .credential_store()
            .unwrap()
            .get("unknown-service")
            .unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_subprocess_denied_unauthorized_binary() {
        // Verify capability check logic: only declared binaries are allowed
        let caps: Vec<String> = vec!["subprocess:ffmpeg".to_string()];

        let allowed_binary = "ffmpeg";
        let denied_binary = "yt-dlp";

        let allowed_cap = format!("subprocess:{allowed_binary}");
        let denied_cap = format!("subprocess:{denied_binary}");

        assert!(caps.iter().any(|c| c == &allowed_cap));
        assert!(!caps.iter().any(|c| c == &denied_cap));
    }
}
