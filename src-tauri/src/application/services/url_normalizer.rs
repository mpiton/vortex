//! Lexical URL canonicaliser used by duplicate detection (PRD §6.2.2).
//!
//! Two URLs that differ only in capitalisation, default port, fragment
//! or a curated set of tracker query parameters (`utm_*`, `fbclid`,
//! `gclid`, …) collapse to the same key, so a freshly pasted
//! "shared from Twitter"-style link matches the original entry already
//! in the queue / history. Path case is intentionally preserved
//! because most CDNs serve case-sensitive paths — collapsing them
//! would create false duplicate hits across genuinely distinct files.
//!
//! Hand-rolled scanner — the `url` crate stays out of the Tauri bundle.

/// Canonicalise a URL string. Non-HTTP schemes (`magnet:`, `file:`, …)
/// round-trip through `trim` only so duplicate detection on those
/// entries falls back to byte-for-byte equality.
pub fn normalize_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let (scheme, scheme_len) = match detect_scheme(trimmed) {
        Some(s) => s,
        None => return trimmed.to_string(),
    };

    let after_scheme = &trimmed[scheme_len..];
    let without_frag = after_scheme
        .split_once('#')
        .map_or(after_scheme, |(a, _)| a);
    let (authority_path, query) = without_frag
        .split_once('?')
        .map_or((without_frag, None), |(a, q)| (a, Some(q)));
    let (authority_raw, path) = match authority_path.find('/') {
        Some(i) => (&authority_path[..i], &authority_path[i..]),
        None => (authority_path, "/"),
    };

    if authority_raw.is_empty() {
        return trimmed.to_string();
    }

    let canonical_authority = canonicalize_authority(scheme, authority_raw);
    let canonical_query = query.map(filter_tracking_params).unwrap_or_default();

    let mut out = String::with_capacity(trimmed.len());
    out.push_str(scheme);
    out.push_str("://");
    out.push_str(&canonical_authority);
    out.push_str(path);
    if !canonical_query.is_empty() {
        out.push('?');
        out.push_str(&canonical_query);
    }
    out
}

