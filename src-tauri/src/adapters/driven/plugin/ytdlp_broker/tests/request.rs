use std::path::Path;

use super::super::{DEFAULT_OUTPUT_LIMIT, METADATA_OUTPUT_LIMIT};
use super::super::{PluginYtDlpRequest, YtDlpProvider, request, validation};
use super::private_root;

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
    assert_eq!(prepared.stdout_limit, METADATA_OUTPUT_LIMIT);
    assert_eq!(prepared.stderr_limit, DEFAULT_OUTPUT_LIMIT);
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
fn generic_metadata_rejects_loopback_and_untrusted_hosts() {
    for url in [
        "http://127.0.0.1/admin",
        "http://[::1]/admin",
        "https://attacker.example/video",
        "http://youtube.com/watch?v=abcdefghijk",
        "https://user:pass@youtube.com/watch?v=abcdefghijk",
        "https://youtube.com:8443/watch?v=abcdefghijk",
    ] {
        request::prepare(YtDlpProvider::Generic, metadata(url), Path::new("/tmp"))
            .expect_err("generic metadata must not reach an attacker-controlled host");
    }
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
    private_root(temp.path());
    let outside = temp.path().join("outside");
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
            .contains("must be a Vortex-managed request root")
    );
}

#[test]
fn each_download_gets_a_private_output_directory() {
    let temp = tempfile::tempdir().unwrap();
    let managed = private_root(temp.path());
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

#[test]
fn invalid_download_does_not_allocate_a_private_output_directory() {
    let temp = tempfile::tempdir().unwrap();
    let managed = private_root(temp.path());
    let request = PluginYtDlpRequest::Download {
        url: "https://attacker.example/video".to_string(),
        quality: Some(1080),
        format: Some("mp4".to_string()),
        output_dir: managed.to_string_lossy().into_owned(),
        audio_only: false,
    };

    request::prepare(YtDlpProvider::Youtube, request, temp.path())
        .expect_err("invalid URL must fail");
    assert_eq!(std::fs::read_dir(&managed).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn group_readable_output_root_is_rejected() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let managed = temp.path().join("vortex-downloads");
    std::fs::create_dir(&managed).unwrap();
    std::fs::set_permissions(&managed, std::fs::Permissions::from_mode(0o750)).unwrap();

    request::prepare(
        YtDlpProvider::Youtube,
        download(&managed.to_string_lossy(), Some("mp4")),
        temp.path(),
    )
    .expect_err("non-private root must fail");
}

#[cfg(unix)]
#[test]
fn host_created_output_root_is_private() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("private-root");

    validation::ensure_private_root(&root).unwrap();

    let metadata = std::fs::symlink_metadata(root).unwrap();
    assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
    assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
}

#[cfg(unix)]
#[test]
fn group_writable_private_root_parent_is_rejected() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o770)).unwrap();
    let cache = temp.path().join("cache");

    validation::ensure_owned_directory_until(&cache, temp.path())
        .expect_err("group-writable parent must not protect broker output");
}

#[cfg(unix)]
#[test]
fn missing_cache_directory_is_created_privately() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let cache = temp.path().join("new").join("cache");

    let created = validation::ensure_owned_directory_until(&cache, temp.path()).unwrap();

    assert_eq!(created, cache);
    assert_eq!(
        std::fs::metadata(&created).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

#[test]
fn request_cleanup_removes_partial_job_trees() {
    let temp = tempfile::tempdir().unwrap();
    let request = private_root(temp.path());
    let job = validation::create_private_child(&request, "job-").unwrap();
    std::fs::write(job.join("partial.part"), b"partial").unwrap();

    validation::cleanup_private_request_dir(&request).unwrap();

    assert!(!request.exists());
}

#[cfg(unix)]
#[test]
fn symlinked_output_root_is_rejected() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let managed = temp.path().join("vortex-downloads");
    symlink(outside.path(), &managed).unwrap();

    request::prepare(
        YtDlpProvider::Youtube,
        download(&managed.to_string_lossy(), Some("mp4")),
        temp.path(),
    )
    .expect_err("symlinked root must fail");
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
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
