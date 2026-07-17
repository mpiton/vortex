//! Typed output of a hoster plugin extraction.

/// One hoster file resolved by a plugin adapter.
///
/// The adapter owns deserialisation of the plugin wire format. Application
/// services receive this std-only type and never depend on JSON field names.
#[derive(Clone, PartialEq, Eq)]
pub struct ExtractedHosterLink {
    pub source_url: String,
    pub filename: Option<String>,
    pub size_bytes: Option<u64>,
    /// Ephemeral bearer URL produced for the selected account.
    pub direct_url: Option<String>,
    pub resumable: Option<bool>,
    /// Ephemeral request headers required by the direct URL.
    pub request_headers: Vec<(String, String)>,
    pub traffic_used_bytes: Option<u64>,
    pub traffic_total_bytes: Option<u64>,
}

impl std::fmt::Debug for ExtractedHosterLink {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let direct_url = self.direct_url.as_ref().map(|_| "<redacted>");
        formatter
            .debug_struct("ExtractedHosterLink")
            .field("source_url", &self.source_url)
            .field("filename", &self.filename)
            .field("size_bytes", &self.size_bytes)
            .field("direct_url", &direct_url)
            .field("resumable", &self.resumable)
            .field("request_headers", &"<redacted>")
            .field("traffic_used_bytes", &self.traffic_used_bytes)
            .field("traffic_total_bytes", &self.traffic_total_bytes)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_redacts_the_direct_url() {
        let link = ExtractedHosterLink {
            source_url: "https://1fichier.com/?abc".into(),
            filename: Some("file.zip".into()),
            size_bytes: Some(42),
            direct_url: Some("https://cdn.example/secret-token".into()),
            resumable: Some(true),
            request_headers: vec![("Authorization".into(), "Bearer secret".into())],
            traffic_used_bytes: None,
            traffic_total_bytes: None,
        };

        let debug = format!("{link:?}");
        assert!(debug.contains("source_url"));
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("secret-token"));
        assert!(!debug.contains("Bearer secret"));
    }
}
