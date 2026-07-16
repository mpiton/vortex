use super::super::{PluginYtDlpRequest, YtDlpProvider, request};
use super::private_root;

#[test]
fn vimeo_audio_download_uses_audio_extraction() {
    let temp = tempfile::tempdir().unwrap();
    let managed = private_root(temp.path());
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
    let audio_format = prepared
        .args
        .windows(2)
        .find(|pair| pair[0] == "--audio-format")
        .expect("audio format argument");
    assert_eq!(audio_format[1], "m4a");
    assert!(!prepared.args.contains(&"--merge-output-format".to_string()));
}

#[test]
fn video_download_remuxes_premuxed_fallback_to_requested_container() {
    let temp = tempfile::tempdir().unwrap();
    let managed = private_root(temp.path());
    let prepared = request::prepare(
        YtDlpProvider::Youtube,
        PluginYtDlpRequest::Download {
            url: "https://www.youtube.com/watch?v=abcdefghijk".to_string(),
            quality: Some(1080),
            format: Some("mkv".to_string()),
            output_dir: managed.to_string_lossy().into_owned(),
            audio_only: false,
        },
        temp.path(),
    )
    .unwrap();

    assert!(
        prepared
            .args
            .windows(2)
            .any(|pair| pair == ["--remux-video", "mkv"])
    );
}
