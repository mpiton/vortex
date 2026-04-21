//! HTTP adapter for fetching the Vortex plugin registry from GitHub
//! and downloading plugin assets from GitHub Releases.

use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::domain::error::DomainError;
use crate::domain::model::plugin::PluginCategory;
use crate::domain::model::plugin_store::{PluginStoreEntry, PluginStoreStatus};
use crate::domain::ports::driven::PluginStoreClient;

const MAX_REGISTRY_BYTES: usize = 512 * 1024; // 512 KB
const MAX_WASM_BYTES: usize = 100 * 1024 * 1024; // 100 MB

/// Raw TOML shape for a `[[plugin]]` entry in the registry.
#[derive(Debug, serde::Deserialize)]
struct RawPluginEntry {
    name: String,
    description: String,
    author: String,
    version: String,
    category: String,
    repository: String,
    checksum_sha256: String,
    #[serde(default)]
    checksum_sha256_toml: Option<String>,
    #[serde(default)]
    official: bool,
    min_vortex_version: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct RawRegistry {
    #[serde(rename = "plugin")]
    plugins: Vec<RawPluginEntry>,
}

pub struct GithubStoreClient {
    /// URL to the raw registry.toml on GitHub.
    registry_url: String,
    /// Directory where downloaded plugins are staged before installation.
    staging_dir: PathBuf,
    /// Reusable HTTP client with timeouts configured.
    http_client: reqwest::blocking::Client,
}

impl GithubStoreClient {
    pub fn new(registry_url: impl Into<String>, staging_dir: PathBuf) -> Self {
        let http_client = reqwest::blocking::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_default();
        Self {
            registry_url: registry_url.into(),
            staging_dir,
            http_client,
        }
    }

    fn build_wasm_url(repository: &str, version: &str, name: &str) -> String {
        // Cargo replaces hyphens with underscores when building cdylib
        // targets (Rust identifiers disallow hyphens), so the release
        // asset is `vortex_mod_<plugin>.wasm` while the registry entry
        // keeps the kebab-case id. Normalise to the on-disk filename.
        let wasm_name = name.replace('-', "_");
        format!(
            "{}/releases/download/v{}/{}.wasm",
            repository, version, wasm_name
        )
    }

    fn build_toml_url(repository: &str, version: &str) -> String {
        format!("{}/releases/download/v{}/plugin.toml", repository, version)
    }

    /// Download bytes from `url`, capping at `max_bytes`.
    fn download_bytes(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, DomainError> {
        let response = self
            .http_client
            .get(url)
            .send()
            .map_err(|e| DomainError::PluginError(format!("download failed: {e}")))?;
        if !response.status().is_success() {
            return Err(DomainError::PluginError(format!(
                "HTTP {} for {url}",
                response.status()
            )));
        }
        let bytes = response
            .bytes()
            .map_err(|e| DomainError::PluginError(format!("failed to read response: {e}")))?;
        if bytes.len() > max_bytes {
            return Err(DomainError::PluginError(format!(
                "response from {url} exceeds size limit ({} > {max_bytes} bytes)",
                bytes.len()
            )));
        }
        Ok(bytes.to_vec())
    }
}

/// Validate that a plugin name is safe and well-formed.
fn validate_plugin_name(name: &str) -> Result<(), DomainError> {
    if name.is_empty() || name.len() > 64 {
        return Err(DomainError::ValidationError(format!(
            "plugin name '{name}' is invalid (must be 1–64 characters)"
        )));
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(DomainError::ValidationError(format!(
            "plugin name '{name}' contains invalid characters (only a-z, 0-9, '-' allowed)"
        )));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(DomainError::ValidationError(format!(
            "plugin name '{name}' must not start or end with '-'"
        )));
    }
    Ok(())
}

impl PluginStoreClient for GithubStoreClient {
    fn fetch_registry(&self) -> Result<Vec<PluginStoreEntry>, DomainError> {
        let body = self.download_bytes(&self.registry_url, MAX_REGISTRY_BYTES)?;
        let text = String::from_utf8(body)
            .map_err(|e| DomainError::PluginError(format!("registry is not valid UTF-8: {e}")))?;

        let raw: RawRegistry = toml::from_str(&text)
            .map_err(|e| DomainError::PluginError(format!("failed to parse registry.toml: {e}")))?;

        Ok(raw
            .plugins
            .into_iter()
            .filter_map(|p| {
                if let Err(e) = validate_plugin_name(&p.name) {
                    tracing::warn!(error = %e, "skipping registry entry with invalid plugin name");
                    return None;
                }
                Some(PluginStoreEntry {
                    name: p.name,
                    description: p.description,
                    author: p.author,
                    version: p.version,
                    category: p
                        .category
                        .parse::<PluginCategory>()
                        .unwrap_or(PluginCategory::Utility),
                    repository: p.repository,
                    checksum_sha256: p.checksum_sha256,
                    checksum_sha256_toml: p.checksum_sha256_toml,
                    official: p.official,
                    min_vortex_version: p.min_vortex_version,
                    status: PluginStoreStatus::NotInstalled,
                    installed_version: None,
                })
            })
            .collect())
    }

