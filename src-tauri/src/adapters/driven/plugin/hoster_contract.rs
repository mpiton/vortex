//! Deserialisation boundary for the generic hoster plugin wire format.

use std::collections::BTreeMap;

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
    resumable: Option<bool>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    traffic_used_bytes: Option<u64>,
    traffic_total_bytes: Option<u64>,
}

#[cfg(test)]
pub(super) fn parse_hoster_link(payload: &str) -> Result<ExtractedHosterLink, DomainError> {
    parse_hoster_links(payload)?
        .into_iter()
        .next()
        .ok_or(DomainError::HosterNoFile)
}

pub(super) fn parse_hoster_links(payload: &str) -> Result<Vec<ExtractedHosterLink>, DomainError> {
    let response: HosterResponse = serde_json::from_str(payload)
        .map_err(|_| DomainError::PluginError("hoster returned an invalid response".into()))?;
    if response.files.is_empty() {
        return Err(DomainError::HosterNoFile);
    }
    Ok(response
        .files
        .into_iter()
        .map(|file| ExtractedHosterLink {
            source_url: file.url,
            filename: file.filename,
            size_bytes: file.size_bytes,
            direct_url: file.direct_url,
            resumable: file.resumable,
            request_headers: file.headers.into_iter().collect(),
            traffic_used_bytes: file.traffic_used_bytes,
            traffic_total_bytes: file.traffic_total_bytes,
        })
        .collect())
}
