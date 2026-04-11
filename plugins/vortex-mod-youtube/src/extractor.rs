//! yt-dlp subprocess request/response helpers.
//!
//! The actual host-function call lives in `lib.rs`. This module provides
//! pure, unit-testable helpers to build the subprocess request and parse
//! the response.

use serde::{Deserialize, Serialize};

use crate::error::PluginError;

/// JSON request shape expected by the host's `run_subprocess` function.
#[derive(Debug, Serialize)]
pub struct SubprocessRequest {
    pub binary: String,
    pub args: Vec<String>,
    pub timeout_ms: u64,
}

/// JSON response shape returned by the host's `run_subprocess` function.
#[derive(Debug, Deserialize)]
pub struct SubprocessResponse {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Default subprocess timeout — 60 seconds.
pub const DEFAULT_TIMEOUT_MS: u64 = 60_000;

/// Build the yt-dlp CLI arguments for a single video.
///
/// Uses `--dump-json` with `--no-playlist` to avoid accidentally expanding
/// playlists on mixed URLs (e.g. `watch?v=...&list=...`). A `--` sentinel is
/// inserted before the URL so that a URL accidentally starting with `-` can
/// never be interpreted as a yt-dlp option (defense in depth — URL is already
/// host-validated by [`crate::ensure_youtube_url`]).
pub fn yt_dlp_args_for_single_video(url: &str) -> Vec<String> {
    vec![
        "--dump-json".into(),
        "--no-playlist".into(),
        "--no-warnings".into(),
        "--no-call-home".into(),
        "--".into(),
        url.into(),
    ]
}

/// Build the yt-dlp CLI arguments for flat playlist extraction.
pub fn yt_dlp_args_for_playlist(url: &str) -> Vec<String> {
    vec![
        "--dump-json".into(),
        "--flat-playlist".into(),
        "--no-warnings".into(),
        "--no-call-home".into(),
        "--".into(),
        url.into(),
    ]
}

/// Serialize a subprocess request as the JSON string expected by the host.
///
/// Returns [`PluginError::SerdeJson`] in the (practically unreachable) case
/// where serde cannot serialise a struct of plain `String` and `u64` fields.
/// The contract is enforced at compile time by the `Serialize` impl, but we
/// propagate the error rather than panic to honour the project's
/// zero-unwrap/expect policy.
pub fn build_subprocess_request(args: Vec<String>) -> Result<String, PluginError> {
    let req = SubprocessRequest {
        binary: "yt-dlp".into(),
        args,
        timeout_ms: DEFAULT_TIMEOUT_MS,
    };
    Ok(serde_json::to_string(&req)?)
}

/// Deserialize the host's subprocess response JSON and extract stdout.
///
/// Returns [`PluginError::Subprocess`] when the exit code is non-zero.
pub fn parse_subprocess_response(response_json: &str) -> Result<String, PluginError> {
    let resp: SubprocessResponse = serde_json::from_str(response_json)?;

    if resp.exit_code != 0 {
        return Err(PluginError::Subprocess {
            exit_code: resp.exit_code,
            stderr: truncate_stderr(&resp.stderr),
        });
    }

    Ok(resp.stdout)
}

/// Keep stderr manageable for error messages.
///
/// Truncation is performed on character boundaries, never byte offsets, so
/// multi-byte yt-dlp output (filenames with non-ASCII titles, localised
/// messages) cannot cause a panic. In WASM that would otherwise abort the
/// plugin instance without unwinding.
fn truncate_stderr(stderr: &str) -> String {
    const MAX_CHARS: usize = 512;
    let trimmed = stderr.trim();
    let char_count = trimmed.chars().count();
    if char_count <= MAX_CHARS {
        trimmed.to_string()
    } else {
        let mut out: String = trimmed.chars().take(MAX_CHARS).collect();
        out.push('…');
        out
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_video_args_include_no_playlist_flag() {
        let args = yt_dlp_args_for_single_video("https://youtu.be/abc12345678");
        assert!(args.contains(&"--dump-json".into()));
        assert!(args.contains(&"--no-playlist".into()));
        assert!(args.contains(&"https://youtu.be/abc12345678".into()));
    }

    #[test]
    fn playlist_args_include_flat_playlist_flag() {
        let args = yt_dlp_args_for_playlist("https://www.youtube.com/playlist?list=PLxyz");
        assert!(args.contains(&"--flat-playlist".into()));
        assert!(args.contains(&"--dump-json".into()));
    }

    #[test]
    fn subprocess_request_serialises_with_yt_dlp_binary() {
        let req_json = build_subprocess_request(vec!["--version".into()]).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&req_json).unwrap();
        assert_eq!(parsed["binary"], "yt-dlp");
        assert_eq!(parsed["args"][0], "--version");
        assert_eq!(parsed["timeout_ms"], DEFAULT_TIMEOUT_MS);
    }

    #[test]
    fn parse_response_returns_stdout_on_success() {
        let json = r#"{"exit_code":0,"stdout":"ok","stderr":""}"#;
        assert_eq!(parse_subprocess_response(json).unwrap(), "ok");
    }

    #[test]
    fn parse_response_errors_on_non_zero_exit_code() {
        let json = r#"{"exit_code":1,"stdout":"","stderr":"ERROR: video unavailable"}"#;
        let result = parse_subprocess_response(json);
        match result {
            Err(PluginError::Subprocess { exit_code, stderr }) => {
                assert_eq!(exit_code, 1);
                assert!(stderr.contains("video unavailable"));
            }
            _ => panic!("expected Subprocess error, got {result:?}"),
        }
    }

    #[test]
    fn parse_response_errors_on_invalid_json() {
        let result = parse_subprocess_response("not json");
        assert!(matches!(result, Err(PluginError::SerdeJson(_))));
    }

    #[test]
    fn truncates_stderr_on_character_boundaries() {
        // Repeat a multi-byte grapheme past the character cap; ensure no panic
        // and that the truncation happens on a char boundary.
        let long = "é".repeat(2000);
        let json = format!(r#"{{"exit_code":1,"stdout":"","stderr":"{long}"}}"#);
        let result = parse_subprocess_response(&json);
        match result {
            Err(PluginError::Subprocess { stderr, .. }) => {
                // All chars are 'é' (2 bytes each); truncated to 512 + ellipsis
                assert!(stderr.chars().count() <= 513);
                assert!(stderr.ends_with('…'));
            }
            _ => panic!("expected Subprocess error"),
        }
    }

    #[test]
    fn build_request_includes_dash_dash_sentinel() {
        let args = yt_dlp_args_for_single_video("https://youtu.be/abc");
        let dash_idx = args.iter().position(|a| a == "--").expect("expected --");
        let url_idx = args
            .iter()
            .position(|a| a == "https://youtu.be/abc")
            .expect("expected url");
        assert!(dash_idx < url_idx);
    }

    #[test]
    fn truncates_long_stderr() {
        let long = "x".repeat(2000);
        let json = format!(r#"{{"exit_code":1,"stdout":"","stderr":"{long}"}}"#);
        let result = parse_subprocess_response(&json);
        match result {
            Err(PluginError::Subprocess { stderr, .. }) => {
                assert!(stderr.len() < 600);
                assert!(stderr.ends_with('…'));
            }
            _ => panic!("expected Subprocess error"),
        }
    }
}
