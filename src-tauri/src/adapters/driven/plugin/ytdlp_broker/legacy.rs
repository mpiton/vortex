use std::path::Path;

use anyhow::bail;

use super::request::{append_url, prepare as prepare_typed, secure_prefix, strings};
use super::validation::validate_url;
use super::{
    DEFAULT_TIMEOUT, LegacySubprocessRequest, PluginYtDlpRequest, PreparedCommand, YtDlpProvider,
    legacy_download, provider_for_plugin,
};

pub(super) fn prepare(
    plugin_name: &str,
    request: LegacySubprocessRequest,
    temp_root: &Path,
) -> anyhow::Result<PreparedCommand> {
    if request.binary != "yt-dlp" {
        bail!("run_subprocess compatibility: binary replacement is forbidden");
    }
    let provider = provider_for_plugin(plugin_name)?;
    if contains_dangerous_option(&request.args) {
        bail!("run_subprocess compatibility: arguments are not approved");
    }
    if let Some(prepared) = metadata_profile(provider, &request.args, temp_root) {
        return prepared;
    }
    if let Some(prepared) = resolve_profile(provider, &request.args, temp_root) {
        return prepared;
    }
    let _ignored_timeout = request.timeout_ms;
    legacy_download::prepare(provider, &request.args, temp_root)
}

fn metadata_profile(
    provider: YtDlpProvider,
    args: &[String],
    temp_root: &Path,
) -> Option<anyhow::Result<PreparedCommand>> {
    let matches = provider == YtDlpProvider::Youtube
        && args.len() == 5
        && args[0] == "--dump-json"
        && matches!(args[1].as_str(), "--no-playlist" | "--flat-playlist")
        && args[2] == "--no-warnings"
        && args[3] == "--";
    matches.then(|| {
        prepare_typed(
            provider,
            PluginYtDlpRequest::Metadata {
                url: args[4].clone(),
                playlist: args[1] == "--flat-playlist",
            },
            temp_root,
        )
    })
}

fn resolve_profile(
    provider: YtDlpProvider,
    args: &[String],
    temp_root: &Path,
) -> Option<anyhow::Result<PreparedCommand>> {
    let matches = provider == YtDlpProvider::Youtube
        && args.len() == 7
        && args[0] == "--get-url"
        && args[1] == "--no-playlist"
        && args[2] == "--no-warnings"
        && args[3] == "--format"
        && args[5] == "--";
    matches.then(|| rebuild_resolve(provider, args, temp_root))
}

fn rebuild_resolve(
    provider: YtDlpProvider,
    args: &[String],
    temp_root: &Path,
) -> anyhow::Result<PreparedCommand> {
    validate_selector(&args[4])?;
    let mut rebuilt = secure_prefix();
    rebuilt.extend(strings([
        "--get-url",
        "--no-playlist",
        "--no-warnings",
        "--format",
    ]));
    rebuilt.push(args[4].clone());
    append_url(&mut rebuilt, validate_url(provider, &args[6])?);
    Ok(PreparedCommand {
        args: rebuilt,
        working_dir: temp_root.join("vortex-ytdlp"),
        timeout: DEFAULT_TIMEOUT,
    })
}

fn contains_dangerous_option(args: &[String]) -> bool {
    const BLOCKED: [&str; 6] = [
        "--exec",
        "--config-location",
        "--config-locations",
        "--plugin-dirs",
        "--netrc-cmd",
        "--use-postprocessor",
    ];
    args.iter().any(|arg| {
        BLOCKED
            .iter()
            .any(|blocked| arg == blocked || arg.starts_with(&format!("{blocked}=")))
    })
}

pub(super) fn validate_selector(selector: &str) -> anyhow::Result<()> {
    let safe = !selector.is_empty()
        && selector.len() <= 1024
        && selector.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(
                    character,
                    '[' | ']' | '<' | '>' | '=' | '+' | '/' | '_' | '.' | ',' | '-'
                )
        });
    if !safe {
        bail!("run_subprocess compatibility: format selector is not approved");
    }
    Ok(())
}
