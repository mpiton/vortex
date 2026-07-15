use std::path::Path;

use anyhow::bail;

use super::selectors::{build_direct_selector, build_download_args};
use super::validation::{private_output_dir, validate_format, validate_quality, validate_url};
use super::{
    DEFAULT_TIMEOUT, DOWNLOAD_TIMEOUT, PluginYtDlpRequest, PreparedCommand, YtDlpProvider,
};

pub(super) fn prepare(
    provider: YtDlpProvider,
    request: PluginYtDlpRequest,
    temp_root: &Path,
) -> anyhow::Result<PreparedCommand> {
    match request {
        PluginYtDlpRequest::Metadata { url, playlist } => {
            prepare_metadata(provider, url, playlist, temp_root)
        }
        PluginYtDlpRequest::Resolve {
            url,
            quality,
            format,
            audio_only,
        } => prepare_resolve(provider, url, quality, format, audio_only, temp_root),
        PluginYtDlpRequest::Download {
            url,
            quality,
            format,
            output_dir,
            audio_only,
        } => prepare_download(
            provider, url, quality, format, output_dir, audio_only, temp_root,
        ),
    }
}

fn prepare_metadata(
    provider: YtDlpProvider,
    url: String,
    playlist: bool,
    temp_root: &Path,
) -> anyhow::Result<PreparedCommand> {
    if !matches!(provider, YtDlpProvider::Youtube | YtDlpProvider::Generic) {
        bail!("run_ytdlp: metadata action is not available for this provider");
    }
    let mut args = secure_prefix();
    args.extend(metadata_args(provider, playlist));
    append_url(&mut args, validate_url(provider, &url)?);
    Ok(prepared(args, temp_root, DEFAULT_TIMEOUT))
}

fn metadata_args(provider: YtDlpProvider, playlist: bool) -> Vec<String> {
    if provider == YtDlpProvider::Generic {
        strings(["--dump-single-json", "--flat-playlist", "--no-warnings"])
    } else if playlist {
        strings(["--dump-json", "--flat-playlist", "--no-warnings"])
    } else {
        strings(["--dump-json", "--no-playlist", "--no-warnings"])
    }
}

fn prepare_resolve(
    provider: YtDlpProvider,
    url: String,
    quality: Option<u32>,
    format: Option<String>,
    audio_only: bool,
    temp_root: &Path,
) -> anyhow::Result<PreparedCommand> {
    if provider != YtDlpProvider::Youtube {
        bail!("run_ytdlp: resolve action is only available for YouTube");
    }
    let selector = build_direct_selector(
        validate_quality(quality)?,
        validate_format(format)?.as_deref(),
        audio_only,
    );
    let mut args = secure_prefix();
    args.extend(strings(["--get-url", "--no-playlist", "--no-warnings"]));
    args.extend(["--format".to_string(), selector]);
    append_url(&mut args, validate_url(provider, &url)?);
    Ok(prepared(args, temp_root, DEFAULT_TIMEOUT))
}

fn prepare_download(
    provider: YtDlpProvider,
    url: String,
    quality: Option<u32>,
    format: Option<String>,
    output_dir: String,
    audio_only: bool,
    temp_root: &Path,
) -> anyhow::Result<PreparedCommand> {
    let format = validate_format(format)?;
    let output_dir = private_output_dir(&output_dir, temp_root)?;
    let args = build_download_args(
        provider,
        &validate_url(provider, &url)?,
        validate_quality(quality)?,
        format.as_deref(),
        &output_dir,
        audio_only,
    )?;
    Ok(PreparedCommand {
        args,
        working_dir: output_dir,
        timeout: DOWNLOAD_TIMEOUT,
    })
}

fn prepared(args: Vec<String>, temp_root: &Path, timeout: std::time::Duration) -> PreparedCommand {
    PreparedCommand {
        args,
        working_dir: temp_root.join("vortex-ytdlp"),
        timeout,
    }
}

pub(super) fn secure_prefix() -> Vec<String> {
    strings([
        "--ignore-config",
        "--no-plugin-dirs",
        "--no-remote-components",
        "--no-exec",
        "--no-cache-dir",
    ])
}

pub(super) fn strings<const N: usize>(values: [&str; N]) -> Vec<String> {
    values.into_iter().map(str::to_string).collect()
}

pub(super) fn append_url(args: &mut Vec<String>, url: String) {
    args.extend(["--".to_string(), url]);
}
