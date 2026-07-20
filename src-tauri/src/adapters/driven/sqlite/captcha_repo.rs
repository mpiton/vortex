use sea_orm::{
    ColumnTrait, DatabaseBackend, DatabaseConnection, EntityTrait, FromQueryResult, QueryFilter,
    QueryOrder, Statement, sea_query::OnConflict,
};

use crate::domain::error::DomainError;
use crate::domain::model::captcha::{CaptchaChallenge, CaptchaId, CaptchaStatus};
use crate::domain::model::download::DownloadId;
use crate::domain::ports::driven::CaptchaRepository;

use super::entities::captcha_log;
use super::util::{block_on, map_db_err};

const CAPTCHA_METADATA_QUERY: &str = "SELECT id, download_id, challenge_type, \
    '[redacted]' AS challenge_url, \
    NULL AS image_data, status, solver, attempts, solver_attempts_json, created_at, expires_at, resolved_at, \
    duration_ms, failure_reason FROM captcha_log ORDER BY created_at DESC LIMIT 200";
const PENDING_CAPTCHA_METADATA_QUERY: &str = "SELECT id, download_id, challenge_type, \
    '[redacted]' AS challenge_url, NULL AS image_data, status, solver, attempts, solver_attempts_json, created_at, expires_at, \
    resolved_at, duration_ms, failure_reason FROM captcha_log WHERE status = ? \
    ORDER BY created_at ASC";

pub struct SqliteCaptchaRepo {
    db: DatabaseConnection,
}

impl SqliteCaptchaRepo {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

impl CaptchaRepository for SqliteCaptchaRepo {
    fn save(&self, challenge: &CaptchaChallenge) -> Result<(), DomainError> {
        let active = captcha_log::ActiveModel::from_domain(challenge)?;
        block_on(async {
            captcha_log::Entity::insert(active)
                .on_conflict(
                    OnConflict::column(captcha_log::Column::Id)
                        .update_columns([
                            captcha_log::Column::DownloadId,
                            captcha_log::Column::ChallengeType,
                            captcha_log::Column::ChallengeUrl,
                            captcha_log::Column::ImageData,
                            captcha_log::Column::Status,
                            captcha_log::Column::Solver,
                            captcha_log::Column::Attempts,
                            captcha_log::Column::SolverAttemptsJson,
                            captcha_log::Column::ExpiresAt,
                            captcha_log::Column::ResolvedAt,
                            captcha_log::Column::DurationMs,
                            captcha_log::Column::FailureReason,
                        ])
                        .to_owned(),
                )
                .exec(&self.db)
                .await
                .map_err(map_db_err)?;
            Ok(())
        })
    }

    fn find_by_id(&self, id: &CaptchaId) -> Result<Option<CaptchaChallenge>, DomainError> {
        let id = id.to_string();
        block_on(async {
            captcha_log::Entity::find_by_id(id)
                .one(&self.db)
                .await
                .map_err(map_db_err)?
                .map(captcha_log::Model::into_domain)
                .transpose()
        })
    }

    fn list(&self) -> Result<Vec<CaptchaChallenge>, DomainError> {
        block_on(async {
            let models = captcha_log::Model::find_by_statement(Statement::from_string(
                DatabaseBackend::Sqlite,
                CAPTCHA_METADATA_QUERY.to_string(),
            ))
            .all(&self.db)
            .await
            .map_err(map_db_err)?;
            models
                .into_iter()
                .map(captcha_log::Model::into_domain)
                .collect()
        })
    }

    fn list_pending(&self) -> Result<Vec<CaptchaChallenge>, DomainError> {
        block_on(async {
            let models = captcha_log::Model::find_by_statement(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                PENDING_CAPTCHA_METADATA_QUERY,
                [CaptchaStatus::Pending.to_string().into()],
            ))
            .all(&self.db)
            .await
            .map_err(map_db_err)?;
            models
                .into_iter()
                .map(captcha_log::Model::into_domain)
                .collect()
        })
    }

    fn find_next_pending(&self) -> Result<Option<CaptchaChallenge>, DomainError> {
        block_on(async {
            captcha_log::Entity::find()
                .filter(captcha_log::Column::Status.eq(CaptchaStatus::Pending.to_string()))
                .order_by_asc(captcha_log::Column::CreatedAt)
                .one(&self.db)
                .await
                .map_err(map_db_err)?
                .map(captcha_log::Model::into_domain)
                .transpose()
        })
    }

    fn find_pending_by_download(
        &self,
        download_id: DownloadId,
    ) -> Result<Option<CaptchaChallenge>, DomainError> {
        let download_id = i64::try_from(download_id.0).map_err(|_| {
            DomainError::ValidationError("CAPTCHA download id exceeds i64::MAX".into())
        })?;
        block_on(async {
            captcha_log::Entity::find()
                .filter(captcha_log::Column::DownloadId.eq(download_id))
                .filter(captcha_log::Column::Status.eq(CaptchaStatus::Pending.to_string()))
                .order_by_asc(captcha_log::Column::CreatedAt)
                .one(&self.db)
                .await
                .map_err(map_db_err)?
                .map(captcha_log::Model::into_domain)
                .transpose()
        })
    }
}
