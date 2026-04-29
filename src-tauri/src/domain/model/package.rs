use std::fmt;
use std::str::FromStr;

use crate::domain::error::DomainError;
use crate::domain::model::download::DownloadId;

/// Identifier of a `Package` aggregate. Stored as `TEXT` in SQLite — the
/// caller picks the format (UUID, slug…). The wrapper makes the type
/// distinct from a plain `String` and from other `*Id` types.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageId(pub String);

impl PackageId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Origin of a `Package`. Persisted as a lower-snake-case string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageSourceType {
    /// Container file imported from disk (DLC, CCF, RSDF, Metalink…).
    Container,
    /// Auto-grouped playlist extracted by a crawler plugin.
    Playlist,
    /// User-built package (manual grouping).
    Manual,
    /// Multi-part archive auto-grouped by file naming convention.
    SplitArchive,
}

impl fmt::Display for PackageSourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            PackageSourceType::Container => "container",
            PackageSourceType::Playlist => "playlist",
            PackageSourceType::Manual => "manual",
            PackageSourceType::SplitArchive => "split-archive",
        };
        f.write_str(s)
    }
}

impl FromStr for PackageSourceType {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "container" => Ok(PackageSourceType::Container),
            "playlist" => Ok(PackageSourceType::Playlist),
            "manual" => Ok(PackageSourceType::Manual),
            "split-archive" => Ok(PackageSourceType::SplitArchive),
            other => Err(DomainError::ValidationError(format!(
                "invalid package source type: {other}"
            ))),
        }
    }
}

/// Default scheduling priority for a package (1..=10 scale, mid-range).
pub const DEFAULT_PACKAGE_PRIORITY: u8 = 5;

#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    id: PackageId,
    name: String,
    source_type: PackageSourceType,
    folder_path: Option<String>,
    /// Reference to the keyring entry holding the archive password, or
    /// `None` when the package has no password. The repo persists the
    /// raw string verbatim — the keyring lookup happens elsewhere.
    password: Option<String>,
    auto_extract: bool,
    priority: u8,
    created_at: u64,
    /// In-memory aggregate children. Persistence stores the inverse FK
    /// on `downloads.package_id` — never a column on `packages` itself.
    download_ids: Vec<DownloadId>,
}

impl Package {
    pub fn new(
        id: PackageId,
        name: String,
        source_type: PackageSourceType,
        created_at: u64,
    ) -> Self {
        Self {
            id,
            name,
            source_type,
            folder_path: None,
            password: None,
            auto_extract: true,
            priority: DEFAULT_PACKAGE_PRIORITY,
            created_at,
            download_ids: Vec::new(),
        }
    }

    /// Rebuild a package from persisted state without children. Used by
    /// the SQLite adapter; the children list is repopulated separately
    /// via `PackageRepository::list_downloads`.
    #[allow(clippy::too_many_arguments)]
    pub fn reconstruct(
        id: PackageId,
        name: String,
        source_type: PackageSourceType,
        folder_path: Option<String>,
        password: Option<String>,
        auto_extract: bool,
        priority: u8,
        created_at: u64,
    ) -> Self {
        Self {
            id,
            name,
            source_type,
            folder_path,
            password,
            auto_extract,
            priority,
            created_at,
            download_ids: Vec::new(),
        }
    }

    pub fn set_folder_path(&mut self, path: Option<String>) {
        self.folder_path = path;
    }

    pub fn set_password(&mut self, password: Option<String>) {
        self.password = password;
    }

    pub fn set_auto_extract(&mut self, enabled: bool) {
        self.auto_extract = enabled;
    }

    pub fn set_priority(&mut self, priority: u8) {
        self.priority = priority;
    }

    pub fn add_download(&mut self, id: DownloadId) {
        if !self.download_ids.contains(&id) {
            self.download_ids.push(id);
        }
    }

    pub fn remove_download(&mut self, id: DownloadId) {
        self.download_ids.retain(|d| d != &id);
    }

    pub fn download_count(&self) -> usize {
        self.download_ids.len()
    }

    pub fn contains_download(&self, id: DownloadId) -> bool {
        self.download_ids.contains(&id)
    }

    pub fn downloads(&self) -> &[DownloadId] {
        &self.download_ids
    }

