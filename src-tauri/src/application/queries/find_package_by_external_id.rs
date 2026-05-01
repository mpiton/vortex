//! Handler for [`FindPackageByExternalIdQuery`].
//!
//! Delegates to [`PackageReadRepository`](crate::domain::ports::driven::PackageReadRepository)
//! which performs a direct index look-up on `packages.external_id`.
//! Returns `None` when no package has been registered for the given key.

use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::application::read_models::PackageSummaryDto;

impl QueryBus {
    pub async fn handle_find_package_by_external_id(
        &self,
        query: super::FindPackageByExternalIdQuery,
    ) -> Result<Option<PackageSummaryDto>, AppError> {
        let repo = self
            .package_read_repo()
            .ok_or_else(|| AppError::Validation("package read repository not configured".into()))?;
        let view = repo.find_package_by_external_id(&query.external_id)?;
        Ok(view.as_ref().map(PackageSummaryDto::from))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::application::queries::FindPackageByExternalIdQuery;
    use crate::application::test_support::{
        InMemoryPackageReadRepo, query_bus_with_packages, sample_package_view,
    };

    #[tokio::test]
    async fn test_find_package_by_external_id_returns_summary_when_match() {
        let repo = Arc::new(InMemoryPackageReadRepo::new());
        repo.insert_with_external_id(
            sample_package_view("yt-1", "YouTube Mix", "playlist", 1),
            "youtube:playlist:abc",
        );
        let bus = query_bus_with_packages(repo);

        let result = bus
            .handle_find_package_by_external_id(FindPackageByExternalIdQuery {
                external_id: "youtube:playlist:abc".to_string(),
            })
            .await
            .unwrap();

        let summary = result.expect("should return Some when external_id matches");
        assert_eq!(summary.package_id, "yt-1");
        assert_eq!(summary.package_name, "YouTube Mix");
    }

    #[tokio::test]
    async fn test_find_package_by_external_id_returns_none_when_no_match() {
        let repo = Arc::new(InMemoryPackageReadRepo::new());
        let bus = query_bus_with_packages(repo);

        let result = bus
            .handle_find_package_by_external_id(FindPackageByExternalIdQuery {
                external_id: "youtube:playlist:missing".to_string(),
            })
            .await
            .unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_find_package_by_external_id_uses_exact_match() {
        let repo = Arc::new(InMemoryPackageReadRepo::new());
        repo.insert_with_external_id(
            sample_package_view("a", "Alpha", "playlist", 1),
            "yt:playlist:aaa",
        );
        repo.insert_with_external_id(
            sample_package_view("b", "Beta", "playlist", 2),
            "yt:playlist:bbb",
        );
        repo.insert(sample_package_view("c", "Gamma", "manual", 3));
        let bus = query_bus_with_packages(repo);

        let result = bus
            .handle_find_package_by_external_id(FindPackageByExternalIdQuery {
                external_id: "yt:playlist:bbb".to_string(),
            })
            .await
            .unwrap();

        let summary = result.expect("should return Some for exact key bbb");
        assert_eq!(summary.package_id, "b");
        assert_eq!(summary.package_name, "Beta");
    }
}
