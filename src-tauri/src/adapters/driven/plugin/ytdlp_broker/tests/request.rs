use std::path::Path;

use super::super::{PluginYtDlpRequest, YtDlpProvider, request};

#[test]
fn typed_contract_rejects_binary_and_raw_arguments() {
    let with_binary = r#"{
        "action":"metadata",
        "url":"https://www.youtube.com/watch?v=abcdefghijk",
        "playlist":false,
        "binary":"/tmp/yt-dlp"
    }"#;
    let with_args = r#"{
        "action":"metadata",
        "url":"https://www.youtube.com/watch?v=abcdefghijk",
        "playlist":false,
        "args":["--exec","sh"]
    }"#;
    assert!(serde_json::from_str::<PluginYtDlpRequest>(with_binary).is_err());
    assert!(serde_json::from_str::<PluginYtDlpRequest>(with_args).is_err());
}

#[test]
fn metadata_disables_external_configuration_and_code_loading() {
    let prepared = request::prepare(
        YtDlpProvider::Youtube,
        metadata("https://www.youtube.com/watch?v=abcdefghijk"),
        Path::new("/tmp"),
    )
    .expect("valid request");
    assert!(prepared.args.starts_with(&[
        "--ignore-config".to_string(),
        "--no-plugin-dirs".to_string(),
        "--no-remote-components".to_string(),
        "--no-exec".to_string(),
        "--no-cache-dir".to_string(),
    ]));
}

#[test]
fn url_is_validated_and_appended_after_option_sentinel() {
    let url = "https://www.youtube.com/watch?v=abcdefghijk&next=--exec";
    let prepared = request::prepare(YtDlpProvider::Youtube, metadata(url), Path::new("/tmp"))
        .expect("valid request");
    let sentinel = prepared.args.iter().position(|arg| arg == "--").unwrap();
    assert_eq!(
        prepared.args.get(sentinel + 1).map(String::as_str),
        Some(url)
    );
    assert_eq!(sentinel + 2, prepared.args.len());
}

#[test]
fn provider_host_mismatch_is_rejected() {
    let error = request::prepare(
        YtDlpProvider::Youtube,
        metadata("https://attacker.example/video"),
        Path::new("/tmp"),
    )
    .expect_err("host must be rejected");
    assert!(error.to_string().contains("YouTube URL"));
}

#[test]
fn dangerous_format_value_is_rejected() {
    let error = request::prepare(
        YtDlpProvider::Youtube,
        download("/tmp/vortex-downloads", Some("mp4 --exec sh")),
        Path::new("/tmp"),
    )
    .expect_err("format must be rejected");
    assert!(error.to_string().contains("format"));
}

#[test]
fn output_directory_must_be_managed_root() {
    let temp = tempfile::tempdir().unwrap();
    let managed = temp.path().join("vortex-downloads");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&managed).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let error = request::prepare(
        YtDlpProvider::Youtube,
        download(&outside.to_string_lossy(), Some("mp4")),
        temp.path(),
    )
    .expect_err("working directory must be rejected");
    assert!(
        error
            .to_string()
            .contains("must be the Vortex download root")
    );
}

#[test]
fn each_download_gets_a_private_output_directory() {
    let temp = tempfile::tempdir().unwrap();
    let managed = temp.path().join("vortex-downloads");
    std::fs::create_dir_all(&managed).unwrap();
    let output = managed.to_string_lossy();
    let first = request::prepare(
        YtDlpProvider::Youtube,
        download(&output, Some("mp4")),
        temp.path(),
    )
    .unwrap();
    let second = request::prepare(
        YtDlpProvider::Youtube,
        download(&output, Some("mp4")),
        temp.path(),
    )
    .unwrap();
    assert_ne!(first.working_dir, second.working_dir);
    assert_eq!(first.working_dir.parent(), Some(managed.as_path()));
}

fn metadata(url: &str) -> PluginYtDlpRequest {
    PluginYtDlpRequest::Metadata {
        url: url.to_string(),
        playlist: false,
    }
}

fn download(output_dir: &str, format: Option<&str>) -> PluginYtDlpRequest {
    PluginYtDlpRequest::Download {
        url: "https://www.youtube.com/watch?v=abcdefghijk".to_string(),
        quality: Some(1080),
        format: format.map(str::to_string),
        output_dir: output_dir.to_string(),
        audio_only: false,
    }
}
