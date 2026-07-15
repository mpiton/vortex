use super::super::{PluginYtDlpRequest, YtDlpProvider, request};

#[test]
fn vimeo_audio_download_uses_audio_extraction() {
    let temp = tempfile::tempdir().unwrap();
    let managed = temp.path().join("vortex-downloads");
    std::fs::create_dir_all(&managed).unwrap();
    let prepared = request::prepare(
        YtDlpProvider::Vimeo,
        PluginYtDlpRequest::Download {
            url: "https://vimeo.com/123456789".to_string(),
            quality: None,
            format: Some("m4a".to_string()),
            output_dir: managed.to_string_lossy().into_owned(),
            audio_only: true,
        },
        temp.path(),
    )
    .unwrap();

    assert!(prepared.args.contains(&"--extract-audio".to_string()));
    assert!(!prepared.args.contains(&"--merge-output-format".to_string()));
}
