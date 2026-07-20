use sea_orm::entity::prelude::*;

use crate::domain::error::DomainError;
use crate::domain::model::captcha::{
    CaptchaChallenge, CaptchaChallengeRecord, CaptchaId, CaptchaSolverAttempt,
    CaptchaSolverAttemptOutcome, CaptchaStatus, CaptchaType,
};
use crate::domain::model::download::DownloadId;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "captcha_log")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub download_id: i64,
    pub challenge_type: String,
    pub challenge_url: String,
    pub image_data: Option<Vec<u8>>,
    pub status: String,
    pub solver: Option<String>,
    pub attempts: i32,
    pub solver_attempts_json: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub resolved_at: Option<i64>,
    pub duration_ms: Option<i64>,
    pub failure_reason: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    pub fn into_domain(self) -> Result<CaptchaChallenge, DomainError> {
        CaptchaChallenge::reconstruct(CaptchaChallengeRecord {
            id: CaptchaId::new(self.id),
            download_id: DownloadId(to_u64(self.download_id, "download_id")?),
            challenge_type: self.challenge_type.parse::<CaptchaType>()?,
            url: self.challenge_url,
            image_data: self.image_data,
            status: self.status.parse::<CaptchaStatus>()?,
            solver: self.solver,
            attempts: u32::try_from(self.attempts).map_err(|_| invalid_integer("attempts"))?,
            solver_attempts: decode_solver_attempts(&self.solver_attempts_json)?,
            created_at: to_u64(self.created_at, "created_at")?,
            expires_at: to_u64(self.expires_at, "expires_at")?,
            resolved_at: self
                .resolved_at
                .map(|value| to_u64(value, "resolved_at"))
                .transpose()?,
            duration_ms: self
                .duration_ms
                .map(|value| to_u64(value, "duration_ms"))
                .transpose()?,
            failure_reason: self.failure_reason,
        })
    }
}

impl ActiveModel {
    pub fn from_domain(challenge: &CaptchaChallenge) -> Result<Self, DomainError> {
        use sea_orm::ActiveValue::Set;

        Ok(Self {
            id: Set(challenge.id().to_string()),
            download_id: Set(to_i64(challenge.download_id().0, "download id")?),
            challenge_type: Set(challenge.challenge_type().to_string()),
            challenge_url: Set(challenge.url().to_string()),
            image_data: Set(challenge.image_data().map(<[u8]>::to_vec)),
            status: Set(challenge.status().to_string()),
            solver: Set(challenge.solver().map(str::to_string)),
            attempts: Set(
                i32::try_from(challenge.attempts()).map_err(|_| invalid_integer("attempts"))?
            ),
            solver_attempts_json: Set(encode_solver_attempts(challenge.solver_attempts())?),
            created_at: Set(to_i64(challenge.created_at(), "created_at")?),
            expires_at: Set(to_i64(challenge.expires_at(), "expires_at")?),
            resolved_at: Set(challenge
                .resolved_at()
                .map(|value| to_i64(value, "resolved_at"))
                .transpose()?),
            duration_ms: Set(challenge
                .duration_ms()
                .map(|value| to_i64(value, "duration_ms"))
                .transpose()?),
            failure_reason: Set(challenge.failure_reason().map(str::to_string)),
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SolverAttemptDto {
    solver: String,
    outcome: String,
    attempted_at: u64,
    duration_ms: u64,
}

fn encode_solver_attempts(attempts: &[CaptchaSolverAttempt]) -> Result<String, DomainError> {
    let attempts: Vec<_> = attempts
        .iter()
        .map(|attempt| SolverAttemptDto {
            solver: attempt.solver().to_string(),
            outcome: attempt.outcome().to_string(),
            attempted_at: attempt.attempted_at(),
            duration_ms: attempt.duration_ms(),
        })
        .collect();
    serde_json::to_string(&attempts).map_err(|error| {
        DomainError::StorageError(format!("failed to encode CAPTCHA attempts: {error}"))
    })
}

fn decode_solver_attempts(value: &str) -> Result<Vec<CaptchaSolverAttempt>, DomainError> {
    let attempts: Vec<SolverAttemptDto> = serde_json::from_str(value).map_err(|error| {
        DomainError::StorageError(format!(
            "captcha_log contains invalid solver attempts: {error}"
        ))
    })?;
    attempts
        .into_iter()
        .map(|attempt| {
            Ok(CaptchaSolverAttempt::new(
                attempt.solver,
                attempt.outcome.parse::<CaptchaSolverAttemptOutcome>()?,
                attempt.attempted_at,
                attempt.duration_ms,
            ))
        })
        .collect()
}

fn to_u64(value: i64, field: &str) -> Result<u64, DomainError> {
    u64::try_from(value).map_err(|_| invalid_integer(field))
}

fn to_i64(value: u64, field: &str) -> Result<i64, DomainError> {
    i64::try_from(value)
        .map_err(|_| DomainError::ValidationError(format!("CAPTCHA {field} exceeds i64::MAX")))
}

fn invalid_integer(field: &str) -> DomainError {
    DomainError::StorageError(format!("captcha_log contains an invalid {field}"))
}
