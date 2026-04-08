use sea_orm::entity::prelude::*;

use crate::domain::error::DomainError;
use crate::domain::model::download::{Download, DownloadId, DownloadState, FileSize, Url};
use crate::domain::model::queue::Priority;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "downloads")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i64,
    pub url: String,
    pub file_name: String,
    pub state: String,
    pub priority: i32,
    pub total_bytes: Option<i64>,
    pub downloaded_bytes: i64,
    pub speed_bytes_per_sec: i64,
    pub retry_count: i32,
    pub max_retries: i32,
    pub segments_count: i32,
    pub checksum_expected: Option<String>,
    pub source_hostname: String,
    pub protocol: String,
    pub resume_supported: i32,
    pub module_name: Option<String>,
    pub account_id: Option<i64>,
    pub destination_path: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::download_segment::Entity")]
    DownloadSegments,
}

impl Related<super::download_segment::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DownloadSegments.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    pub fn into_domain(self) -> Result<Download, DomainError> {
        let url = Url::new(&self.url)?;
        let state: DownloadState = self
            .state
            .parse()
            .map_err(|e: DomainError| DomainError::ValidationError(e.to_string()))?;
        let priority_val = u8::try_from(self.priority).map_err(|_| {
            DomainError::ValidationError(format!("invalid priority in DB: {}", self.priority))
        })?;
        let priority = Priority::new(priority_val)?;
        let file_size = self.total_bytes.map(|b| FileSize(b as u64));

        Ok(Download::reconstruct(
            DownloadId(self.id as u64),
            url,
            self.file_name,
            file_size,
            self.downloaded_bytes as u64,
            state,
            priority,
            self.retry_count as u32,
            self.max_retries as u32,
            self.segments_count as u32,
            self.checksum_expected,
            self.source_hostname,
            self.protocol,
            self.resume_supported != 0,
            self.module_name,
            self.account_id.map(|id| id as u64),
            self.destination_path,
            self.created_at as u64,
            self.updated_at as u64,
        ))
    }
}

impl ActiveModel {
    pub fn from_domain(download: &Download) -> Self {
        use sea_orm::ActiveValue::Set;

        Self {
            id: Set(download.id().0 as i64),
            url: Set(download.url().as_str().to_string()),
            file_name: Set(download.file_name().to_string()),
            state: Set(download.state().to_string()),
            priority: Set(download.priority().value() as i32),
            total_bytes: Set(download.file_size().map(|fs| fs.0 as i64)),
            downloaded_bytes: Set(download.downloaded_bytes() as i64),
            speed_bytes_per_sec: Set(0),
            retry_count: Set(download.retry_count() as i32),
            max_retries: Set(download.max_retries() as i32),
            segments_count: Set(download.segments_count() as i32),
            checksum_expected: Set(download.checksum_expected().map(|s| s.to_string())),
            source_hostname: Set(download.source_hostname().to_string()),
            protocol: Set(download.protocol().to_string()),
            resume_supported: Set(if download.resume_supported() { 1 } else { 0 }),
            module_name: Set(download.module_name().map(|s| s.to_string())),
            account_id: Set(download.account_id().map(|id| id as i64)),
            destination_path: Set(download.destination_path().to_string()),
            created_at: Set(download.created_at() as i64),
            updated_at: Set(download.updated_at() as i64),
        }
    }
}
