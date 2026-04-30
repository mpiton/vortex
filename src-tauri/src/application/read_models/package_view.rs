//! Serializable package view DTOs for the frontend.
//!
//! Mirrors [`PackageView`] field-by-field with `camelCase` JSON keys for
//! the React layer. The DTO never carries the keyring password — the
//! write-side `Package` aggregate keeps that in `password`, but the read
//! view intentionally omits it so query results never leak credential
//! references.

use serde::Serialize;

use crate::domain::model::views::PackageView;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageViewDto {
    pub id: String,
    pub name: String,
    pub source_type: String,
    pub folder_path: Option<String>,
    pub auto_extract: bool,
    pub priority: u8,
    pub created_at: u64,
    pub downloads_count: u64,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub progress_percent: f64,
    pub all_completed: bool,
}

impl From<PackageView> for PackageViewDto {
    fn from(view: PackageView) -> Self {
        Self {
            id: view.id,
            name: view.name,
            source_type: view.source_type,
            folder_path: view.folder_path,
            auto_extract: view.auto_extract,
            priority: view.priority,
            created_at: view.created_at,
            downloads_count: view.downloads_count,
            total_bytes: view.total_bytes,
            downloaded_bytes: view.downloaded_bytes,
            progress_percent: view.progress_percent,
            all_completed: view.all_completed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_view() -> PackageView {
        PackageView {
            id: "pkg-7".to_string(),
            name: "Holiday".to_string(),
            source_type: "playlist".to_string(),
            folder_path: Some("/srv/dl".to_string()),
            auto_extract: false,
            priority: 8,
            created_at: 1_700_000_000_000,
            downloads_count: 4,
            total_bytes: 12_000,
            downloaded_bytes: 6_000,
            progress_percent: 50.0,
            all_completed: false,
        }
    }

    #[test]
    fn test_package_view_dto_from_view_copies_every_field() {
        let dto: PackageViewDto = make_view().into();
        assert_eq!(dto.id, "pkg-7");
        assert_eq!(dto.name, "Holiday");
        assert_eq!(dto.source_type, "playlist");
        assert_eq!(dto.folder_path.as_deref(), Some("/srv/dl"));
        assert!(!dto.auto_extract);
        assert_eq!(dto.priority, 8);
        assert_eq!(dto.created_at, 1_700_000_000_000);
        assert_eq!(dto.downloads_count, 4);
        assert_eq!(dto.total_bytes, 12_000);
        assert_eq!(dto.downloaded_bytes, 6_000);
        assert!((dto.progress_percent - 50.0).abs() < 1e-9);
        assert!(!dto.all_completed);
    }

    #[test]
    fn test_package_view_dto_serializes_to_camel_case() {
        let dto: PackageViewDto = make_view().into();
        let value = serde_json::to_value(&dto).unwrap();
        let object = value.as_object().expect("object");
        for camel_field in [
            "id",
            "name",
            "sourceType",
            "folderPath",
            "autoExtract",
            "priority",
            "createdAt",
            "downloadsCount",
            "totalBytes",
            "downloadedBytes",
            "progressPercent",
            "allCompleted",
        ] {
            assert!(
                object.contains_key(camel_field),
                "camelCase field `{camel_field}` missing"
            );
        }
    }

    #[test]
    fn test_package_view_dto_omits_password_field() {
        let dto: PackageViewDto = make_view().into();
        let value = serde_json::to_value(&dto).unwrap();
        let object = value.as_object().expect("object");
        assert!(
            !object.contains_key("password"),
            "PackageViewDto must never expose a password field"
        );
    }
}
