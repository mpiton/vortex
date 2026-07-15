use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

use super::YtDlpProvider;

const MAX_URL_BYTES: usize = 8 * 1024;

pub(super) fn validate_url(provider: YtDlpProvider, url: &str) -> anyhow::Result<String> {
    let url = url.trim();
    if url.is_empty() || url.len() > MAX_URL_BYTES {
        bail!("run_ytdlp: URL length is invalid");
    }
    let parsed = reqwest::Url::parse(url).context("run_ytdlp: invalid URL")?;
    if !matches!(parsed.scheme(), "http" | "https") {
        bail!("run_ytdlp: URL must use HTTP or HTTPS");
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("run_ytdlp: URL has no host"))?
        .to_ascii_lowercase();
    if !host_is_approved(provider, &host) {
        bail!("run_ytdlp: expected a {}", provider_label(provider));
    }
    Ok(url.to_string())
}

fn host_is_approved(provider: YtDlpProvider, host: &str) -> bool {
    match provider {
        YtDlpProvider::Youtube => matches!(
            host,
            "youtube.com"
                | "www.youtube.com"
                | "m.youtube.com"
                | "music.youtube.com"
                | "youtube-nocookie.com"
                | "www.youtube-nocookie.com"
                | "youtu.be"
        ),
        YtDlpProvider::Vimeo => matches!(
            host,
            "vimeo.com" | "www.vimeo.com" | "player.vimeo.com" | "m.vimeo.com"
        ),
        YtDlpProvider::Soundcloud => matches!(
            host,
            "soundcloud.com" | "www.soundcloud.com" | "m.soundcloud.com" | "on.soundcloud.com"
        ),
        YtDlpProvider::Generic => true,
    }
}

fn provider_label(provider: YtDlpProvider) -> &'static str {
    match provider {
        YtDlpProvider::Youtube => "YouTube URL",
        YtDlpProvider::Vimeo => "Vimeo URL",
        YtDlpProvider::Soundcloud => "SoundCloud URL",
        YtDlpProvider::Generic => "HTTP(S) URL",
    }
}

pub(super) fn validate_quality(quality: Option<u32>) -> anyhow::Result<Option<u32>> {
    if quality.is_some_and(|value| value == 0 || value > 4320) {
        bail!("run_ytdlp: quality must be between 1 and 4320");
    }
    Ok(quality)
}

pub(super) fn validate_format(format: Option<String>) -> anyhow::Result<Option<String>> {
    let Some(format) = format else {
        return Ok(None);
    };
    let format = format.trim().to_ascii_lowercase();
    if format.is_empty() {
        return Ok(None);
    }
    if !is_approved_format(&format) {
        bail!("run_ytdlp: unsupported format '{format}'");
    }
    Ok(Some(format))
}

fn is_approved_format(format: &str) -> bool {
    matches!(
        format,
        "aac"
            | "best"
            | "flac"
            | "m4a"
            | "mkv"
            | "mov"
            | "mp3"
            | "mp4"
            | "ogg"
            | "opus"
            | "vorbis"
            | "wav"
            | "webm"
    )
}

pub(super) fn private_output_dir(output_dir: &str, temp_root: &Path) -> anyhow::Result<PathBuf> {
    let allowed_root = temp_root.join("vortex-downloads");
    let canonical_root = std::fs::canonicalize(&allowed_root).with_context(|| {
        format!(
            "run_ytdlp: output root '{}' is unavailable",
            allowed_root.display()
        )
    })?;
    let requested = std::fs::canonicalize(output_dir)
        .with_context(|| format!("run_ytdlp: output directory '{output_dir}' is unavailable"))?;
    if requested != canonical_root {
        bail!("run_ytdlp: output directory must be the Vortex download root");
    }
    create_private_dir(&canonical_root)
}

fn create_private_dir(root: &Path) -> anyhow::Result<PathBuf> {
    let path = root.join(format!("job-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir(&path).with_context(|| {
        format!(
            "run_ytdlp: failed to create private output directory '{}'",
            path.display()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(path)
}
