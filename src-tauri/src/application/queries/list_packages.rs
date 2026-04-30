//! Handler for [`ListPackagesQuery`].
//!
//! Delegates to the [`PackageReadRepository`](crate::domain::ports::driven::PackageReadRepository)
//! which performs the `LEFT JOIN` aggregation in a single SQL round-trip.
//! Returned rows are sorted by `created_at` ascending then `id`
//! ascending so successive calls are deterministic.

use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::application::read_models::package_view::PackageViewDto;

impl QueryBus {
    pub async fn handle_list_packages(
        &self,
        query: super::ListPackagesQuery,
    ) -> Result<Vec<PackageViewDto>, AppError> {
        let repo = self
            .package_read_repo()
            .ok_or_else(|| AppError::Validation("package read repository not configured".into()))?;
        let views = repo.find_packages(query.filter)?;
        Ok(views.into_iter().map(PackageViewDto::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::application::error::AppError;
    use crate::application::queries::ListPackagesQuery;
    use crate::application::test_support::{
        InMemoryPackageReadRepo, query_bus_with_packages, sample_package_view,
    };
    use crate::domain::model::views::PackageFilter;

    #[tokio::test]
    async fn test_list_packages_returns_dtos_for_every_view() {
        let repo = Arc::new(InMemoryPackageReadRepo::new());
        repo.insert(sample_package_view("a", "Apple", "manual", 1));
        repo.insert(sample_package_view("b", "Banana", "playlist", 2));
        let bus = query_bus_with_packages(repo);

        let result = bus
            .handle_list_packages(ListPackagesQuery { filter: None })
            .await
            .unwrap();
        let ids: Vec<&str> = result.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
        assert_eq!(result[0].source_type, "manual");
        assert_eq!(result[1].source_type, "playlist");
    }

    #[tokio::test]
    async fn test_list_packages_forwards_filter_to_repo() {
        let repo = Arc::new(InMemoryPackageReadRepo::new());
        repo.insert(sample_package_view("a", "Holiday Mix", "playlist", 1));
        repo.insert(sample_package_view("b", "Misc", "manual", 2));
        let bus = query_bus_with_packages(repo);

        let result = bus
            .handle_list_packages(ListPackagesQuery {
                filter: Some(PackageFilter {
                    source_type: Some("playlist".to_string()),
                    name_q: None,
                }),
            })
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "a");
    }

    #[tokio::test]
    async fn test_list_packages_filter_name_q_is_case_insensitive_substring() {
        let repo = Arc::new(InMemoryPackageReadRepo::new());
        repo.insert(sample_package_view("1", "Holiday Photos", "manual", 1));
        repo.insert(sample_package_view("2", "Music — Holidays", "manual", 2));
        repo.insert(sample_package_view("3", "Misc", "manual", 3));
        let bus = query_bus_with_packages(repo);

        let result = bus
            .handle_list_packages(ListPackagesQuery {
                filter: Some(PackageFilter {
                    source_type: None,
                    name_q: Some("HOLIDAY".to_string()),
                }),
            })
            .await
            .unwrap();
        let ids: Vec<&str> = result.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["1", "2"]);
    }

    #[tokio::test]
    async fn test_list_packages_returns_validation_error_when_repo_missing() {
        let bus = crate::application::test_support::make_history_query_bus(Arc::new(
            crate::application::test_support::NoopHistoryRepo,
        ));
        let err = bus
            .handle_list_packages(ListPackagesQuery { filter: None })
            .await
            .expect_err("missing repo");
        assert!(matches!(err, AppError::Validation(msg) if msg.contains("package")));
    }
}
