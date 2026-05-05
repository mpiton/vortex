//! Metalink mirror — alternative source URL for the same file.
//!
//! Modelled after the Metalink XML `<url>` element (RFC 5854): one mirror
//! per CDN edge or geographic region, with optional preference + country
//! hints used to pick the best candidate at the start of a download.
//!
//! Per the architecture rule, this module imports `std` only. The SQLite
//! adapter is responsible for serialising the list to JSON via its own
//! DTO; the domain refuses to know about persistence formats.

use crate::domain::error::DomainError;
use crate::domain::model::download::Url;

/// One alternative source for a download.
///
/// `priority` follows the Metalink convention reversed for clarity: higher
/// values are preferred. Allowed range is `1..=100`. `country` is an
/// optional ISO-3166 alpha-2 code (`"US"`, `"DE"`…) — useful for routing
/// downloads to a geographically close mirror, but only validated for
/// shape, not for membership in the actual ISO list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mirror {
    url: Url,
    priority: u8,
    country: Option<String>,
}

impl Mirror {
    /// Build a new mirror, validating the priority range and the country
    /// shape. Returns [`DomainError::ValidationError`] when either is out
    /// of contract.
    pub fn new(url: Url, priority: u8, country: Option<String>) -> Result<Self, DomainError> {
        if !(1..=100).contains(&priority) {
            return Err(DomainError::ValidationError(format!(
                "mirror priority must be in 1..=100, got {priority}"
            )));
        }
        let country_norm = match country {
            Some(c) => {
                if c.len() != 2 || !c.chars().all(|ch| ch.is_ascii_alphabetic()) {
                    return Err(DomainError::ValidationError(format!(
                        "mirror country must be ISO-3166 alpha-2, got '{c}'"
                    )));
                }
                Some(c.to_ascii_uppercase())
            }
            None => None,
        };
        Ok(Mirror {
            url,
            priority,
            country: country_norm,
        })
    }

    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn priority(&self) -> u8 {
        self.priority
    }

    pub fn country(&self) -> Option<&str> {
        self.country.as_deref()
    }
}

/// Sort mirrors so the highest-priority entry comes first; ties are
/// broken by the URL string for deterministic ordering across runs (the
/// engine's failover loop must visit mirrors in a stable sequence
/// otherwise the same download retried twice would prefer different
/// sources).
pub fn sort_by_priority(mirrors: &mut [Mirror]) {
    mirrors.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.url.as_str().cmp(b.url.as_str()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::new(s).expect("test url")
    }

    #[test]
    fn test_mirror_new_accepts_priority_in_range() {
        let m = Mirror::new(url("https://a.example.com/f"), 50, None).unwrap();
        assert_eq!(m.priority(), 50);
        assert!(m.country().is_none());
    }

    #[test]
    fn test_mirror_new_rejects_priority_zero() {
        let err = Mirror::new(url("https://a.example.com/f"), 0, None).unwrap_err();
        assert!(matches!(err, DomainError::ValidationError(_)));
    }

    #[test]
    fn test_mirror_new_rejects_priority_above_100() {
        let err = Mirror::new(url("https://a.example.com/f"), 101, None).unwrap_err();
        assert!(matches!(err, DomainError::ValidationError(_)));
    }

    #[test]
    fn test_mirror_new_normalizes_country_to_uppercase() {
        let m = Mirror::new(url("https://a.example.com/f"), 10, Some("us".to_string())).unwrap();
        assert_eq!(m.country(), Some("US"));
    }

    #[test]
    fn test_mirror_new_rejects_country_wrong_length() {
        let err =
            Mirror::new(url("https://a.example.com/f"), 10, Some("USA".to_string())).unwrap_err();
        assert!(matches!(err, DomainError::ValidationError(_)));
    }

    #[test]
    fn test_mirror_new_rejects_country_non_alpha() {
        let err =
            Mirror::new(url("https://a.example.com/f"), 10, Some("U5".to_string())).unwrap_err();
        assert!(matches!(err, DomainError::ValidationError(_)));
    }

    #[test]
    fn test_sort_by_priority_orders_highest_first() {
        let mut mirrors = vec![
            Mirror::new(url("https://low.example.com/f"), 10, None).unwrap(),
            Mirror::new(url("https://high.example.com/f"), 90, None).unwrap(),
            Mirror::new(url("https://mid.example.com/f"), 50, None).unwrap(),
        ];
        sort_by_priority(&mut mirrors);
        assert_eq!(mirrors[0].priority(), 90);
        assert_eq!(mirrors[1].priority(), 50);
        assert_eq!(mirrors[2].priority(), 10);
    }

    #[test]
    fn test_sort_by_priority_ties_broken_by_url_lexicographically() {
        let mut mirrors = vec![
            Mirror::new(url("https://b.example.com/f"), 50, None).unwrap(),
            Mirror::new(url("https://a.example.com/f"), 50, None).unwrap(),
            Mirror::new(url("https://c.example.com/f"), 50, None).unwrap(),
        ];
        sort_by_priority(&mut mirrors);
        assert_eq!(mirrors[0].url().host(), "a.example.com");
        assert_eq!(mirrors[1].url().host(), "b.example.com");
        assert_eq!(mirrors[2].url().host(), "c.example.com");
    }
}