/// Detect the URL scheme via case-insensitive prefix match without
/// allocating a lowercased copy of the whole URL. Returns the
/// canonical lowercase scheme + the byte length consumed including the
/// `://` separator.
fn detect_scheme(url: &str) -> Option<(&'static str, usize)> {
    const CANDIDATES: &[(&str, usize)] = &[
        ("https", "https://".len()),
        ("http", "http://".len()),
        ("ftp", "ftp://".len()),
    ];
    for (scheme, total_len) in CANDIDATES {
        if url.len() >= *total_len
            && url
                .get(..scheme.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
            && url.get(scheme.len()..*total_len) == Some("://")
        {
            return Some((*scheme, *total_len));
        }
    }
    None
}

/// Normalize the authority while preserving userinfo (`user:pass@`)
/// case. Lowercasing userinfo would silently merge distinct credentials
/// (`Alice:Pwd@host` vs `alice:pwd@host`) — RFC 3986 leaves the case
/// of userinfo octets producer-defined.
fn canonicalize_authority(scheme: &str, authority: &str) -> String {
    let (userinfo, host_port) = authority
        .rsplit_once('@')
        .map_or(("", authority), |(u, h)| (u, h));
    let host_port_lower = host_port.to_ascii_lowercase();
    let canonical_host = strip_default_port(scheme, &host_port_lower);
    if userinfo.is_empty() {
        canonical_host
    } else {
        format!("{userinfo}@{canonical_host}")
    }
}

/// `true` when a parameter name is on the curated tracker block-list.
/// Wildcards are spelled out instead of using a regex so the cost
/// stays predictable on 500-URL batches.
fn is_tracking_param(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();

    if lower.starts_with("utm_") {
        return true;
    }
    if lower.starts_with("hsa_") {
        return true;
    }

    matches!(
        lower.as_str(),
        "fbclid"
            | "gclid"
            | "dclid"
            | "msclkid"
            | "yclid"
            | "twclid"
            | "ttclid"
            | "mc_cid"
            | "mc_eid"
            | "_hsenc"
            | "_hsmi"
            | "_hsfp"
            | "igshid"
            | "igsh"
            | "vero_id"
            | "vero_conv"
            | "mkt_tok"
            | "oly_anon_id"
            | "oly_enc_id"
            | "elqtrackid"
            | "elqtrack"
            | "icid"
    )
}

fn filter_tracking_params(query: &str) -> String {
    let kept: Vec<&str> = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter(|pair| {
            let name = pair.split_once('=').map_or(*pair, |(k, _)| k);
            !is_tracking_param(name)
        })
        .collect();
    kept.join("&")
}

fn strip_default_port(scheme: &str, authority: &str) -> String {
    let Some(colon) = authority.rfind(':') else {
        return authority.to_string();
    };
    // Userinfo can contain ':'; the port is only the trailing run of
    // digits. The naive `rfind` is safe because the host part (after
    // `@`) cannot contain another colon in valid URLs.
    let after_at = authority.rfind('@').map_or(0, |i| i + 1);
    if colon < after_at {
        return authority.to_string();
    }
    let port = &authority[colon + 1..];
    if !port.chars().all(|c| c.is_ascii_digit()) {
        return authority.to_string();
    }
    let default = match scheme {
        "http" => "80",
        "https" => "443",
        "ftp" => "21",
        _ => return authority.to_string(),
    };
    if port == default {
        authority[..colon].to_string()
    } else {
        authority.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_url_returns_empty_for_blank_input() {
        assert_eq!(normalize_url(""), "");
        assert_eq!(normalize_url("   "), "");
    }

    #[test]
    fn normalize_url_trims_surrounding_whitespace() {
        assert_eq!(
            normalize_url("  https://example.com/file.zip  "),
            "https://example.com/file.zip"
        );
    }

    #[test]
    fn normalize_url_lowercases_scheme_and_host() {
        assert_eq!(
            normalize_url("HTTPS://Example.COM/Path/File.ZIP"),
            "https://example.com/Path/File.ZIP"
        );
    }

    #[test]
    fn normalize_url_strips_fragment() {
        assert_eq!(
            normalize_url("https://example.com/page#section"),
            "https://example.com/page"
        );
    }

    #[test]
    fn normalize_url_strips_default_https_port() {
        assert_eq!(
            normalize_url("https://example.com:443/file.zip"),
            "https://example.com/file.zip"
        );
    }

    #[test]
    fn normalize_url_strips_default_http_port() {
        assert_eq!(
            normalize_url("http://example.com:80/file.zip"),
            "http://example.com/file.zip"
        );
    }

    #[test]
    fn normalize_url_keeps_non_default_port() {
        assert_eq!(
            normalize_url("https://example.com:8443/file.zip"),
            "https://example.com:8443/file.zip"
        );
    }

    #[test]
    fn normalize_url_strips_utm_params() {
        assert_eq!(
            normalize_url("https://example.com/file.zip?utm_source=newsletter&utm_medium=email"),
            "https://example.com/file.zip"
        );
    }

    #[test]
    fn normalize_url_strips_fbclid_and_gclid() {
        assert_eq!(
            normalize_url("https://example.com/page?fbclid=abc123&gclid=def456"),
            "https://example.com/page"
        );
    }

    #[test]
    fn normalize_url_keeps_non_tracking_query_params() {
        assert_eq!(
            normalize_url("https://example.com/search?q=rust&utm_source=tw"),
            "https://example.com/search?q=rust"
        );
    }

    #[test]
    fn normalize_url_param_match_is_case_insensitive() {
        assert_eq!(
            normalize_url("https://example.com/page?UTM_Source=foo&FBclid=bar&id=42"),
            "https://example.com/page?id=42"
        );
    }

    #[test]
    fn normalize_url_keeps_path_with_no_query() {
        assert_eq!(
            normalize_url("https://example.com/files/archive.zip"),
            "https://example.com/files/archive.zip"
        );
    }

    #[test]
    fn normalize_url_appends_root_path_when_missing() {
        assert_eq!(normalize_url("https://example.com"), "https://example.com/");
    }

    #[test]
    fn normalize_url_keeps_unknown_scheme_untouched() {
        assert_eq!(
            normalize_url("magnet:?xt=urn:btih:abc"),
            "magnet:?xt=urn:btih:abc"
        );
    }

    #[test]
    fn normalize_url_two_urls_differing_only_in_utm_share_normalized_form() {
        let a = normalize_url("https://example.com/post?id=42&utm_source=newsletter");
        let b = normalize_url("https://example.com/post?id=42&utm_source=twitter");
        assert_eq!(a, b);
    }

    #[test]
    fn normalize_url_strips_mailchimp_tokens() {
        assert_eq!(
            normalize_url("https://example.com/x?mc_cid=abc&mc_eid=def&page=2"),
            "https://example.com/x?page=2"
        );
    }

    #[test]
    fn normalize_url_strips_hubspot_tokens() {
        assert_eq!(
            normalize_url("https://example.com/x?_hsenc=foo&_hsmi=bar&hsa_acc=baz&keep=1"),
            "https://example.com/x?keep=1"
        );
    }

    #[test]
    fn normalize_url_strips_instagram_share_token() {
        assert_eq!(
            normalize_url("https://www.instagram.com/p/AbCdEf/?igshid=xyz"),
            "https://www.instagram.com/p/AbCdEf/"
        );
    }

    #[test]
    fn normalize_url_handles_userinfo_authority() {
        assert_eq!(
            normalize_url("https://user:pass@example.com/file.zip"),
            "https://user:pass@example.com/file.zip"
        );
    }

    #[test]
    fn normalize_url_does_not_normalize_path_case() {
        // PathInfo on most CDNs is case-sensitive — preserving the original
        // path avoids producing a "duplicate" hit on URLs that point to
        // genuinely distinct files differing only in case.
        let a = normalize_url("https://cdn.example.com/Files/A.zip");
        let b = normalize_url("https://cdn.example.com/files/a.zip");
        assert_ne!(a, b);
    }

    #[test]
    fn normalize_url_drops_empty_query_after_filtering() {
        assert_eq!(
            normalize_url("https://example.com/page?utm_source=abc"),
            "https://example.com/page"
        );
    }

    #[test]
    fn normalize_url_does_not_panic_on_non_ascii_prefix() {
        // A URL pasted with a stray emoji or non-ASCII prefix would
        // panic at byte-slicing in the old `detect_scheme`. Verify the
        // safe-accessor port returns the input untouched (no recognised
        // scheme) instead of crashing.
        assert_eq!(
            normalize_url("\u{1F60A}https://example.com/file.zip"),
            "\u{1F60A}https://example.com/file.zip"
        );
        assert_eq!(
            normalize_url("©http://example.com/x"),
            "©http://example.com/x"
        );
    }

    #[test]
    fn normalize_url_preserves_userinfo_case() {
        // Userinfo is producer-defined per RFC 3986; lowercasing it
        // would falsely merge distinct credentials.
        assert_eq!(
            normalize_url("https://Alice:S3cret@example.com/file.zip"),
            "https://Alice:S3cret@example.com/file.zip"
        );
        // Host is still lowercased.
        assert_eq!(
            normalize_url("https://Alice@Example.COM/x"),
            "https://Alice@example.com/x"
        );
        // Default port is still stripped from the host portion only.
        assert_eq!(
            normalize_url("https://Alice:Pwd@example.com:443/x"),
            "https://Alice:Pwd@example.com/x"
        );
    }
}
