//! Response policy for short-lived download capabilities returned by plugins.

use crate::domain::error::DomainError;

const MAX_PROTECTED_PREFIX_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourcePolicy {
    Direct,
    Protected { allow_html: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BodyPrefixDecision {
    Accept,
    NeedMore,
    Reject,
}

impl SourcePolicy {
    pub(crate) fn is_protected(self) -> bool {
        matches!(self, Self::Protected { .. })
    }

    pub(crate) fn response_error(
        self,
        status: reqwest::StatusCode,
        content_type: Option<&str>,
    ) -> Option<DomainError> {
        let Self::Protected { allow_html } = self else {
            return None;
        };
        match status {
            reqwest::StatusCode::UNAUTHORIZED => Some(DomainError::HosterAuthenticationRequired),
            reqwest::StatusCode::FORBIDDEN | reqwest::StatusCode::GONE => {
                Some(DomainError::HosterDirectUrlExpired)
            }
            status if !status.is_success() => Some(DomainError::NetworkError(
                "hoster direct URL returned an unsuccessful status".into(),
            )),
            _ if !allow_html && content_type.is_some_and(is_html_content_type) => {
                Some(DomainError::HosterUnexpectedHtml)
            }
            _ => None,
        }
    }

    pub(crate) fn body_prefix_decision(
        self,
        start_byte: u64,
        bytes: &[u8],
        end_of_stream: bool,
    ) -> BodyPrefixDecision {
        if !matches!(self, Self::Protected { allow_html: false }) || start_byte != 0 {
            return BodyPrefixDecision::Accept;
        }
        let can_read_more = !end_of_stream && bytes.len() < MAX_PROTECTED_PREFIX_BYTES;
        let Some(prefix) = normalized_prefix(bytes) else {
            return if can_read_more {
                BodyPrefixDecision::NeedMore
            } else {
                BodyPrefixDecision::Accept
            };
        };
        if prefix.is_empty() {
            return if can_read_more {
                BodyPrefixDecision::NeedMore
            } else {
                BodyPrefixDecision::Reject
            };
        }
        html_prefix_decision(&prefix, can_read_more)
    }
}

pub(crate) fn allows_html_filename(filename: &str) -> bool {
    let filename = filename.to_ascii_lowercase();
    filename.ends_with(".html") || filename.ends_with(".htm")
}

pub(crate) fn safe_source_failure(error: &DomainError) -> String {
    match error {
        DomainError::AccountInvalidCredentials
        | DomainError::AccountExpired
        | DomainError::AccountCooldown
        | DomainError::AccountQuotaExceeded
        | DomainError::HosterNoFile
        | DomainError::HosterAuthenticationRequired
        | DomainError::HosterDirectUrlExpired
        | DomainError::HosterUnexpectedHtml => error.to_string(),
        _ => "Download source could not be resolved".to_string(),
    }
}

fn is_html_content_type(content_type: &str) -> bool {
    let media_type = content_type.split(';').next().unwrap_or_default().trim();
    media_type.eq_ignore_ascii_case("text/html")
        || media_type.eq_ignore_ascii_case("application/xhtml+xml")
}

fn normalized_prefix(bytes: &[u8]) -> Option<Vec<u8>> {
    const UTF8_BOM: &[u8] = &[0xef, 0xbb, 0xbf];
    if bytes.len() < UTF8_BOM.len() && UTF8_BOM.starts_with(bytes) {
        return None;
    }
    let bytes = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
    Some(
        bytes
            .iter()
            .copied()
            .skip_while(u8::is_ascii_whitespace)
            .map(|byte| byte.to_ascii_lowercase())
            .collect(),
    )
}

fn html_prefix_decision(prefix: &[u8], can_read_more: bool) -> BodyPrefixDecision {
    const COMMENT: &[u8] = b"<!--";
    const DOCTYPE: &[u8] = b"<!doctype";

    if prefix.starts_with(COMMENT) {
        return BodyPrefixDecision::Reject;
    }
    if COMMENT.starts_with(prefix) {
        return incomplete_html_decision(can_read_more);
    }
    if DOCTYPE.starts_with(prefix) {
        return incomplete_html_decision(can_read_more);
    }
    if let Some(rest) = prefix.strip_prefix(DOCTYPE) {
        let Some(first) = rest.first() else {
            return incomplete_html_decision(can_read_more);
        };
        if !first.is_ascii_whitespace() {
            return BodyPrefixDecision::Accept;
        }
        let first_non_whitespace = rest
            .iter()
            .position(|byte| !byte.is_ascii_whitespace())
            .unwrap_or(rest.len());
        let rest = &rest[first_non_whitespace..];
        if b"html".starts_with(rest) {
            return if rest.len() == 4 {
                BodyPrefixDecision::Reject
            } else {
                incomplete_html_decision(can_read_more)
            };
        }
        if let Some(boundary) = rest.get(4)
            && rest.starts_with(b"html")
            && (boundary.is_ascii_whitespace() || matches!(boundary, b'>' | b'/'))
        {
            return BodyPrefixDecision::Reject;
        }
        return BodyPrefixDecision::Accept;
    }

    let Some(mut tag) = prefix.strip_prefix(b"<") else {
        return BodyPrefixDecision::Accept;
    };
    if tag.is_empty() {
        return incomplete_html_decision(can_read_more);
    }
    if tag.starts_with(b"?") {
        return BodyPrefixDecision::Accept;
    }
    tag = tag.strip_prefix(b"/").unwrap_or(tag);
    let name_len = tag
        .iter()
        .position(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b':')))
        .unwrap_or(tag.len());
    if name_len == 0 {
        return BodyPrefixDecision::Accept;
    }
    if name_len == tag.len() && can_read_more {
        return BodyPrefixDecision::NeedMore;
    }
    if let Some(boundary) = tag.get(name_len)
        && !(boundary.is_ascii_whitespace() || matches!(boundary, b'>' | b'/'))
    {
        return BodyPrefixDecision::Accept;
    }
    match &tag[..name_len] {
        b"svg" | b"rss" => BodyPrefixDecision::Accept,
        _ => BodyPrefixDecision::Reject,
    }
}

