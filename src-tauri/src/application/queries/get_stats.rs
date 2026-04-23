use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::domain::model::views::StatsView;

impl QueryBus {
    pub async fn handle_get_stats(
        &self,
        query: super::GetStatsQuery,
    ) -> Result<StatsView, AppError> {
        let stats = self.stats_repo().get_stats(query.period)?;
        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::application::queries::GetStatsQuery;
    use crate::application::query_bus::QueryBus;
    use crate::application::test_support::query_bus_with_stats;
    use crate::domain::error::DomainError;
    use crate::domain::model::views::{
        DailyVolume, HostStats, ModuleStats, StatsPeriod, StatsView,
    };
    use crate::domain::ports::driven::StatsRepository;

    /// Records each period it was queried with so tests can assert
    /// the handler forwards the correct value.
    struct RecordingStatsRepo {
        last_period: Mutex<Option<StatsPeriod>>,
        view: StatsView,
    }

    impl RecordingStatsRepo {
        fn new(view: StatsView) -> Self {
            Self {
                last_period: Mutex::new(None),
                view,
            }
        }
    }

    impl StatsRepository for RecordingStatsRepo {
        fn record_completed(&self, _: u64, _: u64) -> Result<(), DomainError> {
            Ok(())
        }
        fn get_stats(&self, period: StatsPeriod) -> Result<StatsView, DomainError> {
            *self.last_period.lock().unwrap() = Some(period);
            Ok(self.view.clone())
        }
        fn top_modules(&self, _: u32) -> Result<Vec<ModuleStats>, DomainError> {
            Ok(vec![])
        }
    }

    fn sample_view() -> StatsView {
        StatsView {
            total_downloaded_bytes: 100,
            total_files: 3,
            avg_speed: 10,
            peak_speed: 20,
            success_rate: 0.5,
            daily_volumes: vec![DailyVolume {
                date: "2026-04-20".to_string(),
                bytes: 50,
                count: 1,
            }],
            top_hosts: vec![HostStats {
                hostname: "example.com".to_string(),
                total_bytes: 100,
                download_count: 3,
            }],
        }
    }

    #[tokio::test]
    async fn test_get_stats_forwards_period_to_repo() {
        let repo = Arc::new(RecordingStatsRepo::new(sample_view()));
        let bus: QueryBus = query_bus_with_stats(Arc::clone(&repo) as Arc<dyn StatsRepository>);

        let result = bus
            .handle_get_stats(GetStatsQuery {
                period: StatsPeriod::Last7Days,
            })
            .await
            .expect("handler ok");

        assert_eq!(result.total_files, 3);
        assert_eq!(
            *repo.last_period.lock().unwrap(),
            Some(StatsPeriod::Last7Days)
        );
    }

    #[tokio::test]
    async fn test_get_stats_all_time_default_period() {
        let repo = Arc::new(RecordingStatsRepo::new(sample_view()));
        let bus: QueryBus = query_bus_with_stats(Arc::clone(&repo) as Arc<dyn StatsRepository>);

        bus.handle_get_stats(GetStatsQuery {
            period: StatsPeriod::AllTime,
        })
        .await
        .expect("handler ok");

        assert_eq!(
            *repo.last_period.lock().unwrap(),
            Some(StatsPeriod::AllTime)
        );
    }
}
