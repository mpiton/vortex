use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::domain::model::views::ModuleStats;

impl QueryBus {
    pub async fn handle_top_modules(
        &self,
        query: super::TopModulesQuery,
    ) -> Result<Vec<ModuleStats>, AppError> {
        let modules = self.stats_repo().top_modules(query.limit)?;
        Ok(modules)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::application::queries::TopModulesQuery;
    use crate::application::query_bus::QueryBus;
    use crate::application::test_support::query_bus_with_stats;
    use crate::domain::error::DomainError;
    use crate::domain::model::views::{ModuleStats, StatsPeriod, StatsView};
    use crate::domain::ports::driven::StatsRepository;

    struct RecordingStatsRepo {
        last_limit: Mutex<Option<u32>>,
        modules: Vec<ModuleStats>,
    }

    impl RecordingStatsRepo {
        fn new(modules: Vec<ModuleStats>) -> Self {
            Self {
                last_limit: Mutex::new(None),
                modules,
            }
        }
    }

    impl StatsRepository for RecordingStatsRepo {
        fn record_completed(&self, _: u64, _: u64) -> Result<(), DomainError> {
            Ok(())
        }
        fn get_stats(&self, _: StatsPeriod) -> Result<StatsView, DomainError> {
            Ok(StatsView {
                total_downloaded_bytes: 0,
                total_files: 0,
                avg_speed: 0,
                peak_speed: 0,
                success_rate: 0.0,
                daily_volumes: vec![],
                top_hosts: vec![],
            })
        }
        fn top_modules(&self, limit: u32) -> Result<Vec<ModuleStats>, DomainError> {
            *self.last_limit.lock().unwrap() = Some(limit);
            Ok(self.modules.clone())
        }
    }

    #[tokio::test]
    async fn test_top_modules_forwards_limit() {
        let repo = Arc::new(RecordingStatsRepo::new(vec![ModuleStats {
            module_name: "vortex-mod-youtube".to_string(),
            download_count: 5,
            total_bytes: 500,
        }]));
        let bus: QueryBus = query_bus_with_stats(Arc::clone(&repo) as Arc<dyn StatsRepository>);

        let result = bus
            .handle_top_modules(TopModulesQuery { limit: 5 })
            .await
            .expect("handler ok");

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].module_name, "vortex-mod-youtube");
        assert_eq!(*repo.last_limit.lock().unwrap(), Some(5));
    }

    #[tokio::test]
    async fn test_top_modules_empty() {
        let repo = Arc::new(RecordingStatsRepo::new(vec![]));
        let bus: QueryBus = query_bus_with_stats(Arc::clone(&repo) as Arc<dyn StatsRepository>);

        let result = bus
            .handle_top_modules(TopModulesQuery { limit: 10 })
            .await
            .expect("handler ok");

        assert!(result.is_empty());
    }
}
