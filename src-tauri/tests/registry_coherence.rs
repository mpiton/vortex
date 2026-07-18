//! MAT-136 R-05: the plugin registry must stay coherent — parseable TOML,
//! complete entries, well-formed versions and checksums, unique names.
//! This test IS the automated validation the ticket asks for; it runs in CI
//! with `cargo test --workspace`.

use std::collections::HashSet;
use std::path::Path;

const CATEGORIES: &[&str] = &[
    "crawler",
    "hoster",
    "debrid",
    "container",
    "captcha",
    "extractor",
    "notifier",
    "utility",
];

fn load_registry() -> toml::Table {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../registry/registry.toml");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    raw.parse::<toml::Table>()
        .unwrap_or_else(|e| panic!("registry.toml is not valid TOML: {e}"))
}

fn plugins(registry: &toml::Table) -> &[toml::Value] {
    registry
        .get("plugin")
        .and_then(|p| p.as_array())
        .map(Vec::as_slice)
        .expect("registry.toml must contain [[plugin]] entries")
}

fn str_field<'a>(plugin: &'a toml::Value, name: &str, field: &str) -> &'a str {
    let value = plugin
        .get(field)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("{name}: missing string field `{field}`"));
    assert!(!value.trim().is_empty(), "{name}: field `{field}` is empty");
    value
}

fn is_semver_triple(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars().all(|c| c.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
        })
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

#[test]
fn test_registry_entries_are_complete_and_well_formed() {
    let registry = load_registry();
    let plugins = plugins(&registry);
    assert!(!plugins.is_empty(), "registry has no plugins");

    for plugin in plugins {
        let name = plugin
            .get("name")
            .and_then(|v| v.as_str())
            .expect("every [[plugin]] entry needs a `name`")
            .to_string();

        for field in ["description", "author", "repository"] {
            str_field(plugin, &name, field);
        }

        let version = str_field(plugin, &name, "version");
        assert!(
            is_semver_triple(version),
            "{name}: version `{version}` is not MAJOR.MINOR.PATCH"
        );
        // `min_vortex_version` is optional (see registry/TEMPLATE.toml and
        // `Option<String>` in the store client); validate only when present.
        if let Some(value) = plugin.get("min_vortex_version") {
            let min_vortex = value
                .as_str()
                .unwrap_or_else(|| panic!("{name}: `min_vortex_version` must be a string"));
            assert!(
                is_semver_triple(min_vortex),
                "{name}: min_vortex_version `{min_vortex}` is not MAJOR.MINOR.PATCH"
            );
        }

        let category = str_field(plugin, &name, "category");
        assert!(
            CATEGORIES.contains(&category),
            "{name}: unknown category `{category}` (expected one of {CATEGORIES:?})"
        );

        for field in ["checksum_sha256", "checksum_sha256_toml"] {
            let checksum = str_field(plugin, &name, field);
            assert!(
                is_sha256_hex(checksum),
                "{name}: `{field}` is not 64 lowercase hex chars: `{checksum}`"
            );
        }
    }
}

#[test]
fn test_is_semver_triple_with_leading_zero_rejects() {
    assert!(!is_semver_triple("01.2.3"));
    assert!(!is_semver_triple("1.02.3"));
    assert!(is_semver_triple("0.2.10"));
}

#[test]
fn test_registry_plugin_names_are_unique() {
    let registry = load_registry();
    let mut seen = HashSet::new();
    for plugin in plugins(&registry) {
        let name = plugin
            .get("name")
            .and_then(|v| v.as_str())
            .expect("every [[plugin]] entry needs a `name`");
        assert!(
            seen.insert(name.to_string()),
            "duplicate plugin name: {name}"
        );
    }
}
