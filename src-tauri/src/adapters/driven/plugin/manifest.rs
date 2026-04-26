//! Parse `plugin.toml` files into domain [`PluginManifest`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::domain::error::DomainError;
use crate::domain::model::plugin::{
    ConfigField, ConfigFieldType, PluginCategory, PluginConfigSchema, PluginInfo, PluginManifest,
    unsupported_regex_feature,
};

#[derive(Deserialize)]
struct RawManifest {
    plugin: RawPluginSection,
    capabilities: Option<RawCapabilities>,
    #[serde(default)]
    config: HashMap<String, RawConfigEntry>,
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

#[derive(Deserialize)]
struct RawConfigEntry {
    #[serde(rename = "type")]
    field_type: Option<String>,
    default: Option<toml::Value>,
    description: Option<String>,
    options: Option<Vec<toml::Value>>,
    min: Option<f64>,
    max: Option<f64>,
    regex: Option<String>,
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
    let config_defaults = build_config_defaults(&raw.config)?;
    let config_schema = build_config_schema(&raw.config)?;

    let mut manifest = PluginManifest::new(info)
        .with_capabilities(caps)
        .with_config_defaults(config_defaults)
        .with_config_schema(config_schema);
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

fn build_config_defaults(
    raw_config: &HashMap<String, RawConfigEntry>,
) -> Result<HashMap<String, String>, DomainError> {
    let mut defaults = HashMap::new();
    for (key, entry) in raw_config {
        let Some(value) = &entry.default else {
            continue;
        };
        defaults.insert(key.clone(), encode_config_default(value)?);
    }
    Ok(defaults)
}

fn build_config_schema(
    raw_config: &HashMap<String, RawConfigEntry>,
) -> Result<PluginConfigSchema, DomainError> {
    let mut schema = PluginConfigSchema::new();
    for (key, entry) in raw_config {
        let field_type = match entry.field_type.as_deref() {
            Some(t) => t
                .parse::<ConfigFieldType>()
                .map_err(|e| DomainError::PluginError(format!("config field '{key}': {e}")))?,
            None => ConfigFieldType::String,
        };

        let mut field = ConfigField::new(field_type);
        if let Some(default) = &entry.default {
            field = field.with_default(encode_config_default(default)?);
        }
        if let Some(desc) = &entry.description {
            field = field.with_description(desc.clone());
        }
        if let Some(options) = &entry.options {
            let opts = options
                .iter()
                .map(encode_config_default)
                .collect::<Result<Vec<_>, _>>()?;
            field = field.with_options(opts);
        }
        if let Some(min) = entry.min {
            field = field.with_min(min);
        }
        if let Some(max) = entry.max {
            field = field.with_max(max);
        }
        if let Some(regex) = &entry.regex {
            if let Some(bad) = unsupported_regex_feature(regex) {
                return Err(DomainError::PluginError(format!(
                    "config field '{key}' regex '{regex}' uses unsupported feature '{bad}' (alternation, groups and counted quantifiers are not implemented)"
                )));
            }
            field = field.with_regex(regex.clone());
        }
        if let Some(default) = field.default_value() {
            field.validate(default).map_err(|e| {
                DomainError::PluginError(format!("config field '{key}' has invalid default: {e}"))
            })?;
        }
        schema.insert(key.clone(), field);
    }
    Ok(schema)
}

fn encode_config_default(value: &toml::Value) -> Result<String, DomainError> {
    match value {
        toml::Value::String(s) => Ok(s.clone()),
        toml::Value::Integer(i) => Ok(i.to_string()),
        toml::Value::Float(f) => Ok(f.to_string()),
        toml::Value::Boolean(b) => Ok(b.to_string()),
        toml::Value::Datetime(dt) => Ok(dt.to_string()),
        toml::Value::Array(_) | toml::Value::Table(_) => serde_json::to_string(value)
            .map_err(|e| DomainError::PluginError(format!("invalid config default value: {e}"))),
    }
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
    fn test_parse_manifest_extracts_config_defaults() {
        let tmp = TempDir::new().unwrap();
        let plugin_dir = tmp.path().join("with-config");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        write_plugin_toml(
            &plugin_dir,
            r#"
[plugin]
name = "with-config"
version = "1.0.0"
category = "crawler"
author = "Alice"
description = "Config defaults"

[config]
default_quality = { type = "string", default = "720p", options = ["360p", "720p"] }
extract_audio_only = { type = "boolean", default = false }
subtitle_languages = { type = "array", default = ["en", "fr"] }
"#,
        );
        write_dummy_wasm(&plugin_dir, "with-config.wasm");

        let (manifest, _) = parse_manifest(&plugin_dir).unwrap();
        assert_eq!(
            manifest
                .config_defaults()
                .get("default_quality")
                .map(String::as_str),
            Some("720p")
        );
        assert_eq!(
            manifest
                .config_defaults()
                .get("extract_audio_only")
                .map(String::as_str),
            Some("false")
        );
        assert_eq!(
            manifest
                .config_defaults()
                .get("subtitle_languages")
                .map(String::as_str),
            Some("[\"en\",\"fr\"]")
        );
    }

    #[test]
    fn test_encode_config_default_covers_remaining_scalar_and_table_branches() {
        let integer = toml::Value::Integer(720);
        let float = toml::Value::Float(1.5);
        let datetime = toml::Value::Datetime("1979-05-27T07:32:00Z".parse().unwrap());
        let table = toml::from_str::<toml::Value>(
            r#"
enabled = true
nested = { quality = "720p" }
"#,
        )
        .unwrap();

        assert_eq!(encode_config_default(&integer).unwrap(), "720");
        assert_eq!(encode_config_default(&float).unwrap(), "1.5");
        assert_eq!(
            encode_config_default(&datetime).unwrap(),
            "1979-05-27T07:32:00Z"
        );
        assert_eq!(
            encode_config_default(&table).unwrap(),
            serde_json::to_string(&table).unwrap()
        );
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

    #[test]
    fn test_parse_manifest_extracts_full_config_schema() {
        use crate::domain::model::plugin::ConfigFieldType;

        let tmp = TempDir::new().unwrap();
        let plugin_dir = tmp.path().join("with-schema");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        write_plugin_toml(
            &plugin_dir,
            r#"
[plugin]
name = "with-schema"
version = "1.0.0"
category = "crawler"
author = "Alice"
description = "Schema fields"

[config]
default_quality = { type = "enum", default = "1080p", options = ["360p", "720p", "1080p"], description = "Preferred resolution" }
extract_audio_only = { type = "boolean", default = false }
max_retries = { type = "integer", default = 3, min = 0, max = 10 }
"#,
        );
        write_dummy_wasm(&plugin_dir, "with-schema.wasm");

        let (manifest, _) = parse_manifest(&plugin_dir).unwrap();
        let schema = manifest.config_schema();
        assert_eq!(schema.len(), 3);

        let q = schema.get("default_quality").unwrap();
        assert_eq!(q.field_type(), ConfigFieldType::Enum);
        assert_eq!(q.default_value(), Some("1080p"));
        assert_eq!(q.options(), &["360p", "720p", "1080p"]);
        assert_eq!(q.description(), Some("Preferred resolution"));

        let a = schema.get("extract_audio_only").unwrap();
        assert_eq!(a.field_type(), ConfigFieldType::Boolean);
        assert_eq!(a.default_value(), Some("false"));

        let r = schema.get("max_retries").unwrap();
        assert_eq!(r.field_type(), ConfigFieldType::Integer);
        assert_eq!(r.min(), Some(0.0));
        assert_eq!(r.max(), Some(10.0));
    }

    #[test]
    fn test_parse_manifest_missing_type_defaults_to_string() {
        use crate::domain::model::plugin::ConfigFieldType;

        let tmp = TempDir::new().unwrap();
        let plugin_dir = tmp.path().join("loose-config");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        write_plugin_toml(
            &plugin_dir,
            r#"
[plugin]
name = "loose-config"
version = "1.0.0"
category = "crawler"
author = "Alice"
description = "Loose schema"

[config]
api_token = { default = "" }
"#,
        );
        write_dummy_wasm(&plugin_dir, "loose-config.wasm");

        let (manifest, _) = parse_manifest(&plugin_dir).unwrap();
        let f = manifest.config_schema().get("api_token").unwrap();
        assert_eq!(f.field_type(), ConfigFieldType::String);
    }

    #[test]
    fn test_parse_manifest_unknown_type_returns_err() {
        let tmp = TempDir::new().unwrap();
        let plugin_dir = tmp.path().join("bad-type");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        write_plugin_toml(
            &plugin_dir,
            r#"
[plugin]
name = "bad-type"
version = "1.0.0"
category = "crawler"
author = "Alice"
description = "Bad type"

[config]
foo = { type = "spaceship" }
"#,
        );
        write_dummy_wasm(&plugin_dir, "bad-type.wasm");

        let result = parse_manifest(&plugin_dir);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("spaceship"), "got: {err}");
    }

    #[test]
    fn test_parse_manifest_no_config_yields_empty_schema() {
        let tmp = TempDir::new().unwrap();
        let plugin_dir = tmp.path().join("no-config");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        write_plugin_toml(
            &plugin_dir,
            r#"
[plugin]
name = "no-config"
version = "1.0.0"
category = "utility"
author = "Charlie"
description = "No config block"
"#,
        );
        write_dummy_wasm(&plugin_dir, "no-config.wasm");

        let (manifest, _) = parse_manifest(&plugin_dir).unwrap();
        assert!(manifest.config_schema().is_empty());
    }

    #[test]
    fn test_parse_manifest_rejects_unsupported_regex_feature() {
        let tmp = TempDir::new().unwrap();
        let plugin_dir = tmp.path().join("bad-regex");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        write_plugin_toml(
            &plugin_dir,
            r#"
[plugin]
name = "bad-regex"
version = "1.0.0"
category = "utility"
author = "Alice"
description = "Bad regex"

[config]
mode = { type = "string", regex = "^(foo|bar)$" }
"#,
        );
        write_dummy_wasm(&plugin_dir, "bad-regex.wasm");

        let result = parse_manifest(&plugin_dir);
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("unsupported feature"),
            "expected unsupported-feature error, got: {err}"
        );
    }

    #[test]
    fn test_parse_manifest_extracts_regex_constraint() {
        let tmp = TempDir::new().unwrap();
        let plugin_dir = tmp.path().join("regexed");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        write_plugin_toml(
            &plugin_dir,
            r#"
[plugin]
name = "regexed"
version = "1.0.0"
category = "utility"
author = "Alice"
description = "Regex"

[config]
api_key = { type = "string", regex = "^[a-z0-9]+$" }
"#,
        );
        write_dummy_wasm(&plugin_dir, "regexed.wasm");

        let (manifest, _) = parse_manifest(&plugin_dir).unwrap();
        let f = manifest.config_schema().get("api_key").unwrap();
        assert_eq!(f.regex(), Some("^[a-z0-9]+$"));
    }
}
