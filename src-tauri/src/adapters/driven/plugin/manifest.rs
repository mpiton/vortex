//! Parse `plugin.toml` files into domain [`PluginManifest`].

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::domain::error::DomainError;
use crate::domain::model::plugin::{PluginCategory, PluginInfo, PluginManifest};

#[derive(Deserialize)]
struct RawManifest {
    plugin: RawPluginSection,
    capabilities: Option<RawCapabilities>,
}

#[derive(Deserialize)]
struct RawPluginSection {
    name: String,
    version: String,
    category: String,
    author: String,
    description: String,
    _license: Option<String>,
    min_vortex_version: Option<String>,
}

#[derive(Deserialize)]
struct RawCapabilities {
    http: Option<bool>,
    filesystem: Option<bool>,
    subprocess: Option<Vec<String>>,
}

/// Parse a plugin directory containing `plugin.toml` and a `.wasm` file.
///
/// Returns the domain manifest and the path to the `.wasm` file.
pub fn parse_manifest(dir: &Path) -> Result<(PluginManifest, PathBuf), DomainError> {
    let toml_path = dir.join("plugin.toml");
    let content = std::fs::read_to_string(&toml_path).map_err(|e| {
        DomainError::PluginError(format!(
            "failed to read plugin.toml at {}: {e}",
            toml_path.display()
        ))
    })?;

    let raw: RawManifest = toml::from_str(&content).map_err(|e| {
        DomainError::PluginError(format!(
            "invalid plugin.toml at {}: {e}",
            toml_path.display()
        ))
    })?;

    // Enforce convention: directory name must match plugin name
    if let Some(dir_name) = dir.file_name().and_then(|n| n.to_str())
        && dir_name != raw.plugin.name
    {
        return Err(DomainError::PluginError(format!(
            "directory name '{dir_name}' does not match plugin name '{}'",
            raw.plugin.name
        )));
    }

    let category = parse_category(&raw.plugin.category)?;
    let info = PluginInfo::new(
        raw.plugin.name,
        raw.plugin.version,
        raw.plugin.description,
        raw.plugin.author,
        category,
    );

    let caps = raw
        .capabilities
        .as_ref()
        .map(build_capabilities)
        .unwrap_or_default();

    let mut manifest = PluginManifest::new(info).with_capabilities(caps);
    if let Some(v) = raw.plugin.min_vortex_version {
        manifest = manifest.with_min_version(v);
    }

    let wasm_path = find_wasm_file(dir)?;
    Ok((manifest, wasm_path))
}

fn parse_category(s: &str) -> Result<PluginCategory, DomainError> {
    match s {
        "crawler" => Ok(PluginCategory::Crawler),
        "hoster" => Ok(PluginCategory::Hoster),
        "debrid" => Ok(PluginCategory::Debrid),
        "container" => Ok(PluginCategory::Container),
        "captcha" => Ok(PluginCategory::Captcha),
        "extractor" => Ok(PluginCategory::Extractor),
        "notifier" => Ok(PluginCategory::Notifier),
        "utility" => Ok(PluginCategory::Utility),
        other => Err(DomainError::PluginError(format!(
            "unknown plugin category: '{other}'"
        ))),
    }
}

fn build_capabilities(caps: &RawCapabilities) -> Vec<String> {
    let mut result = Vec::new();
    if caps.http.unwrap_or(false) {
        result.push("http".to_string());
    }
    if caps.filesystem.unwrap_or(false) {
        result.push("filesystem".to_string());
    }
    if let Some(progs) = &caps.subprocess {
        for prog in progs {
            result.push(format!("subprocess:{prog}"));
        }
    }
    result
}

