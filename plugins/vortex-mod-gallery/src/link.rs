//! Shared image-link type used across providers.
//!
//! The `Provider` enum lives in `url_matcher.rs` (it's the classifier
//! output) and is re-exported here as `link::Provider` so that any
//! module depending on `link.rs` — notably `filter.rs` — sees exactly
//! the same type the matcher produces. Keeping a single canonical
//! definition prevents the two-enum drift hazard flagged during PR
//! review.

use serde::Serialize;

pub use crate::url_matcher::Provider;

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