fn incomplete_html_decision(can_read_more: bool) -> BodyPrefixDecision {
    if can_read_more {
        BodyPrefixDecision::NeedMore
    } else {
        BodyPrefixDecision::Reject
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_binary_rejects_html_even_with_whitespace_and_bom() {
        let policy = SourcePolicy::Protected { allow_html: false };
        assert_eq!(
            policy.body_prefix_decision(0, b"\xef\xbb\xbf \n<!DOCTYPE html><html>expired", false),
            BodyPrefixDecision::Reject
        );
        assert_eq!(
            policy.body_prefix_decision(1, b"<!doctype html>", false),
            BodyPrefixDecision::Accept
        );
        for body in [
            b"PK\x03\x04archive".as_slice(),
            b"<svg xmlns=\"http://www.w3.org/2000/svg\">".as_slice(),
            b"<?xml version=\"1.0\"?><feed>".as_slice(),
            b"<rss version=\"2.0\">".as_slice(),
        ] {
            assert_eq!(
                policy.body_prefix_decision(0, body, false),
                BodyPrefixDecision::Accept
            );
        }
        assert_eq!(
            policy.body_prefix_decision(0, b"<", false),
            BodyPrefixDecision::NeedMore
        );
        assert_eq!(
            policy.body_prefix_decision(0, b"<!doctype html>", false),
            BodyPrefixDecision::Reject
        );
        for body in [
            b"<!doctype\thtml><html>expired".as_slice(),
            b"<!doctype\nhtml><html>expired".as_slice(),
            b"<!-- expired --><p>login required</p>".as_slice(),
            b"<meta charset=\"utf-8\"><p>expired</p>".as_slice(),
            b"<p>expired</p>".as_slice(),
        ] {
            assert_eq!(
                policy.body_prefix_decision(0, body, false),
                BodyPrefixDecision::Reject,
                "{body:?}"
            );
        }
    }

    #[test]
    fn protected_binary_fails_closed_on_whitespace_only_prefix_at_limit() {
        let policy = SourcePolicy::Protected { allow_html: false };
        let prefix = vec![b' '; MAX_PROTECTED_PREFIX_BYTES];

        assert_eq!(
            policy.body_prefix_decision(0, &prefix, false),
            BodyPrefixDecision::Reject
        );
    }

    #[test]
    fn protected_binary_fails_closed_on_incomplete_html_at_end_of_stream() {
        let policy = SourcePolicy::Protected { allow_html: false };

        for body in [
            b"<".as_slice(),
            b"<!doct".as_slice(),
            b"<!doctype ".as_slice(),
        ] {
            assert_eq!(
                policy.body_prefix_decision(0, body, true),
                BodyPrefixDecision::Reject,
                "{body:?}"
            );
        }
    }

    #[test]
    fn explicitly_named_html_downloads_remain_allowed() {
        assert!(allows_html_filename("report.HTML"));
        assert!(allows_html_filename("page.htm"));
        assert!(!allows_html_filename("archive.bin"));
        assert!(matches!(
            SourcePolicy::Protected { allow_html: true }.body_prefix_decision(
                0,
                b"<!doctype html><html>document",
                false
            ),
            BodyPrefixDecision::Accept
        ));
    }
}
