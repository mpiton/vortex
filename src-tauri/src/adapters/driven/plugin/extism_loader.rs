//! Implements [`PluginLoader`] using Extism and [`PluginRegistry`].

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine;

use crate::domain::error::DomainError;
use crate::domain::model::account::AccountStatus;
use crate::domain::model::captcha::{CaptchaChallenge, CaptchaSolution};
use crate::domain::model::credential::Credential;
use crate::domain::model::plugin::{PluginCategory, PluginInfo, PluginManifest};
use crate::domain::ports::driven::plugin_loader::DownloadedFileInfo;
use crate::domain::ports::driven::plugin_store_client::OfficialPluginProvenance;
use crate::domain::ports::driven::{
    CaptchaSolverOutcome, ExtractedHosterLink, PluginLoader, ValidationOutcome,
};

use super::builtin::HttpModule;
use super::capabilities::{SharedHostResources, build_host_functions_for_instance};
use super::hoster_contract::parse_hoster_links;
use super::manifest::{
    find_wasm_file, parse_manifest, parse_manifest_metadata, parse_manifest_metadata_bytes,
};
use super::provenance::OfficialProvenanceStore;
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

const MAX_CAPTCHA_SOLVER_OUTPUT_BYTES: usize = 8 * 1024;
const CAPTCHA_PLUGIN_MEMORY_MAX_PAGES: u32 = 1024;

