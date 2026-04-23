//! Serializable statistics view DTOs for the frontend.

use serde::Serialize;

use crate::domain::model::views::{DailyVolume, HostStats, ModuleStats, StatsView};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyVolumeDto {
    pub date: String,
    pub bytes: u64,
    pub count: u64,
}

impl From<DailyVolume> for DailyVolumeDto {
    fn from(v: DailyVolume) -> Self {
        Self {
            date: v.date,
            bytes: v.bytes,
            count: v.count,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostStatsDto {
    pub hostname: String,
    pub total_bytes: u64,
    pub download_count: u64,
}

impl From<HostStats> for HostStatsDto {
    fn from(h: HostStats) -> Self {
        Self {
            hostname: h.hostname,
            total_bytes: h.total_bytes,
            download_count: h.download_count,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsViewDto {
    pub total_downloaded_bytes: u64,
    pub total_files: u64,
    pub avg_speed: u64,
    pub peak_speed: u64,
    pub success_rate: f64,
    pub daily_volumes: Vec<DailyVolumeDto>,
    pub top_hosts: Vec<HostStatsDto>,
}

impl From<StatsView> for StatsViewDto {
    fn from(s: StatsView) -> Self {
        Self {
            total_downloaded_bytes: s.total_downloaded_bytes,
            total_files: s.total_files,
            avg_speed: s.avg_speed,
            peak_speed: s.peak_speed,
            success_rate: s.success_rate,
            daily_volumes: s
                .daily_volumes
                .into_iter()
                .map(DailyVolumeDto::from)
                .collect(),
            top_hosts: s.top_hosts.into_iter().map(HostStatsDto::from).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleStatsDto {
    pub module_name: String,
    pub download_count: u64,
    pub total_bytes: u64,
}

impl From<ModuleStats> for ModuleStatsDto {
    fn from(m: ModuleStats) -> Self {
        Self {
            module_name: m.module_name,
            download_count: m.download_count,
            total_bytes: m.total_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::views::{DailyVolume, HostStats, StatsView};

    #[test]
    fn test_stats_view_dto_from_domain() {
        let stats = StatsView {
            total_downloaded_bytes: 1_000_000,
            total_files: 10,
            avg_speed: 500,
            peak_speed: 2000,
            success_rate: 0.95,
            daily_volumes: vec![DailyVolume {
                date: "2024-01-01".to_string(),
                bytes: 100_000,
                count: 5,
            }],
            top_hosts: vec![HostStats {
                hostname: "example.com".to_string(),
                total_bytes: 500_000,
                download_count: 7,
            }],
        };
        let dto = StatsViewDto::from(stats);
        assert_eq!(dto.total_downloaded_bytes, 1_000_000);
        assert_eq!(dto.total_files, 10);
        assert_eq!(dto.daily_volumes.len(), 1);
        assert_eq!(dto.daily_volumes[0].date, "2024-01-01");
        assert_eq!(dto.top_hosts.len(), 1);
        assert_eq!(dto.top_hosts[0].hostname, "example.com");
    }

    #[test]
    fn test_stats_view_dto_serializes_to_camel_case() {
        let dto = StatsViewDto {
            total_downloaded_bytes: 0,
            total_files: 0,
            avg_speed: 0,
            peak_speed: 0,
            success_rate: 1.0,
            daily_volumes: vec![],
            top_hosts: vec![],
        };
        let value = serde_json::to_value(&dto).unwrap();
        assert!(value.get("totalDownloadedBytes").is_some());
        assert!(value.get("totalFiles").is_some());
        assert!(value.get("avgSpeed").is_some());
        assert!(value.get("peakSpeed").is_some());
        assert!(value.get("successRate").is_some());
        assert!(value.get("dailyVolumes").is_some());
        assert!(value.get("topHosts").is_some());
    }

    #[test]
    fn test_module_stats_dto_from_domain() {
        let domain = ModuleStats {
            module_name: "vortex-mod-youtube".to_string(),
            download_count: 42,
            total_bytes: 123_456,
        };
        let dto = ModuleStatsDto::from(domain);
        assert_eq!(dto.module_name, "vortex-mod-youtube");
        assert_eq!(dto.download_count, 42);
        assert_eq!(dto.total_bytes, 123_456);
    }

    #[test]
    fn test_module_stats_dto_serializes_to_camel_case() {
        let dto = ModuleStatsDto {
            module_name: "vortex-mod-youtube".to_string(),
            download_count: 1,
            total_bytes: 1,
        };
        let value = serde_json::to_value(&dto).unwrap();
        assert!(value.get("moduleName").is_some());
        assert!(value.get("downloadCount").is_some());
        assert!(value.get("totalBytes").is_some());
    }
}
