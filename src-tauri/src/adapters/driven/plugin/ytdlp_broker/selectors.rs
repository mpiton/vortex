use std::path::Path;

use anyhow::bail;

use super::YtDlpProvider;
use super::request::{append_url, secure_prefix, strings};

pub(super) fn build_direct_selector(
    quality: Option<u32>,
    format: Option<&str>,
    audio_only: bool,
) -> String {
    if audio_only {
        return format
            .map(|ext| {
                format!(
                    "bestaudio[ext={ext}][protocol=https]/bestaudio[protocol=https]/bestaudio[ext={ext}]/bestaudio"
                )
            })
            .unwrap_or_else(|| "bestaudio[protocol=https]/bestaudio".to_string());
    }
    match (quality, format) {
        (Some(height), Some(ext)) => format!(
            "best[height<={height}][ext={ext}][protocol=https]/best[height<={height}][protocol=https]"
        ),
        (Some(height), None) => format!("best[height<={height}][protocol=https]"),
        (None, Some(ext)) => format!("best[ext={ext}][protocol=https]/best[protocol=https]"),
        (None, None) => "best[protocol=https]".to_string(),
    }
}

fn build_download_selector(quality: Option<u32>, format: Option<&str>, audio: bool) -> String {
    if audio {
        return format
            .map(|ext| format!("bestaudio[ext={ext}]/bestaudio"))
            .unwrap_or_else(|| "bestaudio".to_string());
    }
    quality
        .map(|height| {
            format!(
                "bestvideo[height<={height}]+bestaudio/bestvideo[height<={height}]+bestaudio[ext=m4a]/best[height<={height}]"
            )
        })
        .unwrap_or_else(|| "bestvideo+bestaudio/best".to_string())
}

pub(super) fn build_download_args(
    provider: YtDlpProvider,
    url: &str,
    quality: Option<u32>,
    format: Option<&str>,
    output_dir: &Path,
    audio_only: bool,
) -> anyhow::Result<Vec<String>> {
    if provider == YtDlpProvider::Generic {
        bail!("run_ytdlp: generic provider cannot download files");
    }
    let audio = audio_only || provider == YtDlpProvider::Soundcloud;
    let mut args = secure_prefix();
    args.extend([
        "--format".to_string(),
        build_download_selector(quality, format, audio),
    ]);
    if audio {
        args.extend(strings(["--extract-audio", "--audio-format"]));
        args.push(normalize_audio_format(format).to_string());
    } else {
        args.extend(strings([
            "--merge-output-format",
            merge_format(provider, format),
        ]));
    }
    append_output_args(&mut args, output_dir);
    append_provider_args(&mut args, provider);
    append_url(&mut args, url.to_string());
    Ok(args)
}

fn merge_format(provider: YtDlpProvider, format: Option<&str>) -> &str {
    if provider == YtDlpProvider::Vimeo {
        "mp4"
    } else {
        format
            .filter(|value| matches!(*value, "mkv" | "mov" | "mp4" | "webm"))
            .unwrap_or("mp4")
    }
}

fn append_output_args(args: &mut Vec<String>, output_dir: &Path) {
    let escaped = output_dir.to_string_lossy().replace('%', "%%");
    args.extend(["--output".to_string(), format!("{escaped}/%(id)s.%(ext)s")]);
    args.extend(strings([
        "--print",
        "after_move:%(filepath)s",
        "--no-playlist",
        "--no-warnings",
        "--quiet",
    ]));
}

fn append_provider_args(args: &mut Vec<String>, provider: YtDlpProvider) {
    if provider == YtDlpProvider::Youtube {
        args.extend(strings([
            "--extractor-args",
            "youtube:player_client=default,web_safari,android_vr,tv",
        ]));
    }
    if matches!(provider, YtDlpProvider::Youtube | YtDlpProvider::Vimeo) {
        args.extend(strings(["--retries", "3", "--fragment-retries", "3"]));
    }
}

fn normalize_audio_format(format: Option<&str>) -> &str {
    match format {
        Some("aac") => "m4a",
        Some("ogg") => "vorbis",
        Some(value @ ("best" | "flac" | "m4a" | "mp3" | "opus" | "vorbis" | "wav")) => value,
        _ => "mp3",
    }
}
