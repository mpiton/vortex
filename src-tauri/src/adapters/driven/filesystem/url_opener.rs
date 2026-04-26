//! Platform-backed [`UrlOpener`] implementation.
//!
//! Mirrors [`super::SystemFileOpener`] but takes a URL string instead of a
//! filesystem path. The validation rule is conservative: only `http://`
//! and `https://` are accepted so the OS launcher never receives
//! `javascript:`, `file://`, or `data:` payloads from a rogue caller.

use std::process::Command;

use crate::domain::error::DomainError;
use crate::domain::ports::driven::UrlOpener;

pub struct SystemUrlOpener;

impl Default for SystemUrlOpener {
    fn default() -> Self {
        Self
    }
}

impl SystemUrlOpener {
    pub fn new() -> Self {
        Self
    }
}

impl UrlOpener for SystemUrlOpener {
    fn open_url(&self, url: &str) -> Result<(), DomainError> {
        validate_http_url(url)?;

        #[cfg(target_os = "linux")]
        let (program, args): (&str, Vec<std::ffi::OsString>) =
            ("xdg-open", vec![std::ffi::OsString::from(url)]);
        #[cfg(target_os = "macos")]
        let (program, args): (&str, Vec<std::ffi::OsString>) =
            ("open", vec![std::ffi::OsString::from(url)]);
        // `rundll32 url.dll,FileProtocolHandler <URL>` is the canonical
        // Windows shortcut for "open this URL in the default browser".
        // Unlike `cmd /C start`, it does NOT pass the URL through the
        // command interpreter, so query strings containing `&` (issue
        // body separators) or `%` (percent-encoded characters) reach the
        // shell-execute call intact.
        #[cfg(target_os = "windows")]
        let (program, args): (&str, Vec<std::ffi::OsString>) = (
            "rundll32",
            vec![
                std::ffi::OsString::from("url.dll,FileProtocolHandler"),
                std::ffi::OsString::from(url),
            ],
        );

        run_launcher(program, &args)
    }
}

fn validate_http_url(url: &str) -> Result<(), DomainError> {
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(())
    } else {
        Err(DomainError::ValidationError(format!(
            "URL must start with http(s)://, got '{url}'"
        )))
    }
}

#[cfg(not(target_os = "windows"))]
fn run_launcher(program: &str, args: &[std::ffi::OsString]) -> Result<(), DomainError> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| DomainError::StorageError(format!("failed to launch {program}: {e}")))?;
    if !status.success() {
        return Err(DomainError::StorageError(format!(
            "{program} exited with status {status}"
        )));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn run_launcher(program: &str, args: &[std::ffi::OsString]) -> Result<(), DomainError> {
    // `rundll32` returns 0 even when the user has no default browser, so
    // the exit code carries no signal worth checking. We only surface
    // process-spawn failures (missing binary, sandboxing) — those are
    // the cases where the URL really did not reach Windows. This mirrors
    // the rationale documented next to `SystemFileOpener` for the same
    // reason.
    let _status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| DomainError::StorageError(format!("failed to launch {program}: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_url_rejects_non_http_scheme() {
        let opener = SystemUrlOpener::new();
        for bad in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,<script>",
            "",
            "github.com/foo/bar",
        ] {
            let err = opener.open_url(bad).unwrap_err();
            assert!(
                matches!(err, DomainError::ValidationError(_)),
                "expected ValidationError for {bad:?}, got {err:?}"
            );
        }
    }

    #[test]
    fn open_url_accepts_http_and_https() {
        // Validation only — we don't actually launch anything in CI.
        assert!(validate_http_url("http://example.com").is_ok());
        assert!(validate_http_url("https://github.com/foo/bar/issues/new?title=x").is_ok());
    }
}
