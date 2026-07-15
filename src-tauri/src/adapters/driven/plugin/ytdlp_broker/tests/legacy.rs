use std::path::Path;

use super::super::{LegacySubprocessRequest, legacy};

#[test]
fn rejects_binary_replacement() {
    let request = LegacySubprocessRequest {
        binary: "/tmp/yt-dlp".to_string(),
        args: vec!["--version".to_string()],
        timeout_ms: Some(1_000),
    };
    let error = legacy::prepare("vortex-mod-youtube", request, Path::new("/tmp"))
        .expect_err("binary replacement must fail");
    assert!(error.to_string().contains("binary"));
}

#[test]
fn rejects_exec_and_config_arguments() {
    for dangerous in ["--exec", "--config-locations", "--plugin-dirs"] {
        let request = LegacySubprocessRequest {
            binary: "yt-dlp".to_string(),
            args: vec![dangerous.to_string(), "payload".to_string()],
            timeout_ms: Some(1_000),
        };
        let error = legacy::prepare("vortex-mod-youtube", request, Path::new("/tmp"))
            .expect_err("dangerous argument must fail");
        assert!(error.to_string().contains("arguments"));
    }
}

#[test]
fn rebuilds_known_youtube_metadata_profile() {
    let url = "https://www.youtube.com/watch?v=abcdefghijk";
    let request = LegacySubprocessRequest {
        binary: "yt-dlp".to_string(),
        args: ["--dump-json", "--no-playlist", "--no-warnings", "--", url]
            .map(str::to_string)
            .to_vec(),
        timeout_ms: Some(60_000),
    };
    let prepared = legacy::prepare("vortex-mod-youtube", request, Path::new("/tmp"))
        .expect("published profile remains compatible");
    assert_eq!(prepared.args.last().map(String::as_str), Some(url));
    assert!(prepared.args.contains(&"--ignore-config".to_string()));
}
