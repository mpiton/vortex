//! Deserialisation boundary for the generic hoster plugin wire format.

use serde::Deserialize;

use crate::domain::error::DomainError;
use crate::domain::ports::driven::ExtractedHosterLink;

#[derive(Deserialize)]
struct HosterResponse {
    files: Vec<HosterFile>,
}

#[derive(Deserialize)]
struct HosterFile {
    url: String,
    filename: Option<String>,
    size_bytes: Option<u64>,
    direct_url: Option<String>,
    traffic_used_bytes: Option<u64>,
    traffic_total_bytes: Option<u64>,
}

pub(super) fn parse_hoster_link(payload: &str) -> Result<ExtractedHosterLink, DomainError> {
    let response: HosterResponse = serde_json::from_str(payload)
        .map_err(|_| DomainError::PluginError("hoster returned an invalid response".into()))?;
    let file = response
        .files
        .into_iter()
        .next()
        .ok_or_else(|| DomainError::PluginError("hoster returned no file".into()))?;
    Ok(ExtractedHosterLink {
        source_url: file.url,
        filename: file.filename,
        size_bytes: file.size_bytes,
        direct_url: file.direct_url,
        traffic_used_bytes: file.traffic_used_bytes,
        traffic_total_bytes: file.traffic_total_bytes,
    })
}
