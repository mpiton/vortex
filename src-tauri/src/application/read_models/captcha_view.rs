use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Serialize;

use crate::domain::model::captcha::{CaptchaChallenge, captcha_image_mime_type};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptchaViewDto {
    pub id: String,
    pub download_id: u64,
    pub challenge_type: String,
    pub challenge_url: String,
    pub image_data: Option<String>,
    pub image_mime_type: Option<String>,
    pub status: String,
    pub solver: Option<String>,
    pub attempts: u32,
    pub created_at: u64,
    pub expires_at: u64,
    pub resolved_at: Option<u64>,
    pub duration_ms: Option<u64>,
    pub failure_reason: Option<String>,
}

impl From<CaptchaChallenge> for CaptchaViewDto {
    fn from(challenge: CaptchaChallenge) -> Self {
        Self::build(challenge, true)
    }
}

impl CaptchaViewDto {
    pub fn metadata(challenge: CaptchaChallenge) -> Self {
        Self::build(challenge, false)
    }

    fn build(challenge: CaptchaChallenge, include_image: bool) -> Self {
        let image_mime_type = include_image
            .then(|| challenge.image_data().and_then(captcha_image_mime_type))
            .flatten()
            .map(str::to_string);
        Self {
            id: challenge.id().to_string(),
            download_id: challenge.download_id().0,
            challenge_type: challenge.challenge_type().to_string(),
            challenge_url: challenge.url().to_string(),
            image_data: include_image
                .then(|| challenge.image_data().map(|image| STANDARD.encode(image)))
                .flatten(),
            image_mime_type,
            status: challenge.status().to_string(),
            solver: challenge.solver().map(str::to_string),
            attempts: challenge.attempts(),
            created_at: challenge.created_at(),
            expires_at: challenge.expires_at(),
            resolved_at: challenge.resolved_at(),
            duration_ms: challenge.duration_ms(),
            failure_reason: challenge.failure_reason().map(str::to_string),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::captcha::{CaptchaId, CaptchaType};
    use crate::domain::model::download::DownloadId;

    fn challenge() -> CaptchaChallenge {
        CaptchaChallenge::new(
            CaptchaId::new("captcha-1"),
            DownloadId(1),
            CaptchaType::Image,
            "https://hoster.example/file".into(),
            1_000,
            61_000,
        )
        .expect("challenge")
        .with_image_data(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01".to_vec())
        .expect("image")
    }

    #[test]
    fn list_metadata_omits_image_bytes_but_pending_detail_includes_them() {
        let metadata = CaptchaViewDto::metadata(challenge());
        let detail = CaptchaViewDto::from(challenge());
        let payload = serde_json::to_value(&detail).unwrap();

        assert!(metadata.image_data.is_none());
        assert!(metadata.image_mime_type.is_none());
        assert_eq!(payload["imageData"], "iVBORw0KGgoAAAANSUhEUgAAAAEAAAAB");
        assert_eq!(detail.image_mime_type.as_deref(), Some("image/png"));
    }
}
