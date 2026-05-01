//! Minimal package summary DTO for look-up results.
//!
//! Used by [`crate::application::queries::FindPackageByExternalIdQuery`] to
//! return just enough data for the caller to identify the package without
//! pulling the full aggregated view.

use serde::Serialize;

use crate::domain::model::views::PackageView;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageSummaryDto {
    pub package_id: String,
    pub package_name: String,
}

impl From<&PackageView> for PackageSummaryDto {
    fn from(view: &PackageView) -> Self {
        Self {
            package_id: view.id.clone(),
            package_name: view.name.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::views::PackageView;

    fn make_view(id: &str, name: &str) -> PackageView {
        PackageView {
            id: id.to_string(),
            name: name.to_string(),
            source_type: "playlist".to_string(),
            folder_path: None,
            auto_extract: true,
            priority: 5,
            created_at: 1_000,
            downloads_count: 0,
            total_bytes: 0,
            downloaded_bytes: 0,
            progress_percent: 0.0,
            all_completed: false,
        }
    }

    #[test]
    fn test_package_summary_dto_from_view_copies_id_and_name() {
        let view = make_view("pkg-1", "My Playlist");
        let dto = PackageSummaryDto::from(&view);
        assert_eq!(dto.package_id, "pkg-1");
        assert_eq!(dto.package_name, "My Playlist");
    }

    #[test]
    fn test_package_summary_dto_serializes_to_camel_case() {
        let view = make_view("pkg-42", "Test Package");
        let dto = PackageSummaryDto::from(&view);
        let value = serde_json::to_value(&dto).unwrap();
        let obj = value.as_object().expect("object");
        assert!(
            obj.contains_key("packageId"),
            "camelCase field `packageId` missing"
        );
        assert!(
            obj.contains_key("packageName"),
            "camelCase field `packageName` missing"
        );
        assert_eq!(obj["packageId"], "pkg-42");
        assert_eq!(obj["packageName"], "Test Package");
    }
}