    pub fn id(&self) -> &PackageId {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn source_type(&self) -> PackageSourceType {
        self.source_type
    }

    pub fn folder_path(&self) -> Option<&str> {
        self.folder_path.as_deref()
    }

    pub fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    pub fn auto_extract(&self) -> bool {
        self.auto_extract
    }

    pub fn priority(&self) -> u8 {
        self.priority
    }

    pub fn created_at(&self) -> u64 {
        self.created_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_package() -> Package {
        Package::new(
            PackageId::new("pkg-1"),
            "My Package".to_string(),
            PackageSourceType::Manual,
            1_700_000_000_000,
        )
    }

    #[test]
    fn test_package_new_initialises_defaults() {
        let p = make_package();
        assert_eq!(p.id().as_str(), "pkg-1");
        assert_eq!(p.name(), "My Package");
        assert_eq!(p.source_type(), PackageSourceType::Manual);
        assert!(p.folder_path().is_none());
        assert!(p.password().is_none());
        assert!(p.auto_extract());
        assert_eq!(p.priority(), DEFAULT_PACKAGE_PRIORITY);
        assert_eq!(p.created_at(), 1_700_000_000_000);
        assert_eq!(p.download_count(), 0);
        assert!(p.downloads().is_empty());
    }

    #[test]
    fn test_package_default_priority_is_five() {
        assert_eq!(DEFAULT_PACKAGE_PRIORITY, 5);
        let p = make_package();
        assert_eq!(p.priority(), 5);
    }

    #[test]
    fn test_package_setters_store_optional_fields() {
        let mut p = make_package();
        p.set_folder_path(Some("/tmp/dl".to_string()));
        p.set_password(Some("keyring://pkg/secret".to_string()));
        p.set_auto_extract(false);
        p.set_priority(9);
        assert_eq!(p.folder_path(), Some("/tmp/dl"));
        assert_eq!(p.password(), Some("keyring://pkg/secret"));
        assert!(!p.auto_extract());
        assert_eq!(p.priority(), 9);
    }

    #[test]
    fn test_package_setters_clear_optional_fields() {
        let mut p = make_package();
        p.set_folder_path(Some("/x".to_string()));
        p.set_password(Some("k".to_string()));
        p.set_folder_path(None);
        p.set_password(None);
        assert!(p.folder_path().is_none());
        assert!(p.password().is_none());
    }

    #[test]
    fn test_package_add_download() {
        let mut p = make_package();
        p.add_download(DownloadId(10));
        assert_eq!(p.download_count(), 1);
        assert!(p.contains_download(DownloadId(10)));
    }

    #[test]
    fn test_package_add_download_duplicate_ignored() {
        let mut p = make_package();
        p.add_download(DownloadId(10));
        p.add_download(DownloadId(10));
        assert_eq!(p.download_count(), 1);
    }

    #[test]
    fn test_package_remove_download() {
        let mut p = make_package();
        p.add_download(DownloadId(10));
        p.add_download(DownloadId(20));
        p.remove_download(DownloadId(10));
        assert_eq!(p.download_count(), 1);
        assert!(!p.contains_download(DownloadId(10)));
        assert!(p.contains_download(DownloadId(20)));
    }

    #[test]
    fn test_package_remove_nonexistent_is_noop() {
        let mut p = make_package();
        p.remove_download(DownloadId(99));
        assert_eq!(p.download_count(), 0);
    }

    #[test]
    fn test_package_download_count_grows_with_each_unique_id() {
        let mut p = make_package();
        assert_eq!(p.download_count(), 0);
        p.add_download(DownloadId(1));
        p.add_download(DownloadId(2));
        p.add_download(DownloadId(3));
        assert_eq!(p.download_count(), 3);
    }

    #[test]
    fn test_package_contains_reflects_membership() {
        let mut p = make_package();
        assert!(!p.contains_download(DownloadId(5)));
        p.add_download(DownloadId(5));
        assert!(p.contains_download(DownloadId(5)));
    }

    #[test]
    fn test_package_reconstruct_preserves_persisted_fields() {
        let p = Package::reconstruct(
            PackageId::new("pkg-r"),
            "Reloaded".to_string(),
            PackageSourceType::Container,
            Some("/srv/dl".to_string()),
            Some("keyring://srv/secret".to_string()),
            false,
            7,
            1_700_000_000_001,
        );
        assert_eq!(p.id().as_str(), "pkg-r");
        assert_eq!(p.name(), "Reloaded");
        assert_eq!(p.source_type(), PackageSourceType::Container);
        assert_eq!(p.folder_path(), Some("/srv/dl"));
        assert_eq!(p.password(), Some("keyring://srv/secret"));
        assert!(!p.auto_extract());
        assert_eq!(p.priority(), 7);
        assert_eq!(p.created_at(), 1_700_000_000_001);
        assert!(p.downloads().is_empty());
    }

    #[test]
    fn test_package_id_display_returns_inner_value() {
        assert_eq!(PackageId::new("abc-42").to_string(), "abc-42");
        assert_eq!(PackageId::new("abc-42").as_str(), "abc-42");
    }

    #[test]
    fn test_package_source_type_round_trip_via_string() {
        for variant in [
            PackageSourceType::Container,
            PackageSourceType::Playlist,
            PackageSourceType::Manual,
            PackageSourceType::SplitArchive,
        ] {
            let s = variant.to_string();
            let parsed: PackageSourceType = s.parse().expect("round trip");
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn test_package_source_type_from_str_rejects_unknown() {
        let result: Result<PackageSourceType, _> = "garbage".parse();
        assert!(matches!(result, Err(DomainError::ValidationError(_))));
    }
}
