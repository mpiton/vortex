use std::path::{Component, Path, PathBuf};

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
    if provider == YtDlpProvider::Generic
        && (parsed.scheme() != "https"
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.port().is_some_and(|port| port != 443))
    {
        bail!("run_ytdlp: metadata fallback requires a canonical HTTPS URL");
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
        YtDlpProvider::Generic => {
            host_is_approved(YtDlpProvider::Youtube, host)
                || host_is_approved(YtDlpProvider::Vimeo, host)
                || host_is_approved(YtDlpProvider::Soundcloud, host)
        }
    }
}

fn provider_label(provider: YtDlpProvider) -> &'static str {
    match provider {
        YtDlpProvider::Youtube => "YouTube URL",
        YtDlpProvider::Vimeo => "Vimeo URL",
        YtDlpProvider::Soundcloud => "SoundCloud URL",
        YtDlpProvider::Generic => "supported media platform URL",
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
    validate_private_root(&allowed_root)?;
    let requested_metadata = std::fs::symlink_metadata(output_dir)
        .with_context(|| format!("run_ytdlp: output directory '{output_dir}' is unavailable"))?;
    if requested_metadata.file_type().is_symlink() {
        bail!("run_ytdlp: output directory must not be a symlink");
    }
    let canonical_root = std::fs::canonicalize(&allowed_root).with_context(|| {
        format!(
            "run_ytdlp: output root '{}' is unavailable",
            allowed_root.display()
        )
    })?;
    let requested = std::fs::canonicalize(output_dir)
        .with_context(|| format!("run_ytdlp: output directory '{output_dir}' is unavailable"))?;
    if requested.parent() != Some(canonical_root.as_path()) || !has_prefix(&requested, "request-") {
        bail!("run_ytdlp: output directory must be a Vortex-managed request root");
    }
    validate_private_root(&requested)?;
    create_private_child(&requested, "job-")
}

pub(super) fn ensure_private_root(root: &Path) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(root) {
        Ok(_) => validate_private_root(root),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(root).with_context(|| {
                format!(
                    "run_ytdlp: failed to create private root '{}'",
                    root.display()
                )
            })?;
            validate_private_root(root)
        }
        Err(error) => Err(error)
            .with_context(|| format!("run_ytdlp: failed to inspect root '{}'", root.display())),
    }
}

#[cfg(unix)]
pub(super) fn ensure_owned_cache_directory(path: &Path) -> anyhow::Result<PathBuf> {
    ensure_owned_directory_until(path, Path::new("/"))
}

#[cfg(not(unix))]
pub(super) fn ensure_owned_cache_directory(path: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(path).with_context(|| {
        format!(
            "run_ytdlp: failed to create user cache directory '{}'",
            path.display()
        )
    })?;
    let canonical = std::fs::canonicalize(path)?;
    if !canonical.is_dir() {
        bail!("run_ytdlp: user cache path must be a directory");
    }
    Ok(canonical)
}

#[cfg(unix)]
pub(super) fn ensure_owned_directory_until(path: &Path, anchor: &Path) -> anyhow::Result<PathBuf> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("run_ytdlp: user cache path must be absolute and normalized");
    }

    let canonical_anchor = std::fs::canonicalize(anchor).with_context(|| {
        format!(
            "run_ytdlp: cache trust anchor '{}' is unavailable",
            anchor.display()
        )
    })?;
    let mut existing = path.to_path_buf();
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(&existing) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = existing.file_name().ok_or_else(|| {
                    anyhow::anyhow!("run_ytdlp: user cache path has no existing ancestor")
                })?;
                missing.push(name.to_os_string());
                existing = existing
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("run_ytdlp: invalid user cache path"))?
                    .to_path_buf();
            }
            Err(error) => return Err(error).context("run_ytdlp: failed to inspect user cache"),
        }
    }

    let mut current = std::fs::canonicalize(&existing)?;
    if !current.starts_with(&canonical_anchor) {
        bail!("run_ytdlp: user cache escapes its trust anchor");
    }
    validate_directory_chain(&current, &canonical_anchor)?;

    for component in missing.into_iter().rev() {
        current.push(component);
        let mut builder = std::fs::DirBuilder::new();
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
        builder.create(&current).with_context(|| {
            format!(
                "run_ytdlp: failed to create user cache directory '{}'",
                current.display()
            )
        })?;
        validate_private_root(&current)?;
    }
    Ok(current)
}

#[cfg(unix)]
fn validate_directory_chain(path: &Path, anchor: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let mut current = path;
    let mut first = true;
    loop {
        let metadata = std::fs::metadata(current)?;
        let owner_is_trusted = if first {
            metadata.uid() == unsafe { libc::geteuid() }
        } else {
            matches!(metadata.uid(), 0) || metadata.uid() == unsafe { libc::geteuid() }
        };
        if !metadata.is_dir() || !owner_is_trusted || metadata.permissions().mode() & 0o022 != 0 {
            bail!(
                "run_ytdlp: cache ancestor '{}' is not safely owned (uid {}, mode {:o})",
                current.display(),
                metadata.uid(),
                metadata.permissions().mode() & 0o777,
            );
        }
        if current == anchor {
            return Ok(());
        }
        current = current.parent().ok_or_else(|| {
            anyhow::anyhow!("run_ytdlp: cache path does not reach its trust anchor")
        })?;
        first = false;
    }
}

fn validate_private_root(root: &Path) -> anyhow::Result<()> {
    let metadata = std::fs::symlink_metadata(root)
        .with_context(|| format!("run_ytdlp: output root '{}' is unavailable", root.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("run_ytdlp: output root must be a private directory, not a symlink");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        if metadata.uid() != unsafe { libc::geteuid() } {
            bail!("run_ytdlp: output root is not owned by the current user");
        }
        if metadata.permissions().mode() & 0o077 != 0 {
            bail!("run_ytdlp: output root permissions must be 0700");
        }
    }
    Ok(())
}

pub(super) fn create_private_child(root: &Path, prefix: &str) -> anyhow::Result<PathBuf> {
    let path = root.join(format!("{prefix}{}", uuid::Uuid::new_v4().simple()));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&path).with_context(|| {
        format!(
            "run_ytdlp: failed to create private output directory '{}'",
            path.display()
        )
    })?;
    Ok(path)
}

pub(super) fn cleanup_private_job_dir(path: &Path) -> anyhow::Result<()> {
    if !has_prefix(path, "job-")
        || !path
            .parent()
            .is_some_and(|parent| has_prefix(parent, "request-"))
    {
        bail!("run_ytdlp: refusing to clean an unmanaged job directory");
    }
    cleanup_private_tree(path)
}

pub(super) fn cleanup_private_request_dir(path: &Path) -> anyhow::Result<()> {
    if !has_prefix(path, "request-")
        || path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            != Some("vortex-downloads")
    {
        bail!("run_ytdlp: refusing to clean an unmanaged request directory");
    }
    cleanup_private_tree(path)
}

fn cleanup_private_tree(path: &Path) -> anyhow::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("run_ytdlp: failed to inspect cleanup path"),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("run_ytdlp: refusing to clean a non-directory or symlink");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } {
            bail!("run_ytdlp: refusing to clean a directory owned by another user");
        }
    }
    std::fs::remove_dir_all(path).with_context(|| {
        format!(
            "run_ytdlp: failed to clean private directory '{}'",
            path.display()
        )
    })
}

fn has_prefix(path: &Path, prefix: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(prefix))
}
