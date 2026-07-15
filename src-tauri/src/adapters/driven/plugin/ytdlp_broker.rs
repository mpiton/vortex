//! Constrained yt-dlp broker for official media plugins.

mod legacy;
mod legacy_download;
mod output;
mod platform;
mod process;
mod request;
mod selectors;
mod validation;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::bail;
use serde::{Deserialize, Serialize};

pub(super) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
pub(super) const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
pub(super) const DEFAULT_OUTPUT_LIMIT: usize = 1024 * 1024;
pub(super) const METADATA_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum YtDlpProvider {
    Youtube,
    Vimeo,
    Soundcloud,
    Generic,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PluginYtDlpRequest {
    Metadata {
        url: String,
        #[serde(default)]
        playlist: bool,
    },
    Resolve {
        url: String,
        quality: Option<u32>,
        format: Option<String>,
        #[serde(default)]
        audio_only: bool,
    },
    Download {
        url: String,
        quality: Option<u32>,
        format: Option<String>,
        output_dir: String,
        #[serde(default)]
        audio_only: bool,
    },
}

#[derive(Debug, Deserialize)]
pub(crate) struct LegacySubprocessRequest {
    pub(crate) binary: String,
    #[serde(default)]
    pub(crate) args: Vec<String>,
    pub(crate) timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct YtDlpResponse {
    pub(crate) exit_code: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

#[derive(Debug)]
pub(super) struct PreparedCommand {
    pub(super) args: Vec<String>,
    pub(super) working_dir: PathBuf,
    pub(super) timeout: Duration,
    pub(super) stdout_limit: usize,
    pub(super) stderr_limit: usize,
    pub(super) cleanup_working_dir_on_failure: bool,
}

pub(crate) struct ManagedOutputRequest {
    path: PathBuf,
}

impl ManagedOutputRequest {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ManagedOutputRequest {
    fn drop(&mut self) {
        if let Err(error) = validation::cleanup_private_request_dir(&self.path) {
            tracing::warn!(
                path = %self.path.display(),
                error = %error,
                "failed to clean private yt-dlp request directory"
            );
        }
    }
}

pub(crate) fn supports_plugin(plugin_name: &str) -> bool {
    provider_for_plugin(plugin_name).is_ok()
}

pub(crate) fn run_plugin_request(
    plugin_name: &str,
    request: PluginYtDlpRequest,
) -> anyhow::Result<YtDlpResponse> {
    let provider = provider_for_plugin(plugin_name)?;
    process::execute(request::prepare(provider, request, &managed_temp_root()?)?)
}

pub(crate) fn run_legacy_request(
    plugin_name: &str,
    request: LegacySubprocessRequest,
) -> anyhow::Result<YtDlpResponse> {
    process::execute(legacy::prepare(
        plugin_name,
        request,
        &managed_temp_root()?,
    )?)
}

pub(crate) fn run_generic_metadata(url: String) -> anyhow::Result<YtDlpResponse> {
    let request = PluginYtDlpRequest::Metadata {
        url,
        playlist: true,
    };
    process::execute(request::prepare(
        YtDlpProvider::Generic,
        request,
        &managed_temp_root()?,
    )?)
}

pub(crate) fn managed_output_request() -> anyhow::Result<ManagedOutputRequest> {
    let temp_root = managed_temp_root()?;
    let output_root = temp_root.join("vortex-downloads");
    validation::ensure_private_root(&output_root)?;
    Ok(ManagedOutputRequest {
        path: validation::create_private_child(&output_root, "request-")?,
    })
}

fn managed_temp_root() -> anyhow::Result<PathBuf> {
    let cache = dirs::cache_dir()
        .ok_or_else(|| anyhow::anyhow!("run_ytdlp: user cache directory is unavailable"))?;
    let cache = validation::ensure_owned_cache_directory(&cache)?;
    let app_root = cache.join("vortex");
    validation::ensure_private_root(&app_root)?;
    let broker_root = app_root.join("ytdlp");
    validation::ensure_private_root(&broker_root)?;
    Ok(broker_root)
}

pub(super) fn provider_for_plugin(plugin_name: &str) -> anyhow::Result<YtDlpProvider> {
    match plugin_name {
        "vortex-mod-youtube" => Ok(YtDlpProvider::Youtube),
        "vortex-mod-vimeo" => Ok(YtDlpProvider::Vimeo),
        "vortex-mod-soundcloud" => Ok(YtDlpProvider::Soundcloud),
        _ => bail!("run_ytdlp: plugin '{plugin_name}' is not approved for yt-dlp"),
    }
}
