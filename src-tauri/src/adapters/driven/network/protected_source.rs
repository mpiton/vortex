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
                BodyPrefixDecision::Accept
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
    const HTML_SIGNATURES: [&[u8]; 4] = [b"<!doctype html", b"<html", b"<head", b"<body"];
    let mut may_be_html = false;
    for signature in HTML_SIGNATURES {
        if prefix.starts_with(signature) {
            return match prefix.get(signature.len()) {
                Some(byte) if byte.is_ascii_whitespace() || matches!(byte, b'>' | b'/') => {
                    BodyPrefixDecision::Reject
                }
                None if can_read_more => BodyPrefixDecision::NeedMore,
                None => BodyPrefixDecision::Reject,
                Some(_) => continue,
            };
        }
        may_be_html |= signature.starts_with(prefix);
    }
    if may_be_html && can_read_more {
        BodyPrefixDecision::NeedMore
    } else {
        BodyPrefixDecision::Accept
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