/// Find exactly one `.wasm` file in the plugin directory.
/// Returns an error if zero or more than one `.wasm` file is found.
pub fn find_wasm_file(dir: &Path) -> Result<PathBuf, DomainError> {
    let entries = std::fs::read_dir(dir).map_err(|e| {
        DomainError::PluginError(format!("cannot read plugin dir {}: {e}", dir.display()))
    })?;

    let mut wasm_files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| DomainError::PluginError(format!("dir entry error: {e}")))?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("wasm") {
            wasm_files.push(path);
        }
    }

    match wasm_files.len() {
        0 => Err(DomainError::PluginError(format!(
            "no .wasm file found in {}",
            dir.display()
        ))),
        1 => Ok(wasm_files.into_iter().next().expect("checked len == 1")),
        n => Err(DomainError::PluginError(format!(
            "expected exactly one .wasm file in {}, found {n}",
            dir.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_plugin_toml(dir: &Path, content: &str) {
        let mut f = std::fs::File::create(dir.join("plugin.toml")).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    fn write_dummy_wasm(dir: &Path, name: &str) {
        std::fs::File::create(dir.join(name)).unwrap();
    }

    #[test]
    fn test_parse_manifest_valid() {
        let tmp = TempDir::new().unwrap();
        let plugin_dir = tmp.path().join("my-hoster");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        write_plugin_toml(
            &plugin_dir,
            r#"
[plugin]
name = "my-hoster"
version = "1.0.0"
category = "hoster"
author = "Alice"
description = "A hoster plugin"
min_vortex_version = "0.5.0"

[capabilities]
http = true
filesystem = false
subprocess = ["ffmpeg"]
"#,
        );
        write_dummy_wasm(&plugin_dir, "my-hoster.wasm");

        let (manifest, wasm_path) = parse_manifest(&plugin_dir).unwrap();
        assert_eq!(manifest.info().name(), "my-hoster");
        assert_eq!(manifest.info().version(), "1.0.0");
        assert_eq!(manifest.info().category(), PluginCategory::Hoster);
        assert_eq!(manifest.min_vortex_version(), Some("0.5.0"));
        assert!(manifest.has_capability("http"));
        assert!(!manifest.has_capability("filesystem"));
        assert!(manifest.has_capability("subprocess:ffmpeg"));
        assert!(wasm_path.ends_with("my-hoster.wasm"));
    }

    #[test]
    fn test_parse_manifest_missing_field() {
        let tmp = TempDir::new().unwrap();
        let plugin_dir = tmp.path().join("bad-plugin");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        // Missing `version` field
        write_plugin_toml(
            &plugin_dir,
            r#"
[plugin]
name = "bad-plugin"
category = "hoster"
author = "Alice"
description = "Missing version"
"#,
        );
        write_dummy_wasm(&plugin_dir, "bad-plugin.wasm");

        let result = parse_manifest(&plugin_dir);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DomainError::PluginError(_)));
    }

    #[test]
    fn test_parse_manifest_unknown_category() {
        let tmp = TempDir::new().unwrap();
        let plugin_dir = tmp.path().join("weird");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        write_plugin_toml(
            &plugin_dir,
            r#"
[plugin]
name = "weird"
version = "1.0.0"
category = "spaceship"
author = "Bob"
description = "Unknown category"
"#,
        );
        write_dummy_wasm(&plugin_dir, "weird.wasm");

        let result = parse_manifest(&plugin_dir);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("spaceship"),
            "expected category error, got: {err_msg}"
        );
    }

    #[test]
    fn test_parse_manifest_missing_wasm() {
        let tmp = TempDir::new().unwrap();
        let plugin_dir = tmp.path().join("no-wasm");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        write_plugin_toml(
            &plugin_dir,
            r#"
[plugin]
name = "no-wasm"
version = "1.0.0"
category = "utility"
author = "Charlie"
description = "No wasm file"
"#,
        );

        let result = parse_manifest(&plugin_dir);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("no .wasm"),
            "expected wasm error, got: {err_msg}"
        );
    }

    #[test]
    fn test_parse_manifest_dir_name_mismatch() {
        let tmp = TempDir::new().unwrap();
        let plugin_dir = tmp.path().join("wrong-dir-name");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        write_plugin_toml(
            &plugin_dir,
            r#"
[plugin]
name = "actual-name"
version = "1.0.0"
category = "utility"
author = "Alice"
description = "Dir name mismatch"
"#,
        );
        write_dummy_wasm(&plugin_dir, "actual-name.wasm");

        let result = parse_manifest(&plugin_dir);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("does not match"),
            "expected name mismatch error, got: {err_msg}"
        );
    }

    #[test]
    fn test_parse_category_valid() {
        assert!(matches!(
            parse_category("crawler"),
            Ok(PluginCategory::Crawler)
        ));
        assert!(matches!(
            parse_category("hoster"),
            Ok(PluginCategory::Hoster)
        ));
        assert!(matches!(
            parse_category("debrid"),
            Ok(PluginCategory::Debrid)
        ));
        assert!(matches!(
            parse_category("container"),
            Ok(PluginCategory::Container)
        ));
        assert!(matches!(
            parse_category("captcha"),
            Ok(PluginCategory::Captcha)
        ));
        assert!(matches!(
            parse_category("extractor"),
            Ok(PluginCategory::Extractor)
        ));
        assert!(matches!(
            parse_category("notifier"),
            Ok(PluginCategory::Notifier)
        ));
        assert!(matches!(
            parse_category("utility"),
            Ok(PluginCategory::Utility)
        ));
    }

    #[test]
    fn test_parse_category_invalid() {
        let result = parse_category("unknown");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DomainError::PluginError(_)));
    }
}
