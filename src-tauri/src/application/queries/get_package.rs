//! Handler for [`GetPackageQuery`].
//!
//! Returns a single package as a [`PackageViewDto`] or
//! [`AppError::NotFound`] when no row matches the requested id.

use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::application::read_models::package_view::PackageViewDto;

impl QueryBus {
    pub async fn handle_get_package(
        &self,
        query: super::GetPackageQuery,
    ) -> Result<PackageViewDto, AppError> {
        let repo = self
            .package_read_repo()
            .ok_or_else(|| AppError::Validation("package read repository not configured".into()))?;
        let view = repo
            .find_package_by_id(&query.id)?
            .ok_or_else(|| AppError::NotFound(format!("package {}", query.id.as_str())))?;
        Ok(view.into())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::application::error::AppError;
    use crate::application::queries::GetPackageQuery;
    use crate::application::test_support::{
        InMemoryPackageReadRepo, query_bus_with_packages, sample_package_view,
    };
    use crate::domain::model::package::PackageId;

    #[tokio::test]
    async fn test_get_package_returns_dto_when_found() {
        let repo = Arc::new(InMemoryPackageReadRepo::new());
        repo.insert(sample_package_view("p-1", "One", "manual", 5));
        let bus = query_bus_with_packages(repo);

        let dto = bus
            .handle_get_package(GetPackageQuery {
                id: PackageId::new("p-1"),
            })
            .await
            .unwrap();
        assert_eq!(dto.id, "p-1");
        assert_eq!(dto.name, "One");
        assert_eq!(dto.source_type, "manual");
    }

    #[tokio::test]
    async fn test_get_package_returns_not_found_when_missing() {
        let repo = Arc::new(InMemoryPackageReadRepo::new());
        let bus = query_bus_with_packages(repo);
        let err = bus
            .handle_get_package(GetPackageQuery {
                id: PackageId::new("ghost"),
            })
            .await
            .expect_err("ghost id");
        assert!(matches!(err, AppError::NotFound(msg) if msg.contains("ghost")));
    }

    #[tokio::test]
    async fn test_get_package_returns_validation_error_when_repo_missing() {
        let bus = crate::application::test_support::make_history_query_bus(Arc::new(
            crate::application::test_support::NoopHistoryRepo,
        ));
        let err = bus
            .handle_get_package(GetPackageQuery {
                id: PackageId::new("p-1"),
            })
            .await
            .expect_err("missing repo");
        assert!(matches!(err, AppError::Validation(_)));
    }
}
