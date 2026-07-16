//! Typed output of a hoster plugin extraction.

/// One hoster file resolved by a plugin adapter.
///
/// The adapter owns deserialisation of the plugin wire format. Application
/// services receive this std-only type and never depend on JSON field names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedHosterLink {
    pub source_url: String,
    pub filename: Option<String>,
    pub size_bytes: Option<u64>,
    /// Ephemeral bearer URL produced for the selected account.
    pub direct_url: Option<String>,
    pub traffic_used_bytes: Option<u64>,
    pub traffic_total_bytes: Option<u64>,
}
