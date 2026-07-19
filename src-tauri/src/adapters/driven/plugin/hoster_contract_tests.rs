use std::sync::Arc;

use super::ExtismPluginLoader;
use super::capabilities::SharedHostResources;
use super::hoster_contract::{parse_hoster_link, parse_hoster_links};
use crate::domain::error::DomainError;
use crate::domain::model::captcha::CaptchaType;
use crate::domain::model::credential::Credential;
use crate::domain::model::plugin::{PluginCategory, PluginInfo, PluginManifest};
use crate::domain::ports::driven::PluginLoader;

#[test]
fn test_parse_hoster_link_maps_wire_fields() {
    let parsed = parse_hoster_link(
        r#"{"files":[{"url":"https://1fichier.com/?a","filename":"a.zip","size_bytes":42,"direct_url":"https://cdn.example/a","resumable":true,"headers":{"Referer":"https://1fichier.com/"},"traffic_used_bytes":1,"traffic_total_bytes":100}]}"#,
    )
    .expect("valid hoster response");

    assert_eq!(parsed.source_url, "https://1fichier.com/?a");
    assert_eq!(parsed.filename.as_deref(), Some("a.zip"));
    assert_eq!(parsed.direct_url.as_deref(), Some("https://cdn.example/a"));
    assert_eq!(parsed.resumable, Some(true));
    assert_eq!(
        parsed.request_headers,
        vec![("Referer".to_string(), "https://1fichier.com/".to_string())]
    );
    assert_eq!(parsed.traffic_total_bytes, Some(100));
}

#[test]
fn test_parse_hoster_link_rejects_empty_file_list() {
    assert_eq!(
        parse_hoster_link(r#"{"files":[]}"#),
        Err(DomainError::HosterNoFile)
    );
}

#[test]
fn test_parse_hoster_link_accepts_bounded_captcha_without_direct_url() {
    let parsed = parse_hoster_link(
        r#"{"files":[{"url":"https://hoster.example/file","direct_url":null,"requires_captcha":true,"captcha_type":"image","captcha_image_data":[137,80,78,71]}]}"#,
    )
    .expect("captcha response is a valid unresolved hoster link");

    assert!(parsed.direct_url.is_none());
    let captcha = parsed.captcha.expect("captcha metadata");
    assert_eq!(captcha.challenge_type, CaptchaType::Image);
    assert_eq!(
        captcha.image_data.as_deref(),
        Some([137, 80, 78, 71].as_slice())
    );
}

#[test]
fn test_parse_hoster_links_preserves_every_gofile_entry() {
    let parsed = parse_hoster_links(
        r#"{"files":[{"url":"https://gofile.io/d/folder/file-a","filename":"a.zip","size_bytes":10,"direct_url":"https://store.example/a","resumable":true},{"url":"https://gofile.io/d/folder/file-b","filename":"b.zip","size_bytes":20,"direct_url":"https://store.example/b","resumable":false}]}"#,
    )
    .expect("valid multi-file hoster response");

    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].filename.as_deref(), Some("a.zip"));
    assert_eq!(parsed[1].source_url, "https://gofile.io/d/folder/file-b");
    assert_eq!(parsed[1].resumable, Some(false));
}

#[test]
fn test_parse_hoster_links_rejects_blank_source_or_direct_urls() {
    for payload in [
        r#"{"files":[{"url":"   ","direct_url":"https://cdn.example/file"}]}"#,
        r#"{"files":[{"url":"https://hoster.example/file","direct_url":"  "}]}"#,
    ] {
        assert_eq!(parse_hoster_links(payload), Err(DomainError::HosterNoFile));
    }
}

#[test]
fn test_parse_hoster_links_caps_plugin_fan_out() {
    let files = (0..501)
        .map(|index| {
            serde_json::json!({
                "url": format!("https://gofile.io/d/folder/file-{index}"),
                "direct_url": format!("https://store.example/file-{index}")
            })
        })
        .collect::<Vec<_>>();
    let payload = serde_json::json!({ "files": files }).to_string();

    assert!(matches!(
        parse_hoster_links(&payload),
        Err(DomainError::PluginError(_))
    ));
}

#[test]
fn test_extract_hoster_link_calls_exact_named_plugin_without_reresolving_url() {
    let temp = tempfile::tempdir().unwrap();
    let name = "hoster-a";
    let plugin_dir = temp.path().join(name);
    std::fs::create_dir(&plugin_dir).unwrap();
    std::fs::write(
        plugin_dir.join("plugin.toml"),
        format!(
            "[plugin]\nname = \"{name}\"\nversion = \"1.0.0\"\ncategory = \"hoster\"\nauthor = \"tester\"\ndescription = \"test hoster\"\n"
        ),
    )
    .unwrap();
    let payload = r#"{"files":[{"url":"https://source.example/file","filename":"from-a.zip","size_bytes":1,"direct_url":"https://cdn.example/file","traffic_used_bytes":0,"traffic_total_bytes":10}]}"#;
    std::fs::write(
        plugin_dir.join(format!("{name}.wasm")),
        output_plugin_wat("false", payload),
    )
    .unwrap();

    let loader = ExtismPluginLoader::new(
        temp.path().to_path_buf(),
        Arc::new(SharedHostResources::new()),
    )
    .unwrap();
    let manifest = PluginManifest::new(PluginInfo::new(
        name.into(),
        "1.0.0".into(),
        "test hoster".into(),
        "tester".into(),
        PluginCategory::Hoster,
    ));
    loader.load(&manifest).unwrap();

    let link = loader
        .extract_hoster_link(
            name,
            "https://url-not-claimed-by-a.example/file",
            Some(&Credential::new("alice", "secret")),
        )
        .expect("the named plugin must be called directly");

    assert_eq!(link.filename.as_deref(), Some("from-a.zip"));
}

fn output_plugin_wat(can_handle: &str, payload: &str) -> String {
    format!(
        r#"(module
  (import "extism:host/env" "alloc" (func $alloc (param i64) (result i64)))
  (import "extism:host/env" "store_u8" (func $store_u8 (param i64 i32)))
  (import "extism:host/env" "output_set" (func $output_set (param i64 i64)))
  {}
  {}
)"#,
        output_function("can_handle", can_handle),
        output_function("extract_links", payload),
    )
}

fn output_function(name: &str, value: &str) -> String {
    let stores = value
        .bytes()
        .enumerate()
        .map(|(index, byte)| {
            format!(
                "(call $store_u8 (i64.add (local.get $output) (i64.const {index})) (i32.const {byte}))"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"(func (export "{name}") (result i32)
  (local $output i64)
  (local.set $output (call $alloc (i64.const {})))
  {stores}
  (call $output_set (local.get $output) (i64.const {}))
  (i32.const 0)
)"#,
        value.len(),
        value.len(),
    )
}
