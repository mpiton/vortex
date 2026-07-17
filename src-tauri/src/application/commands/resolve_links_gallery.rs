//! Gallery expansion for `handle_resolve_links`.
//!
//! Crawler plugins that answer `extract_links` with a `kind: "gallery"`
//! payload (e.g. vortex-mod-gallery) expand one gallery URL into one
//! resolved link per image, mirroring the hoster 1→N expansion. Per-image
//! failures become individual error rows so a partially invalid gallery
//! never silently cancels the valid images (MAT-134 R-05).

use serde::Deserialize;
use uuid::Uuid;

use crate::application::command_bus::CommandBus;

use super::resolve_links::{LinkResolutionErrorKind, ResolvedLinkDto, extract_filename_from_url};

#[derive(Deserialize)]
struct GalleryResponse {
    kind: Option<String>,
    #[serde(default)]
    images: Vec<GalleryImage>,
}

#[derive(Deserialize)]
struct GalleryImage {
    #[serde(default)]
    url: String,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    title: Option<String>,
}

impl CommandBus {
    /// Expand a crawler gallery URL into per-image resolved links.
    ///
    /// Returns `None` when the payload parses but is not a gallery, so the
    /// caller can fall through to the generic HEAD probe; an unparseable
    /// payload becomes a single error row instead. At most `max_rows`
    /// images are expanded — extras are dropped (and logged) so one
    /// oversized gallery cannot fail the whole resolve.
    pub(super) fn try_resolve_gallery_links(
        &self,
        url: &str,
        module_name: &str,
        max_rows: usize,
    ) -> Option<Vec<ResolvedLinkDto>> {
        let raw = match self.plugin_loader().extract_links(url) {
            Ok(raw) => raw,
            Err(e) => {
                tracing::debug!(module_name, error = %e, "gallery extraction failed");
                return Some(vec![gallery_error_row(
                    url,
                    module_name,
                    LinkResolutionErrorKind::Plugin,
                    "Could not extract gallery links",
                )]);
            }
        };
        let parsed: GalleryResponse = match serde_json::from_str(&raw) {
            Ok(parsed) => parsed,
            Err(e) => {
                tracing::debug!(module_name, error = %e, "gallery payload is not valid JSON");
                return Some(vec![gallery_error_row(
                    url,
                    module_name,
                    LinkResolutionErrorKind::Plugin,
                    "Gallery plugin returned an invalid response",
                )]);
            }
        };
        if parsed.kind.as_deref() != Some("gallery") {
            return None;
        }
        if parsed.images.is_empty() {
            return Some(vec![gallery_error_row(
                url,
                module_name,
                LinkResolutionErrorKind::NoFile,
                "Gallery contains no downloadable images",
            )]);
        }
        if parsed.images.len() > max_rows {
            tracing::warn!(
                module_name,
                total = parsed.images.len(),
                kept = max_rows,
                "gallery exceeds the resolve output limit; extra images dropped"
            );
        }
        Some(
            parsed
                .images
                .iter()
                .take(max_rows)
                .enumerate()
                .map(|(index, image)| gallery_image_row(url, module_name, index, image))
                .collect(),
        )
    }
}

fn gallery_image_row(
    gallery_url: &str,
    module_name: &str,
    index: usize,
    image: &GalleryImage,
) -> ResolvedLinkDto {
    let image_url = image.url.trim();
    if !is_http_image_url(image_url) {
        let label = image
            .title
            .as_deref()
            .filter(|t| !t.trim().is_empty())
            .map(|t| format!("'{t}'"))
            .unwrap_or_else(|| format!("#{}", index + 1));
        return gallery_error_row(
            gallery_url,
            module_name,
            LinkResolutionErrorKind::Plugin,
            &format!("Gallery item {label} has an invalid image URL"),
        );
    }
    let filename = image
        .filename
        .clone()
        .filter(|name| !name.trim().is_empty())
        .or_else(|| extract_filename_from_url(image_url));
    ResolvedLinkDto {
        id: Uuid::new_v4().to_string(),
        original_url: image_url.to_string(),
        resolved_url: Some(image_url.to_string()),
        filename,
        size_bytes: None,
        resumable: None,
        status: "online".to_string(),
        error_message: None,
        error_kind: None,
        module_name: module_name.to_string(),
        account_id: None,
        is_media: false,
        media_type: Some("image".to_string()),
        // The gallery provider API already vouched for these URLs;
        // probing every image would fire one HEAD per row for nothing.
        requires_online_probe: false,
    }
}

/// `Url::parse` lowercases the scheme, so `HTTPS://…` passes; a bare
/// `https://` fails on the missing host.
fn is_http_image_url(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.has_host())
}

fn gallery_error_row(
    gallery_url: &str,
    module_name: &str,
    error_kind: LinkResolutionErrorKind,
    message: &str,
) -> ResolvedLinkDto {
    ResolvedLinkDto {
        id: Uuid::new_v4().to_string(),
        original_url: gallery_url.to_string(),
        resolved_url: None,
        filename: None,
        size_bytes: None,
        resumable: None,
        status: "error".to_string(),
        error_message: Some(message.to_string()),
        error_kind: Some(error_kind),
        module_name: module_name.to_string(),
        account_id: None,
        is_media: false,
        media_type: None,
        requires_online_probe: false,
    }
}

#[cfg(test)]
#[path = "resolve_links_gallery_tests.rs"]
mod tests;
