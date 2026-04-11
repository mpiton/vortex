//! Shared image-link type used across providers.

use serde::Serialize;

/// A single image discovered by a provider.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImageLink {
    /// Direct HTTPS URL to the image file.
    pub url: String,
    /// Width in pixels, if known.
    pub width: Option<u32>,
    /// Height in pixels, if known.
    pub height: Option<u32>,
    /// Human title / caption, if known.
    pub title: Option<String>,
    /// Auto-generated filename, if [`crate::filter::auto_name`] ran.
    pub filename: Option<String>,
}

/// Gallery provider that produced an [`ImageLink`]. Mirrored in
/// `url_matcher::Provider` — re-exported here so `filter.rs` can depend
/// on `link.rs` without a circular import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Imgur,
    Reddit,
    Flickr,
    Generic,
}

impl From<crate::url_matcher::Provider> for Provider {
    fn from(p: crate::url_matcher::Provider) -> Self {
        match p {
            crate::url_matcher::Provider::Imgur => Self::Imgur,
            crate::url_matcher::Provider::Reddit => Self::Reddit,
            crate::url_matcher::Provider::Flickr => Self::Flickr,
            crate::url_matcher::Provider::Generic => Self::Generic,
        }
    }
}
