//! Deserialisation boundary for the generic hoster plugin wire format.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::domain::error::DomainError;
use crate::domain::ports::driven::ExtractedHosterLink;

const MAX_HOSTER_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;
const MAX_HOSTER_FILES: usize = 500;
const MAX_URL_BYTES: usize = 8 * 1024;
const MAX_FILENAME_BYTES: usize = 4 * 1024;
const MAX_HEADERS_PER_FILE: usize = 32;
const MAX_HEADER_NAME_BYTES: usize = 256;
const MAX_HEADER_VALUE_BYTES: usize = 16 * 1024;

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
    if payload.len() > MAX_HOSTER_PAYLOAD_BYTES {
        return Err(limit_error());
    }
    let response: HosterResponse = serde_json::from_str(payload)
        .map_err(|_| DomainError::PluginError("hoster returned an invalid response".into()))?;
    if response.files.is_empty() {
        return Err(DomainError::HosterNoFile);
    }
    if response.files.len() > MAX_HOSTER_FILES {
        return Err(limit_error());
    }
    response
        .files
        .into_iter()
        .map(|file| {
            let source_url = bounded_required_url(file.url)?;
            let direct_url =
                bounded_required_url(file.direct_url.ok_or(DomainError::HosterNoFile)?)?;
            if file
                .filename
                .as_ref()
                .is_some_and(|name| name.len() > MAX_FILENAME_BYTES)
                || file.headers.len() > MAX_HEADERS_PER_FILE
                || file.headers.iter().any(|(name, value)| {
                    name.len() > MAX_HEADER_NAME_BYTES || value.len() > MAX_HEADER_VALUE_BYTES
                })
            {
                return Err(limit_error());
            }
            Ok(ExtractedHosterLink {
                source_url,
                filename: file.filename.filter(|name| !name.trim().is_empty()),
                size_bytes: file.size_bytes,
                direct_url: Some(direct_url),
                resumable: file.resumable,
                request_headers: file.headers.into_iter().collect(),
                traffic_used_bytes: file.traffic_used_bytes,
                traffic_total_bytes: file.traffic_total_bytes,
            })
        })
        .collect()
}

fn bounded_required_url(value: String) -> Result<String, DomainError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(DomainError::HosterNoFile);
    }
    if value.len() > MAX_URL_BYTES {
        return Err(limit_error());
    }
    Ok(value.to_string())
}

fn limit_error() -> DomainError {
    DomainError::PluginError("hoster response exceeds safety limits".into())
}