fn runtime_manifest(wasm_bytes: Vec<u8>, category: PluginCategory) -> extism::Manifest {
    let manifest = extism::Manifest::new([extism::Wasm::data(wasm_bytes)]);
    if category == PluginCategory::Captcha {
        // One WebAssembly page is 64 KiB. This bounds allocations made while
        // producing output; the registry's byte cap then prevents a large
        // guest response from being copied into a host String.
        manifest.with_memory_max(CAPTCHA_PLUGIN_MEMORY_MAX_PAGES)
    } else {
        manifest
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
struct CaptchaSolverRequest<'a> {
    challenge_id: &'a str,
    challenge_type: String,
    challenge_url: &'a str,
    image_data: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptchaSolverResponse {
    status: String,
    solution: Option<String>,
}

fn encode_captcha_solver_request(challenge: &CaptchaChallenge) -> Result<String, DomainError> {
    serde_json::to_string(&CaptchaSolverRequest {
        challenge_id: challenge.id().as_str(),
        challenge_type: challenge.challenge_type().to_string(),
        challenge_url: challenge.url(),
        image_data: challenge
            .image_data()
            .map(|image| base64::engine::general_purpose::STANDARD.encode(image)),
    })
    .map_err(|_| DomainError::PluginError("failed to encode CAPTCHA request".into()))
}

fn parse_captcha_solver_output(output: &str) -> Result<CaptchaSolverOutcome, DomainError> {
    if output.len() > MAX_CAPTCHA_SOLVER_OUTPUT_BYTES {
        return Err(DomainError::PluginError(
            "CAPTCHA solver response exceeds safety limit".into(),
        ));
    }
    let response: CaptchaSolverResponse = serde_json::from_str(output)
        .map_err(|_| DomainError::PluginError("CAPTCHA solver returned invalid JSON".into()))?;
    match (response.status.as_str(), response.solution) {
        ("solved", Some(solution)) => CaptchaSolution::try_new(solution)
            .map(CaptchaSolverOutcome::Solved)
            .map_err(|_| {
                DomainError::PluginError("CAPTCHA solver returned an invalid solution".into())
            }),
        ("unavailable", None) => Ok(CaptchaSolverOutcome::Unavailable),
        ("rejected", None) => Ok(CaptchaSolverOutcome::Rejected),
        ("interaction_required", None) => Ok(CaptchaSolverOutcome::InteractionRequired),
        _ => Err(DomainError::PluginError(
            "CAPTCHA solver returned an invalid status payload".into(),
        )),
    }
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
    provenance: OfficialProvenanceStore,
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
        let provenance_path = plugins_dir.with_extension("provenance.json");
        Self::new_with_provenance_path(plugins_dir, shared_resources, provenance_path)
    }

    pub fn new_with_provenance_path(
        plugins_dir: PathBuf,
        shared_resources: Arc<SharedHostResources>,
        provenance_path: PathBuf,
    ) -> Result<Self, DomainError> {
        let plugins_dir = resolve_path(&plugins_dir)?;
        std::fs::create_dir_all(&plugins_dir).map_err(|error| {
            DomainError::PluginError(format!(
                "failed to create plugin directory '{}': {error}",
                plugins_dir.display()
            ))
        })?;
        let plugins_dir = std::fs::canonicalize(&plugins_dir).map_err(|error| {
            DomainError::PluginError(format!(
                "failed to resolve plugin directory '{}': {error}",
                plugins_dir.display()
            ))
        })?;
        let provenance_path = resolve_path(&provenance_path)?;
        if provenance_path.starts_with(&plugins_dir) {
            return Err(DomainError::ValidationError(
                "plugin provenance must live outside the plugin directory".into(),
            ));
        }
        Ok(Self {
            registry: Arc::new(PluginRegistry::new()),
            plugins_dir,
            shared_resources,
            builtin_http: HttpModule::new()?,
            installs: Arc::new(Mutex::new(HashMap::new())),
            provenance: OfficialProvenanceStore::new(provenance_path)?,
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

    fn install_from_dir(
        &self,
        dir: &Path,
        provenance: Option<&OfficialPluginProvenance>,
    ) -> Result<(), DomainError> {
        let (manifest, _) = parse_manifest(dir)?;
        let name = manifest.info().name().to_string();
        if let Some(provenance) = provenance {
            verify_provenance(dir, &manifest, provenance)?;
        }

        let state = self.get_or_create_install_state(&name);
        state.count.fetch_add(1, Ordering::SeqCst);
        let _in_flight = InstallInFlight {
            state: state.clone(),
        };
        let _serializer_guard = state
            .serializer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        match self.unload(&name) {
            Ok(()) | Err(DomainError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
        if provenance.is_none() {
            self.provenance.revoke(&name)?;
        }

        let dest_dir = self.plugins_dir.join(&name);
        if dest_dir.exists() {
            std::fs::remove_dir_all(&dest_dir).map_err(|error| {
                DomainError::PluginError(format!(
                    "failed to remove existing plugin dir '{}': {error}",
                    dest_dir.display()
                ))
            })?;
        }
        std::fs::create_dir_all(&dest_dir).map_err(|error| {
            DomainError::PluginError(format!("failed to create plugin dir: {error}"))
        })?;
        for entry in std::fs::read_dir(dir).map_err(|error| {
            DomainError::PluginError(format!("failed to read staging dir: {error}"))
        })? {
            let entry = entry.map_err(|error| {
                DomainError::PluginError(format!("staging dir entry error: {error}"))
            })?;
            let source = entry.path();
            if source.is_file() {
                let destination = dest_dir.join(entry.file_name());
                std::fs::copy(&source, &destination).map_err(|error| {
                    DomainError::PluginError(format!(
                        "failed to copy {} → {}: {error}",
                        source.display(),
                        destination.display()
                    ))
                })?;
            }
        }

        if let Some(provenance) = provenance {
            verify_provenance(&dest_dir, &manifest, provenance)?;
            self.provenance.record(provenance)?;
        }
        let result = self.load(&manifest);
        if result.is_err() && provenance.is_some() {
            self.provenance.revoke(&name)?;
        }
        result
    }
}

fn resolve_path(path: &Path) -> Result<PathBuf, DomainError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| {
                DomainError::PluginError(format!("failed to resolve current directory: {error}"))
            })?
            .join(path)
    };
    resolve_existing_ancestor(&absolute)
}

fn resolve_existing_ancestor(path: &Path) -> Result<PathBuf, DomainError> {
    match std::fs::canonicalize(path) {
        Ok(resolved) => Ok(resolved),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if path
                .components()
                .any(|component| component == Component::ParentDir)
            {
                return Err(DomainError::ValidationError(format!(
                    "path '{}' contains an unresolved parent traversal",
                    path.display()
                )));
            }
            if std::fs::symlink_metadata(path).is_ok() {
                return Err(DomainError::ValidationError(format!(
                    "path '{}' contains an unresolved symbolic link",
                    path.display()
                )));
            }
            let parent = path.parent().ok_or_else(|| {
                DomainError::ValidationError(format!(
                    "path '{}' has no resolvable parent",
                    path.display()
                ))
            })?;
            let name = path.file_name().ok_or_else(|| {
                DomainError::ValidationError(format!(
                    "path '{}' cannot be resolved safely",
                    path.display()
                ))
            })?;
            Ok(resolve_existing_ancestor(parent)?.join(name))
        }
        Err(error) => Err(DomainError::PluginError(format!(
            "failed to resolve path '{}': {error}",
            path.display()
        ))),
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
        let manifest_bytes = std::fs::read(plugin_dir.join("plugin.toml")).map_err(|error| {
            DomainError::PluginError(format!("failed to read plugin.toml for '{name}': {error}"))
        })?;
        let disk_manifest = parse_manifest_metadata_bytes(&plugin_dir, &manifest_bytes)?;
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

        let grants = self.provenance.grants_for(
            &name,
            disk_manifest.info().version(),
            &wasm_bytes,
            &manifest_bytes,
        );
        let extism_manifest = runtime_manifest(wasm_bytes, disk_manifest.info().category());
        let (host_functions, credential_slot) =
            build_host_functions_for_instance(&disk_manifest, &self.shared_resources, grants);
        let plugin = extism::Plugin::new(&extism_manifest, host_functions, true)
            .map_err(|e| DomainError::PluginError(format!("failed to load plugin: {e}")))?;

        let loaded = LoadedPlugin {
            manifest: disk_manifest,
            plugin: std::sync::Arc::new(std::sync::Mutex::new(plugin)),
            credential_slot,
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
        // A debrid plugin claims every hoster it can unrestrict, so on name
        // order alone it would steal URLs the hoster plugin owns. Debrid is a
        // fallback rung of the resolution cascade, never the URL owner.
        infos.sort_by_key(|i| (i.category() == PluginCategory::Debrid, i.name().to_string()));
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

    fn plugin_can_handle(&self, name: &str, url: &str) -> Result<bool, DomainError> {
        let enabled = self
            .registry
            .list_info()
            .into_iter()
            .any(|i| i.name() == name && i.is_enabled());
        if !enabled {
            return Ok(false);
        }
        match self.registry.call_plugin(name, "can_handle", url) {
            Ok(result) => Ok(result.trim() == "true"),
            Err(e) => {
                tracing::warn!("plugin '{name}' failed can_handle call: {e}");
                Ok(false)
            }
        }
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

    fn solve_captcha(
        &self,
        plugin_name: &str,
        challenge: &CaptchaChallenge,
    ) -> Result<CaptchaSolverOutcome, DomainError> {
        let info = self
            .registry
            .list_info()
            .into_iter()
            .find(|info| info.name() == plugin_name)
            .ok_or_else(|| DomainError::NotFound(plugin_name.to_string()))?;
        if !info.is_enabled() || info.category() != PluginCategory::Captcha {
            return Err(DomainError::NotFound(format!(
                "CAPTCHA solver '{plugin_name}' is not enabled"
            )));
        }
        for export in ["can_solve", "solve"] {
            if !self.registry.function_exists(plugin_name, export)? {
                return Err(DomainError::PluginError(format!(
                    "CAPTCHA plugin '{plugin_name}' does not export '{export}'"
                )));
            }
        }
        let request = encode_captcha_solver_request(challenge)?;
        let supports = self
            .registry
            .call_plugin_capped(
                plugin_name,
                "can_solve",
                &request,
                MAX_CAPTCHA_SOLVER_OUTPUT_BYTES,
            )
            .map_err(|_| {
                DomainError::PluginError(format!(
                    "CAPTCHA plugin '{plugin_name}' capability probe failed"
                ))
            })?;
        match supports.trim() {
            "false" => return Ok(CaptchaSolverOutcome::Unavailable),
            "true" => {}
            _ => {
                return Err(DomainError::PluginError(format!(
                    "CAPTCHA plugin '{plugin_name}' returned an invalid capability response"
                )));
            }
        }
        let output = self
            .registry
            .call_plugin_capped(
                plugin_name,
                "solve",
                &request,
                MAX_CAPTCHA_SOLVER_OUTPUT_BYTES,
            )
            .map_err(|_| {
                DomainError::PluginError(format!("CAPTCHA plugin '{plugin_name}' solve failed"))
            })?;
        parse_captcha_solver_output(&output)
    }

    fn extract_links(&self, url: &str) -> Result<String, DomainError> {
        self.call_url_plugin_function(url, "extract_links")
    }

    fn extract_hoster_link(
        &self,
        service_name: &str,
        url: &str,
        credential: Option<&Credential>,
    ) -> Result<ExtractedHosterLink, DomainError> {
        self.extract_hoster_links(service_name, url, credential)?
            .into_iter()
            .next()
            .ok_or(DomainError::HosterNoFile)
    }

    fn extract_hoster_links(
        &self,
        service_name: &str,
        url: &str,
        credential: Option<&Credential>,
    ) -> Result<Vec<ExtractedHosterLink>, DomainError> {
        let info = self
            .registry
            .list_info()
            .into_iter()
            .find(|info| info.name() == service_name)
            .ok_or_else(|| DomainError::NotFound(service_name.to_string()))?;
        if !info.is_enabled() {
            return Err(DomainError::NotFound(format!(
                "plugin '{service_name}' is disabled"
            )));
        }
        if !self
            .registry
            .function_exists(service_name, "extract_links")?
        {
            return Err(DomainError::NotFound(format!(
                "plugin '{service_name}' does not export 'extract_links'"
            )));
        }
        let output = match credential {
            Some(credential) => self
                .registry
                .call_plugin_with_credential(service_name, "extract_links", url, credential.clone())
                .map_err(|error| {
                    classify_plugin_call_error(error, classify_credentialed_hoster_plugin_error)
                })?,
            None => self
                .registry
                .call_plugin(service_name, "extract_links", url)
                .map_err(|error| classify_plugin_call_error(error, classify_hoster_plugin_error))?,
        };
        parse_hoster_links(&output)
    }

    fn validate_account(
        &self,
        service_name: &str,
        credential: &Credential,
    ) -> Result<ValidationOutcome, DomainError> {
        let info = self
            .registry
            .list_info()
            .into_iter()
            .find(|info| info.name() == service_name)
            .ok_or_else(|| DomainError::NotFound(service_name.to_string()))?;
        if !info.is_enabled() {
            return Err(DomainError::NotFound(format!(
                "plugin '{service_name}' is disabled"
            )));
        }
        if !self
            .registry
            .function_exists(service_name, "validate_account")?
        {
            return Err(DomainError::NotFound(format!(
                "plugin '{service_name}' does not export 'validate_account'"
            )));
        }

        let output = self
            .registry
            .call_plugin_with_credential(service_name, "validate_account", "", credential.clone())
            .map_err(|error| classify_account_plugin_error(&error.to_string()))?;
        parse_validation_outcome(&output)
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
        self.install_from_dir(dir, None)
    }

    fn load_official_from_dir(
        &self,
        dir: &Path,
        provenance: &OfficialPluginProvenance,
    ) -> Result<(), DomainError> {
        self.install_from_dir(dir, Some(provenance))
    }
}

fn verify_provenance(
    dir: &Path,
    manifest: &PluginManifest,
    provenance: &OfficialPluginProvenance,
) -> Result<(), DomainError> {
    use sha2::{Digest, Sha256};

    if manifest.info().name() != provenance.name || manifest.info().version() != provenance.version
    {
        return Err(DomainError::ValidationError(
            "official plugin provenance does not match manifest identity".into(),
        ));
    }
    let wasm = std::fs::read(find_wasm_file(dir)?).map_err(|error| {
        DomainError::PluginError(format!("failed to read verified plugin wasm: {error}"))
    })?;
    let manifest_bytes = std::fs::read(dir.join("plugin.toml")).map_err(|error| {
        DomainError::PluginError(format!("failed to read verified plugin manifest: {error}"))
    })?;
    let wasm_digest = hex::encode(Sha256::digest(&wasm));
    let manifest_digest = hex::encode(Sha256::digest(&manifest_bytes));
    if !wasm_digest.eq_ignore_ascii_case(&provenance.wasm_sha256)
        || !manifest_digest.eq_ignore_ascii_case(&provenance.manifest_sha256)
    {
        return Err(DomainError::PluginError(
            "official plugin checksum changed before load".into(),
        ));
    }
    Ok(())
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

fn classify_account_plugin_error(message: &str) -> DomainError {
    if has_error_code(message, "ACCOUNT_INVALID_CREDENTIALS") {
        DomainError::AccountInvalidCredentials
    } else if has_error_code(message, "ACCOUNT_EXPIRED") {
        DomainError::AccountExpired
    } else if has_error_code(message, "ACCOUNT_COOLDOWN") {
        DomainError::AccountCooldown
    } else if has_error_code(message, "ACCOUNT_QUOTA_EXCEEDED") {
        DomainError::AccountQuotaExceeded
    } else {
        DomainError::PluginError("plugin account operation failed".into())
    }
}

fn classify_plugin_call_error(
    error: DomainError,
    classify: fn(&str) -> DomainError,
) -> DomainError {
    match error {
        DomainError::NetworkError(_) => error,
        error => classify(&error.to_string()),
    }
}

fn classify_credentialed_hoster_plugin_error(message: &str) -> DomainError {
    let account_error = classify_account_plugin_error(message);
    if matches!(account_error, DomainError::PluginError(_)) {
        classify_hoster_plugin_error(message)
    } else {
        account_error
    }
}

fn classify_hoster_plugin_error(message: &str) -> DomainError {
    let normalized = message.to_ascii_lowercase();
    if normalized.starts_with("network error:")
        || normalized.contains("http_request: network error:")
        || normalized.contains("http_request: request failed:")
    {
        DomainError::NetworkError("hoster network request failed".into())
    } else if has_error_code(message, "HOSTER_DIRECT_URL_EXPIRED") {
        DomainError::HosterDirectUrlExpired
    } else if has_error_code(message, "HOSTER_AUTHENTICATION_REQUIRED") {
        DomainError::HosterAuthenticationRequired
    } else if has_error_code(message, "HOSTER_NO_FILE") {
        DomainError::HosterNoFile
    } else {
        classify_legacy_hoster_error(message)
    }
}

fn has_error_code(message: &str, expected: &str) -> bool {
    message
        .split(|character: char| !(character.is_ascii_uppercase() || character == '_'))
        .any(|token| token == expected)
}

/// Compatibility for the three Lot 1 plugins until their WASM ABI exposes
/// structured error codes. Keep these matches tied to their exact diagnostics;
/// generic prose must never become a trusted typed error.
fn classify_legacy_hoster_error(message: &str) -> DomainError {
    let message = message.to_ascii_lowercase();
    if message.contains("error-passwordrequired")
        || message.contains(
            "no direct download link found in mediafire page (file may be private or password-protected)",
        )
        || has_official_hoster_http_status(&message, 401)
        || has_official_hoster_http_status(&message, 403)
    {
        DomainError::HosterAuthenticationRequired
    } else if has_official_hoster_http_status(&message, 410) {
        DomainError::HosterDirectUrlExpired
    } else if message.contains("mediafire file is offline or removed:")
        || message.contains("pixeldrain file is offline or removed:")
        || message.contains("gofile content is offline or removed:")
        || message.contains("gofile folder is empty (no children)")
        || (message.contains("gofile file id ") && message.ends_with(" not found in folder"))
        || has_official_hoster_http_status(&message, 404)
    {
        DomainError::HosterNoFile
    } else {
        DomainError::PluginError("hoster plugin operation failed".into())
    }
}

fn has_official_hoster_http_status(message: &str, status: u16) -> bool {
    ["mediafire", "pixeldrain", "gofile", "hoster"]
        .into_iter()
        .any(|service| message.contains(&format!("{service} http returned status {status}:")))
}

#[derive(serde::Deserialize)]
struct PluginValidationOutcome {
    valid: bool,
    #[serde(default)]
    status: Option<String>,
    #[serde(default, alias = "latencyMs")]
    latency_ms: Option<u64>,
    #[serde(default, alias = "trafficLeft")]
    traffic_left: Option<u64>,
    #[serde(default, alias = "trafficTotal")]
    traffic_total: Option<u64>,
    #[serde(default, alias = "validUntil")]
    valid_until: Option<u64>,
}

fn parse_validation_outcome(output: &str) -> Result<ValidationOutcome, DomainError> {
    use std::str::FromStr;

    let parsed: PluginValidationOutcome = serde_json::from_str(output).map_err(|_| {
        DomainError::PluginError("plugin account validation returned invalid JSON".into())
    })?;
    let status = match parsed.status {
        Some(status) => AccountStatus::from_str(&status).map_err(|_| {
            DomainError::PluginError("plugin account validation returned unknown status".into())
        })?,
        None if parsed.valid => AccountStatus::Valid,
        None => AccountStatus::Error,
    };
    if parsed.valid != (status == AccountStatus::Valid) {
        return Err(DomainError::PluginError(
            "plugin account validation returned contradictory state".into(),
        ));
    }
    let error_message = match status {
        AccountStatus::Valid => None,
        AccountStatus::InvalidCredentials => Some("Account credentials were rejected".into()),
        AccountStatus::MissingCredential => Some("Account credential is missing".into()),
        AccountStatus::Expired => Some("Account is expired".into()),
        AccountStatus::QuotaExhausted => Some("Account quota is exhausted".into()),
        AccountStatus::Cooldown => Some("Account is temporarily rate-limited".into()),
        AccountStatus::Unverified | AccountStatus::Error => {
            Some("Account validation failed".into())
        }
    };
    Ok(ValidationOutcome {
        status,
        latency_ms: parsed.latency_ms,
        traffic_left: parsed.traffic_left,
        traffic_total: parsed.traffic_total,
        valid_until: parsed.valid_until,
        error_message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::plugin::{PluginCategory, PluginInfo, PluginManifest};
    use crate::domain::ports::driven::CaptchaSolverOutcome;
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

    #[test]
    fn captcha_solver_output_accepts_a_redacted_solution() {
        let outcome =
            parse_captcha_solver_output(r#"{"status":"solved","solution":"secret-answer"}"#)
                .expect("valid solver response");

        let CaptchaSolverOutcome::Solved(solution) = outcome else {
            panic!("expected solved outcome");
        };
        assert_eq!(solution.expose(), "secret-answer");
        assert!(!format!("{solution:?}").contains("secret-answer"));
    }

    #[test]
    fn captcha_solver_output_maps_fallback_statuses() {
        for (status, expected) in [
            ("unavailable", CaptchaSolverOutcome::Unavailable),
            ("rejected", CaptchaSolverOutcome::Rejected),
            (
                "interaction_required",
                CaptchaSolverOutcome::InteractionRequired,
            ),
        ] {
            let output = format!(r#"{{"status":"{status}"}}"#);
            assert_eq!(parse_captcha_solver_output(&output), Ok(expected));
        }
    }

    #[test]
    fn captcha_solver_output_rejects_unknown_fields_and_missing_solutions() {
        assert!(parse_captcha_solver_output(r#"{"status":"solved"}"#).is_err());
        assert!(
            parse_captcha_solver_output(
                r#"{"status":"unavailable","solution":"must-not-be-here"}"#,
            )
            .is_err()
        );
        assert!(parse_captcha_solver_output(r#"{"status":"unknown","debug":"secret"}"#).is_err());
    }

    #[test]
    fn captcha_runtime_has_a_guest_memory_limit() {
        let manifest = runtime_manifest(
            vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00],
            PluginCategory::Captcha,
        );

        assert_eq!(
            manifest.memory.max_pages,
            Some(CAPTCHA_PLUGIN_MEMORY_MAX_PAGES)
        );
    }

    #[test]
    fn account_plugin_errors_map_to_typed_domain_errors() {
        assert_eq!(
            classify_account_plugin_error(
                "ACCOUNT_INVALID_CREDENTIALS: configured key was rejected"
            ),
            DomainError::AccountInvalidCredentials
        );
        assert_eq!(
            classify_account_plugin_error("ACCOUNT_EXPIRED: subscription ended"),
            DomainError::AccountExpired
        );
        assert_eq!(
            classify_account_plugin_error("ACCOUNT_COOLDOWN: flood detected"),
            DomainError::AccountCooldown
        );
    }

    #[test]
    fn unknown_account_plugin_error_stays_a_plugin_error() {
        let error = classify_account_plugin_error("PLUGIN_ERROR: malformed response");
        assert_eq!(
            error,
            DomainError::PluginError("plugin account operation failed".into())
        );
    }

    #[test]
    fn unknown_account_plugin_error_does_not_expose_plugin_diagnostics() {
        let error = classify_account_plugin_error(
            "PLUGIN_ERROR: upstream echoed Authorization: Bearer super-secret-key",
        );
        assert_eq!(
            error,
            DomainError::PluginError("plugin account operation failed".into())
        );
        assert!(!error.to_string().contains("super-secret-key"));
    }

    #[test]
    fn hoster_plugin_errors_map_to_safe_typed_errors() {
        assert_eq!(
            classify_hoster_plugin_error("MediaFire file is offline or removed: missing"),
            DomainError::HosterNoFile
        );
        assert_eq!(
            classify_hoster_plugin_error(
                "no direct download link found in MediaFire page (file may be private or password-protected)"
            ),
            DomainError::HosterAuthenticationRequired
        );
        assert_eq!(
            classify_hoster_plugin_error("hoster HTTP returned status 410: gone"),
            DomainError::HosterDirectUrlExpired
        );
        for service in ["MediaFire", "Pixeldrain", "Gofile"] {
            assert_eq!(
                classify_hoster_plugin_error(&format!(
                    "{service} HTTP returned status 403: forbidden"
                )),
                DomainError::HosterAuthenticationRequired
            );
        }
        assert_eq!(
            classify_hoster_plugin_error(
                "Gofile content is offline or removed: error-passwordRequired"
            ),
            DomainError::HosterAuthenticationRequired
        );
        assert_eq!(
            classify_hoster_plugin_error(
                "Plugin error: plugin call failed: http_request: Network error: connection refused"
            ),
            DomainError::NetworkError("hoster network request failed".into())
        );
        assert_eq!(
            classify_plugin_call_error(
                DomainError::NetworkError("connection refused".into()),
                classify_hoster_plugin_error,
            ),
            DomainError::NetworkError("connection refused".into())
        );
    }

    #[test]
    fn credentialed_hoster_errors_preserve_account_and_hoster_codes() {
        assert_eq!(
            classify_credentialed_hoster_plugin_error("ACCOUNT_EXPIRED: subscription ended"),
            DomainError::AccountExpired
        );
        assert_eq!(
            classify_credentialed_hoster_plugin_error("HOSTER_NO_FILE: removed"),
            DomainError::HosterNoFile
        );
        assert_eq!(
            classify_credentialed_hoster_plugin_error("hoster HTTP returned status 410: gone"),
            DomainError::HosterDirectUrlExpired
        );
    }

    #[test]
    fn unknown_hoster_plugin_error_does_not_expose_plugin_diagnostics() {
        let error =
            classify_hoster_plugin_error("upstream echoed Authorization: Bearer super-secret-key");
        assert_eq!(
            error,
            DomainError::PluginError("hoster plugin operation failed".into())
        );
        assert!(!error.to_string().contains("super-secret-key"));
    }

    #[test]
    fn unrelated_hoster_prose_does_not_become_a_typed_error() {
        assert_eq!(
            classify_hoster_plugin_error(
                "private network policy expired while authenticating diagnostics"
            ),
            DomainError::PluginError("hoster plugin operation failed".into())
        );
    }

    #[test]
    fn validation_response_defaults_success_to_valid_status() {
        let outcome = parse_validation_outcome(r#"{"valid":true}"#).expect("valid outcome");
        assert!(outcome.is_valid());
        assert_eq!(outcome.status, AccountStatus::Valid);
    }

    #[test]
    fn validation_response_accepts_typed_metrics() {
        let outcome = parse_validation_outcome(
            r#"{"valid":false,"status":"quota_exhausted","trafficLeft":0,"trafficTotal":100,"errorMessage":"quota used"}"#,
        )
        .expect("typed outcome");
        assert_eq!(outcome.status, AccountStatus::QuotaExhausted);
        assert_eq!(outcome.traffic_left, Some(0));
        assert_eq!(outcome.traffic_total, Some(100));
    }

    #[test]
    fn validation_response_rejects_invalid_flag_with_valid_status() {
        let error = parse_validation_outcome(r#"{"valid":false,"status":"valid"}"#)
            .expect_err("contradictory validation response must fail closed");
        assert!(matches!(error, DomainError::PluginError(_)));
    }

    #[test]
    fn validation_response_rejects_valid_flag_with_non_valid_status() {
        let error = parse_validation_outcome(r#"{"valid":true,"status":"expired"}"#)
            .expect_err("contradictory validation response must fail closed");
        assert!(matches!(error, DomainError::PluginError(_)));
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

    fn setup_ytdlp_importing_plugin(dir: &Path, name: &str) -> (Vec<u8>, Vec<u8>) {
        std::fs::create_dir_all(dir).unwrap();
        let manifest = format!(
            r#"[plugin]
name = "{name}"
version = "1.0.0"
category = "crawler"
author = "tester"
description = "Test plugin"

[capabilities]
subprocess = ["yt-dlp"]
"#
        )
        .into_bytes();
        let wasm = br#"(module
  (import "extism:host/user" "run_ytdlp" (func (param i64) (result i64)))
)"#
        .to_vec();
        std::fs::write(dir.join("plugin.toml"), &manifest).unwrap();
        std::fs::write(dir.join(format!("{name}.wasm")), &wasm).unwrap();
        (wasm, manifest)
    }

    #[test]
    fn test_new_rejects_absolute_provenance_inside_relative_plugins_dir() {
        let current_dir = std::env::current_dir().unwrap();
        let temp = tempfile::Builder::new()
            .prefix("vortex-provenance-containment-")
            .tempdir_in(&current_dir)
            .unwrap();
        let relative_root = temp.path().strip_prefix(&current_dir).unwrap();
        let relative_plugins_dir = relative_root.join("plugins");
        let absolute_provenance_path = temp.path().join("plugins").join("provenance.json");

        let result = ExtismPluginLoader::new_with_provenance_path(
            relative_plugins_dir,
            Arc::new(SharedHostResources::new()),
            absolute_provenance_path,
        );

        assert!(matches!(result, Err(DomainError::ValidationError(_))));
    }

    #[test]
    fn test_new_rejects_unresolved_parent_traversal_into_plugins_dir() {
        let temp = TempDir::new().unwrap();
        let plugins_dir = temp.path().join("plugins");
        std::fs::create_dir(&plugins_dir).unwrap();
        let provenance_path = temp
            .path()
            .join("missing")
            .join("..")
            .join("plugins")
            .join("provenance.json");

        let result = ExtismPluginLoader::new_with_provenance_path(
            plugins_dir,
            Arc::new(SharedHostResources::new()),
            provenance_path,
        );

        assert!(matches!(result, Err(DomainError::ValidationError(_))));
    }

    #[test]
    fn test_new_rejects_case_only_provenance_alias_inside_plugins_dir() {
        let temp = TempDir::new().unwrap();
        let plugins_dir = temp.path().join("Plugins");
        std::fs::create_dir(&plugins_dir).unwrap();
        let canonical_plugins = std::fs::canonicalize(&plugins_dir).unwrap();
        let case_only_parent = temp.path().join("plugins");
        let case_only_parent_is_alias =
            std::fs::canonicalize(&case_only_parent).is_ok_and(|path| path == canonical_plugins);
        let provenance_path = case_only_parent.join("provenance.json");

        let result = ExtismPluginLoader::new_with_provenance_path(
            plugins_dir,
            Arc::new(SharedHostResources::new()),
            provenance_path,
        );

        if case_only_parent_is_alias {
            assert!(matches!(result, Err(DomainError::ValidationError(_))));
        } else {
            assert!(
                result.is_ok(),
                "distinct case-sensitive path must remain valid"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_new_rejects_provenance_symlink_resolving_inside_plugins_dir() {
        let temp = TempDir::new().unwrap();
        let plugins_dir = temp.path().join("plugins");
        std::fs::create_dir(&plugins_dir).unwrap();
        let plugins_alias = temp.path().join("plugins-alias");
        std::os::unix::fs::symlink(&plugins_dir, &plugins_alias).unwrap();

        let result = ExtismPluginLoader::new_with_provenance_path(
            plugins_dir,
            Arc::new(SharedHostResources::new()),
            plugins_alias.join("provenance.json"),
        );

        assert!(matches!(result, Err(DomainError::ValidationError(_))));
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

    #[test]
    fn test_load_official_from_dir_grants_ytdlp_for_verified_files() {
        use sha2::{Digest, Sha256};

        let tmp = TempDir::new().unwrap();
        let plugins_dir = tmp.path().join("plugins");
        let staged = tmp.path().join("staging").join("vortex-mod-youtube");
        let (wasm, manifest) = setup_ytdlp_importing_plugin(&staged, "vortex-mod-youtube");
        let loader =
            ExtismPluginLoader::new(plugins_dir, Arc::new(SharedHostResources::new())).unwrap();
        let provenance =
            crate::domain::ports::driven::plugin_store_client::OfficialPluginProvenance {
                name: "vortex-mod-youtube".into(),
                version: "1.0.0".into(),
                wasm_sha256: hex::encode(Sha256::digest(&wasm)),
                manifest_sha256: hex::encode(Sha256::digest(&manifest)),
            };

        let result = loader.load_official_from_dir(&staged, &provenance);

        assert!(
            result.is_ok(),
            "official verified plugin should load: {result:?}"
        );
        assert!(loader.registry().contains("vortex-mod-youtube"));
    }

    #[test]
    fn test_local_official_name_does_not_receive_ytdlp_grant() {
        let tmp = TempDir::new().unwrap();
        let plugin_dir = tmp.path().join("vortex-mod-youtube");
        setup_ytdlp_importing_plugin(&plugin_dir, "vortex-mod-youtube");
        let loader = ExtismPluginLoader::new(
            tmp.path().to_path_buf(),
            Arc::new(SharedHostResources::new()),
        )
        .unwrap();
        let (manifest, _) = parse_manifest(&plugin_dir).unwrap();

        let result = loader.load(&manifest);

        assert!(result.is_err());
        assert!(!loader.registry().contains("vortex-mod-youtube"));
    }

    #[test]
    fn test_reload_revalidates_wasm_before_restoring_ytdlp_grant() {
        use sha2::{Digest, Sha256};

        let tmp = TempDir::new().unwrap();
        let plugins_dir = tmp.path().join("plugins");
        let staged = tmp.path().join("staging").join("vortex-mod-youtube");
        let (wasm, manifest_bytes) = setup_ytdlp_importing_plugin(&staged, "vortex-mod-youtube");
        let loader =
            ExtismPluginLoader::new(plugins_dir.clone(), Arc::new(SharedHostResources::new()))
                .unwrap();
        let provenance =
            crate::domain::ports::driven::plugin_store_client::OfficialPluginProvenance {
                name: "vortex-mod-youtube".into(),
                version: "1.0.0".into(),
                wasm_sha256: hex::encode(Sha256::digest(&wasm)),
                manifest_sha256: hex::encode(Sha256::digest(&manifest_bytes)),
            };
        loader.load_official_from_dir(&staged, &provenance).unwrap();

        let installed = plugins_dir
            .join("vortex-mod-youtube")
            .join("vortex-mod-youtube.wasm");
        let mut tampered = wasm;
        tampered.push(b'\n');
        std::fs::write(installed, tampered).unwrap();
        loader.unload("vortex-mod-youtube").unwrap();
        let (manifest, _) = parse_manifest(&plugins_dir.join("vortex-mod-youtube")).unwrap();

        let result = loader.load(&manifest);

        assert!(result.is_err());
        assert!(!loader.registry().contains("vortex-mod-youtube"));
    }

    #[test]
    fn test_startup_load_reuses_persisted_provenance_after_checksum_revalidation() {
        use sha2::{Digest, Sha256};

        let tmp = TempDir::new().unwrap();
        let plugins_dir = tmp.path().join("plugins");
        let staged = tmp.path().join("staging").join("vortex-mod-youtube");
        let (wasm, manifest_bytes) = setup_ytdlp_importing_plugin(&staged, "vortex-mod-youtube");
        let provenance =
            crate::domain::ports::driven::plugin_store_client::OfficialPluginProvenance {
                name: "vortex-mod-youtube".into(),
                version: "1.0.0".into(),
                wasm_sha256: hex::encode(Sha256::digest(&wasm)),
                manifest_sha256: hex::encode(Sha256::digest(&manifest_bytes)),
            };
        {
            let loader =
                ExtismPluginLoader::new(plugins_dir.clone(), Arc::new(SharedHostResources::new()))
                    .unwrap();
            loader.load_official_from_dir(&staged, &provenance).unwrap();
        }
        let loader =
            ExtismPluginLoader::new(plugins_dir.clone(), Arc::new(SharedHostResources::new()))
                .unwrap();
        let (manifest, _) = parse_manifest(&plugins_dir.join("vortex-mod-youtube")).unwrap();

        let result = loader.load(&manifest);

        assert!(
            result.is_ok(),
            "startup load should retain grant: {result:?}"
        );
    }
}
