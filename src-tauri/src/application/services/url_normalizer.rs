//! URL normalization for duplicate detection (PRD §6.2.2).
//!
//! Produces a canonical string for a URL by lowercasing the scheme and
//! host, stripping the fragment, default ports, trailing slash, and a
//! curated list of well-known tracking query parameters
//! (`utm_*`, `fbclid`, `gclid`, …). The result is purely lexical — two
//! URLs that differ only in tracking metadata or capitalisation collapse
//! to the same key, so a freshly pasted "shared from Twitter" link
//! matches the original entry already in the queue / history.
//!
//! The normalizer never fetches the network and does not require the
//! `url` crate (kept out of the dependency tree to avoid bloating the
//! Tauri bundle); parsing is a small hand-rolled scanner that mirrors
//! the rules already used by `domain::model::download::Url`.

/// Canonicalise a URL string.
///
/// Behaviour:
///
/// * Whitespace is trimmed.
/// * The scheme and host components are lowercased.
/// * The default port (`:80` for `http`, `:443` for `https`, `:21` for
///   `ftp`) is stripped.
/// * Tracking query parameters (`utm_*`, `fbclid`, `gclid`, `mc_cid`,
///   `mc_eid`, `igshid`, `vero_id`, …) are removed, comparing parameter
///   names case-insensitively. Remaining parameters are kept in their
///   original order so the caller can still distinguish requests that
///   genuinely depend on parameter ordering.
/// * The fragment (`#…`) is removed.
/// * A trailing `/` on the empty path is preserved as `/`; bare hosts
///   without a path get a `/` appended so `https://example.com` and
///   `https://example.com/` collapse to the same key.
/// * Schemes that the rest of Vortex never rewrites (`magnet:`, `file:`,
///   …) are returned trimmed but otherwise untouched, so duplicate
///   detection on non-HTTP entries falls back to byte-for-byte equality.
pub fn normalize_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let lower_for_scheme = trimmed.to_ascii_lowercase();
    let scheme_len = if lower_for_scheme.starts_with("https://") {
        "https://".len()
    } else if lower_for_scheme.starts_with("http://") {
        "http://".len()
    } else if lower_for_scheme.starts_with("ftp://") {
        "ftp://".len()
    } else {
        return trimmed.to_string();
    };
    let scheme = &lower_for_scheme[..scheme_len - 3];

    let after_scheme = &trimmed[scheme_len..];

    // Strip fragment first so it never leaks into the path/query.
    let (without_frag, _frag) = match after_scheme.find('#') {
        Some(i) => (&after_scheme[..i], Some(&after_scheme[i..])),
        None => (after_scheme, None),
    };

    let (authority_path, query) = match without_frag.find('?') {
        Some(i) => (&without_frag[..i], Some(&without_frag[i + 1..])),
        None => (without_frag, None),
    };

    let (authority_raw, path) = match authority_path.find('/') {
        Some(i) => (&authority_path[..i], &authority_path[i..]),
        None => (authority_path, "/"),
    };

    if authority_raw.is_empty() {
        return trimmed.to_string();
    }

    let authority_lower = authority_raw.to_ascii_lowercase();
    let canonical_authority = strip_default_port(scheme, &authority_lower);

    let canonical_query = match query {
        Some(q) => filter_tracking_params(q),
        None => String::new(),
    };

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

/// `true` when a parameter name is on the curated tracker block-list.
///
/// Matching is case-insensitive. Wildcards are spelled out instead of
/// using a regex so the cost stays predictable on huge URL batches.
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
}
