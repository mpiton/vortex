//! Handler for [`ListPackageDownloadsQuery`].
//!
//! Returns the [`DownloadView`] rows currently attached to the package,
//! ordered by `queue_position` ascending. Reuses the existing
//! `DownloadView` shape so the React layer can render member rows with
//! the same component as the main downloads list.

use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::domain::model::views::DownloadView;

impl QueryBus {
    pub async fn handle_list_package_downloads(
        &self,
        query: super::ListPackageDownloadsQuery,
    ) -> Result<Vec<DownloadView>, AppError> {
        let repo = self
            .package_read_repo()
            .ok_or_else(|| AppError::Validation("package read repository not configured".into()))?;
        Ok(repo.find_package_downloads(&query.id)?)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::application::error::AppError;
    use crate::application::queries::ListPackageDownloadsQuery;
    use crate::application::test_support::{
        InMemoryPackageReadRepo, query_bus_with_packages, sample_download_view,
    };
    use crate::domain::model::download::DownloadId;
    use crate::domain::model::package::PackageId;

    #[tokio::test]
    async fn test_list_package_downloads_returns_views_in_order_repo_provides() {
        let repo = Arc::new(InMemoryPackageReadRepo::new());
        repo.attach_downloads(
            "pkg",
            vec![
                sample_download_view(101, "first.zip", 1),
                sample_download_view(102, "second.zip", 2),
            ],
        );
        let bus = query_bus_with_packages(repo);

        let result = bus
            .handle_list_package_downloads(ListPackageDownloadsQuery {
                id: PackageId::new("pkg"),
            })
            .await
            .unwrap();
        let ids: Vec<u64> = result.iter().map(|v| v.id.0).collect();
        assert_eq!(ids, vec![101, 102]);
    }

    #[tokio::test]
    async fn test_list_package_downloads_returns_empty_when_no_members() {
        let repo = Arc::new(InMemoryPackageReadRepo::new());
        let bus = query_bus_with_packages(repo);
        let result = bus
            .handle_list_package_downloads(ListPackageDownloadsQuery {
                id: PackageId::new("none"),
            })
            .await
            .unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_list_package_downloads_does_not_leak_other_packages_members() {
        let repo = Arc::new(InMemoryPackageReadRepo::new());
        repo.attach_downloads("a", vec![sample_download_view(1, "a.zip", 0)]);
        repo.attach_downloads("b", vec![sample_download_view(2, "b.zip", 0)]);
        let bus = query_bus_with_packages(repo);

        let result = bus
            .handle_list_package_downloads(ListPackageDownloadsQuery {
                id: PackageId::new("a"),
            })
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, DownloadId(1));
    }

    #[tokio::test]
    async fn test_list_package_downloads_returns_validation_error_when_repo_missing() {
        let bus = crate::application::test_support::make_history_query_bus(Arc::new(
            crate::application::test_support::NoopHistoryRepo,
        ));
        let err = bus
            .handle_list_package_downloads(ListPackageDownloadsQuery {
                id: PackageId::new("pkg"),
            })
            .await
            .expect_err("missing repo");
        assert!(matches!(err, AppError::Validation(_)));
    }
}
