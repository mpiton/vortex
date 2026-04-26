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
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| {
            DomainError::ValidationError(format!("URL must start with http(s)://, got '{url}'"))
        })?;

    // Reject scheme-only inputs (`https://`), missing-authority shapes
    // (`https:///foo`, `https://?x`, `https://#x`) and any whitespace,
    // which would derail the OS launcher even though the prefix check
    // passed.
    if rest.is_empty()
        || rest.starts_with('/')
        || rest.starts_with('?')
        || rest.starts_with('#')
        || url.chars().any(char::is_whitespace)
    {
        return Err(DomainError::ValidationError(format!(
            "invalid http(s) URL: '{url}'"
        )));
    }

    // Authority MUST carry a non-empty host. RFC 3986 leaves the door
    // open for `https://:443/x` (port-only) and `https://user@/x`
    // (userinfo without host) — both are accepted by the prefix check
    // but mean nothing to a browser and just produce a launcher error.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let (host, port) = if let Some(rest) = host_port.strip_prefix('[') {
        // Bracketed IPv6 host: require `]`, non-empty literal, and an
        // optional `:port` tail — anything else (`[::1]oops`) is junk.
        let end = rest
            .find(']')
            .ok_or_else(|| DomainError::ValidationError(format!("invalid http(s) URL: '{url}'")))?;
        if end == 0 {
            return Err(DomainError::ValidationError(format!(
                "http(s) URL has empty host: '{url}'"
            )));
        }
        let tail = &rest[end + 1..];
        if !tail.is_empty() && !tail.starts_with(':') {
            return Err(DomainError::ValidationError(format!(
                "invalid http(s) URL: '{url}'"
            )));
        }
        (&rest[..end], tail.strip_prefix(':'))
    } else {
        match host_port.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (host_port, None),
        }
    };
    if host.is_empty() {
        return Err(DomainError::ValidationError(format!(
            "http(s) URL has empty host: '{url}'"
        )));
    }
    if let Some(p) = port
        && (p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(DomainError::ValidationError(format!(
            "http(s) URL has non-numeric port: '{url}'"
        )));
    }

    Ok(())
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

    #[test]
    fn validate_http_url_rejects_missing_authority() {
        // Scheme-only or no-host shapes used to slip past the prefix check
        // and bubble up as a useless launcher error.
        for bad in [
            "https://",
            "http://",
            "https:///etc/passwd",
            "https://?title=x",
            "https://#frag",
            "https:// example.com",
        ] {
            let err = validate_http_url(bad).unwrap_err();
            assert!(
                matches!(err, DomainError::ValidationError(_)),
                "expected ValidationError for {bad:?}, got {err:?}"
            );
        }
    }

    #[test]
    fn validate_http_url_rejects_empty_host_authority() {
        // Authority shapes that survive the prefix / leading-char check
        // but still carry no usable host: a stray port, a userinfo block
        // without a host, or an unclosed/empty IPv6 literal.
        for bad in [
            "https://:443/path",
            "https://@/x",
            "https://user@/x",
            "https://user:pwd@/x",
            "https://[]/foo",
            "https://[/foo",
        ] {
            let err = validate_http_url(bad).unwrap_err();
            assert!(
                matches!(err, DomainError::ValidationError(_)),
                "expected ValidationError for {bad:?}, got {err:?}"
            );
        }
    }

    #[test]
    fn validate_http_url_accepts_userinfo_and_ipv6() {
        // Valid authorities should still pass — a userinfo prefix or an
        // IPv6 literal with a real host must not be classified as empty.
        for good in [
            "https://user:pass@example.com/path",
            "https://[::1]/path",
            "https://[2001:db8::1]:8080/foo",
            "http://example.com:8080/x",
        ] {
            assert!(
                validate_http_url(good).is_ok(),
                "expected {good:?} to validate"
            );
        }
    }

    #[test]
    fn validate_http_url_rejects_malformed_port_or_bracket_tail() {
        // Port must be all-digits and an IPv6 literal must be followed
        // by either nothing or `:port` — junk after `]` is invalid.
        for bad in [
            "https://example.com:abc/x",
            "https://example.com:/x",
            "https://example.com:80a/x",
            "https://[::1]oops/x",
            "https://[::1]:abc/x",
        ] {
            let err = validate_http_url(bad).unwrap_err();
            assert!(
                matches!(err, DomainError::ValidationError(_)),
                "expected ValidationError for {bad:?}, got {err:?}"
            );
        }
    }
}
