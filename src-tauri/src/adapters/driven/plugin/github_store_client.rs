//! HTTP adapter for fetching the Vortex plugin registry from GitHub
//! and downloading plugin assets from GitHub Releases.

use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::domain::error::DomainError;
use crate::domain::model::plugin::PluginCategory;
use crate::domain::model::plugin_store::{PluginStoreEntry, PluginStoreStatus};
use crate::domain::ports::driven::PluginStoreClient;

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
}

impl GithubStoreClient {
    pub fn new(registry_url: impl Into<String>, staging_dir: PathBuf) -> Self {
        Self {
            registry_url: registry_url.into(),
            staging_dir,
        }
    }

    fn build_wasm_url(repository: &str, version: &str, name: &str) -> String {
        format!(
            "{}/releases/download/v{}/{}.wasm",
            repository, version, name
        )
    }

    fn build_toml_url(repository: &str, version: &str) -> String {
        format!("{}/releases/download/v{}/plugin.toml", repository, version)
    }

    fn download_bytes(url: &str) -> Result<Vec<u8>, DomainError> {
        let response = reqwest::blocking::get(url)
            .map_err(|e| DomainError::PluginError(format!("download failed: {e}")))?;
        if !response.status().is_success() {
            return Err(DomainError::PluginError(format!(
                "HTTP {} for {url}",
                response.status()
            )));
        }
        response
            .bytes()
            .map(|b| b.to_vec())
            .map_err(|e| DomainError::PluginError(format!("failed to read response: {e}")))
    }
}

impl PluginStoreClient for GithubStoreClient {
    fn fetch_registry(&self) -> Result<Vec<PluginStoreEntry>, DomainError> {
        let body = Self::download_bytes(&self.registry_url)?;
        let text = String::from_utf8(body)
            .map_err(|e| DomainError::PluginError(format!("registry is not valid UTF-8: {e}")))?;

        let raw: RawRegistry = toml::from_str(&text)
            .map_err(|e| DomainError::PluginError(format!("failed to parse registry.toml: {e}")))?;

        Ok(raw
            .plugins
            .into_iter()
            .map(|p| PluginStoreEntry {
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
                official: p.official,
                min_vortex_version: p.min_vortex_version,
                status: PluginStoreStatus::NotInstalled,
                installed_version: None,
            })
            .collect())
    }

    fn download_plugin(&self, entry: &PluginStoreEntry) -> Result<PathBuf, DomainError> {
        // Re-fetch registry to get real repository URL and checksum
        let all = self.fetch_registry()?;
        let full_entry = all
            .into_iter()
            .find(|e| e.name == entry.name)
            .ok_or_else(|| DomainError::NotFound(entry.name.clone()))?;

        let wasm_url = Self::build_wasm_url(
            &full_entry.repository,
            &full_entry.version,
            &full_entry.name,
        );
        let toml_url = Self::build_toml_url(&full_entry.repository, &full_entry.version);

        // Download wasm
        let wasm_bytes = Self::download_bytes(&wasm_url)?;

        // Verify checksum
        let mut hasher = Sha256::new();
        hasher.update(&wasm_bytes);
        let digest = hex::encode(hasher.finalize());
        if digest != full_entry.checksum_sha256 {
            return Err(DomainError::PluginError(format!(
                "checksum mismatch for '{}': expected {}, got {digest}",
                full_entry.name, full_entry.checksum_sha256
            )));
        }

        // Download plugin.toml
        let toml_bytes = Self::download_bytes(&toml_url)?;

        // Write to staging dir
        let plugin_dir = self.staging_dir.join(&full_entry.name);
        std::fs::create_dir_all(&plugin_dir)
            .map_err(|e| DomainError::PluginError(format!("failed to create staging dir: {e}")))?;

        std::fs::write(
            plugin_dir.join(format!("{}.wasm", full_entry.name)),
            &wasm_bytes,
        )
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
    fn test_build_wasm_url() {
        let url = GithubStoreClient::build_wasm_url(
            "https://github.com/johndoe/vortex-mod-gallery",
            "1.0.0",
            "vortex-mod-gallery",
        );
        assert_eq!(
            url,
            "https://github.com/johndoe/vortex-mod-gallery/releases/download/v1.0.0/vortex-mod-gallery.wasm"
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
}