    fn download_plugin(&self, entry: &PluginStoreEntry) -> Result<PathBuf, DomainError> {
        // Validate name to prevent path traversal
        validate_plugin_name(&entry.name)?;

        // Guard against placeholder (all-zero) checksums
        const ZERO_CHECKSUM: &str =
            "0000000000000000000000000000000000000000000000000000000000000000";
        if entry.checksum_sha256 == ZERO_CHECKSUM {
            return Err(DomainError::PluginError(format!(
                "plugin '{}': checksum is a placeholder — registry must be updated before installation",
                entry.name
            )));
        }

        let wasm_url = Self::build_wasm_url(&entry.repository, &entry.version, &entry.name);
        let toml_url = Self::build_toml_url(&entry.repository, &entry.version);

        // Download + verify WASM
        let wasm_bytes = self.download_bytes(&wasm_url, MAX_WASM_BYTES)?;

        let mut hasher = Sha256::new();
        hasher.update(&wasm_bytes);
        let digest = hex::encode(hasher.finalize());
        if digest != entry.checksum_sha256 {
            return Err(DomainError::PluginError(format!(
                "checksum mismatch for '{}': expected {}, got {digest}",
                entry.name, entry.checksum_sha256
            )));
        }

        // Download plugin.toml
        let toml_bytes = self.download_bytes(&toml_url, MAX_REGISTRY_BYTES)?;

        // Verify plugin.toml checksum if provided
        if let Some(ref expected_toml_checksum) = entry.checksum_sha256_toml {
            let mut hasher = Sha256::new();
            hasher.update(&toml_bytes);
            let toml_digest = hex::encode(hasher.finalize());
            if toml_digest != *expected_toml_checksum {
                return Err(DomainError::PluginError(format!(
                    "plugin.toml checksum mismatch for '{}': expected {expected_toml_checksum}, got {toml_digest}",
                    entry.name
                )));
            }
        }

        // Path-safe staging directory (name already validated above)
        let plugin_dir = self.staging_dir.join(&entry.name);
        std::fs::create_dir_all(&plugin_dir)
            .map_err(|e| DomainError::PluginError(format!("failed to create staging dir: {e}")))?;

        std::fs::write(plugin_dir.join(format!("{}.wasm", entry.name)), &wasm_bytes)
            .map_err(|e| DomainError::PluginError(format!("failed to write wasm: {e}")))?;

        std::fs::write(plugin_dir.join("plugin.toml"), &toml_bytes)
            .map_err(|e| DomainError::PluginError(format!("failed to write plugin.toml: {e}")))?;

        Ok(plugin_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_wasm_url_normalises_hyphens_to_underscores() {
        let url = GithubStoreClient::build_wasm_url(
            "https://github.com/johndoe/vortex-mod-gallery",
            "1.0.0",
            "vortex-mod-gallery",
        );
        assert_eq!(
            url,
            "https://github.com/johndoe/vortex-mod-gallery/releases/download/v1.0.0/vortex_mod_gallery.wasm"
        );
    }

    #[test]
    fn test_build_wasm_url_single_word_name_unchanged() {
        // `validate_plugin_name` only admits alphanumeric + hyphen, so a
        // single-word name is the realistic "no replacement needed" case.
        let url = GithubStoreClient::build_wasm_url(
            "https://github.com/johndoe/singleword",
            "2.3.4",
            "singleword",
        );
        assert_eq!(
            url,
            "https://github.com/johndoe/singleword/releases/download/v2.3.4/singleword.wasm"
        );
    }

    #[test]
    fn test_build_toml_url() {
        let url = GithubStoreClient::build_toml_url(
            "https://github.com/johndoe/vortex-mod-gallery",
            "0.3.1",
        );
        assert_eq!(
            url,
            "https://github.com/johndoe/vortex-mod-gallery/releases/download/v0.3.1/plugin.toml"
        );
    }

    #[test]
    fn test_fetch_registry_parses_valid_toml() {
        let toml = r#"
[[plugin]]
name = "vortex-mod-test"
description = "Test plugin"
author = "tester"
version = "1.0.0"
category = "utility"
repository = "https://github.com/tester/vortex-mod-test"
checksum_sha256 = "abc123"
official = false
"#;
        let raw: RawRegistry = toml::from_str(toml).unwrap();
        assert_eq!(raw.plugins.len(), 1);
        assert_eq!(raw.plugins[0].name, "vortex-mod-test");
        assert!(!raw.plugins[0].official);
    }

    #[test]
    fn test_checksum_verification_logic() {
        let wasm = b"\x00asm\x01\x00\x00\x00";
        let mut hasher = Sha256::new();
        hasher.update(wasm);
        let real_digest = hex::encode(hasher.finalize());

        // Real checksum should NOT equal a fake one
        assert_ne!(
            real_digest,
            "0000000000000000000000000000000000000000000000000000000000000000"
        );
        // But should equal itself
        let mut hasher2 = Sha256::new();
        hasher2.update(wasm);
        let digest2 = hex::encode(hasher2.finalize());
        assert_eq!(real_digest, digest2);
    }

    #[test]
    fn test_validate_plugin_name_rejects_traversal() {
        assert!(validate_plugin_name("../evil").is_err());
        assert!(validate_plugin_name("../../etc/passwd").is_err());
        assert!(validate_plugin_name("").is_err());
        assert!(validate_plugin_name("valid-plugin-name").is_ok());
        assert!(validate_plugin_name("vortex-mod-test").is_ok());
    }

    #[test]
    fn test_validate_plugin_name_rejects_leading_trailing_dash() {
        assert!(validate_plugin_name("-bad").is_err());
        assert!(validate_plugin_name("bad-").is_err());
        assert!(validate_plugin_name("good-plugin").is_ok());
    }

    #[test]
    fn test_validate_plugin_name_rejects_too_long() {
        let long = "a".repeat(65);
        assert!(validate_plugin_name(&long).is_err());
        let ok = "a".repeat(64);
        assert!(validate_plugin_name(&ok).is_ok());
    }

    #[test]
    fn test_download_plugin_rejects_zero_checksum() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let client = GithubStoreClient::new("http://localhost/registry.toml", tmp.path().into());
        let entry = PluginStoreEntry {
            name: "my-plugin".into(),
            description: "test".into(),
            author: "author".into(),
            version: "1.0.0".into(),
            category: PluginCategory::Utility,
            repository: "https://github.com/author/my-plugin".into(),
            checksum_sha256: "0000000000000000000000000000000000000000000000000000000000000000"
                .into(),
            checksum_sha256_toml: None,
            official: false,
            min_vortex_version: None,
            status: PluginStoreStatus::NotInstalled,
            installed_version: None,
        };
        let result = client.download_plugin(&entry);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("placeholder"),
            "expected 'placeholder' in: {msg}"
        );
    }
}
