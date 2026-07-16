use std::path::{Path, PathBuf};

use anyhow::bail;

use super::legacy::validate_selector;
use super::request::secure_prefix;
use super::validation::{private_output_dir, validate_url};
use super::{DEFAULT_OUTPUT_LIMIT, DOWNLOAD_TIMEOUT, PreparedCommand, YtDlpProvider};

struct LegacyProfile<'a> {
    selector: Option<&'a str>,
    output_template: &'a str,
    output_index: usize,
    url: &'a str,
}

pub(super) fn prepare(
    provider: YtDlpProvider,
    args: &[String],
    temp_root: &Path,
) -> anyhow::Result<PreparedCommand> {
    let profile = match_profile(provider, args)?;
    if let Some(selector) = profile.selector {
        validate_selector(selector)?;
    }
    let url = validate_url(provider, profile.url)?;
    let output_dir = parse_output_dir(profile.output_template, temp_root)?;
    let mut rebuilt = secure_prefix();
    let offset = rebuilt.len();
    rebuilt.extend_from_slice(args);
    rebuilt[offset + profile.output_index] = output_template(&output_dir);
    let last = rebuilt.len() - 1;
    rebuilt[last] = url;
    Ok(PreparedCommand {
        args: rebuilt,
        working_dir: output_dir,
        timeout: DOWNLOAD_TIMEOUT,
        stdout_limit: DEFAULT_OUTPUT_LIMIT,
        stderr_limit: DEFAULT_OUTPUT_LIMIT,
        cleanup_working_dir_on_failure: true,
    })
}

fn match_profile(provider: YtDlpProvider, args: &[String]) -> anyhow::Result<LegacyProfile<'_>> {
    match provider {
        YtDlpProvider::Youtube if youtube_profile(args) => {
            validate_merge_format(&args[3])?;
            Ok(LegacyProfile {
                selector: Some(&args[1]),
                output_template: &args[5],
                output_index: 5,
                url: &args[18],
            })
        }
        YtDlpProvider::Vimeo if vimeo_profile(args) => {
            validate_merge_format(&args[3])?;
            Ok(LegacyProfile {
                selector: Some(&args[1]),
                output_template: &args[5],
                output_index: 5,
                url: &args[16],
            })
        }
        YtDlpProvider::Soundcloud if soundcloud_profile(args) => {
            validate_audio_format(&args[2])?;
            Ok(LegacyProfile {
                selector: None,
                output_template: &args[4],
                output_index: 4,
                url: &args[11],
            })
        }
        _ => bail!("run_subprocess compatibility: arguments are not an approved yt-dlp profile"),
    }
}

fn validate_audio_format(format: &str) -> anyhow::Result<()> {
    if !matches!(
        format,
        "aac" | "alac" | "best" | "flac" | "m4a" | "mp3" | "opus" | "vorbis" | "wav"
    ) {
        bail!("run_subprocess compatibility: audio format is not approved");
    }
    Ok(())
}

fn youtube_profile(args: &[String]) -> bool {
    args.len() == 19
        && args[0] == "--format"
        && args[2] == "--merge-output-format"
        && args[4] == "--output"
        && common_tail(&args[6..])
        && args[11] == "--extractor-args"
        && args[12] == "youtube:player_client=default,web_safari,android_vr,tv"
        && retry_tail(&args[13..], 5)
}

fn vimeo_profile(args: &[String]) -> bool {
    args.len() == 17
        && args[0] == "--format"
        && args[2] == "--merge-output-format"
        && args[4] == "--output"
        && common_tail(&args[6..])
        && retry_tail(&args[11..], 5)
}

fn soundcloud_profile(args: &[String]) -> bool {
    args.len() == 12
        && args[0] == "--extract-audio"
        && args[1] == "--audio-format"
        && args[3] == "--output"
        && common_tail(&args[5..])
        && args[10] == "--"
}

fn common_tail(args: &[String]) -> bool {
    args.first().is_some_and(|arg| arg == "--print")
        && args
            .get(1)
            .is_some_and(|arg| arg == "after_move:%(filepath)s")
        && args.get(2).is_some_and(|arg| arg == "--no-playlist")
        && args.get(3).is_some_and(|arg| arg == "--no-warnings")
        && args.get(4).is_some_and(|arg| arg == "--quiet")
}

fn retry_tail(args: &[String], sentinel: usize) -> bool {
    args.first().is_some_and(|arg| arg == "--retries")
        && args.get(1).is_some_and(|arg| arg == "3")
        && args.get(2).is_some_and(|arg| arg == "--fragment-retries")
        && args.get(3).is_some_and(|arg| arg == "3")
        && args.get(sentinel - 1).is_some_and(|arg| arg == "--")
}

fn validate_merge_format(format: &str) -> anyhow::Result<()> {
    if !matches!(format, "mkv" | "mov" | "mp4" | "webm") {
        bail!("run_subprocess compatibility: merge format is not approved");
    }
    Ok(())
}

fn parse_output_dir(template: &str, temp_root: &Path) -> anyhow::Result<PathBuf> {
    let raw = template.strip_suffix("/%(id)s.%(ext)s").ok_or_else(|| {
        anyhow::anyhow!("run_subprocess compatibility: output template is not approved")
    })?;
    if raw.replace("%%", "").contains('%') {
        bail!("run_subprocess compatibility: output template is not approved");
    }
    private_output_dir(&raw.replace("%%", "%"), temp_root)
}

fn output_template(output_dir: &Path) -> String {
    format!(
        "{}/%(id)s.%(ext)s",
        output_dir.to_string_lossy().replace('%', "%%")
    )
}
