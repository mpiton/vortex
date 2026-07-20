use std::fmt;
use std::str::FromStr;

use crate::domain::error::DomainError;
use crate::domain::model::download::DownloadId;

pub const MAX_CAPTCHA_IMAGE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_CAPTCHA_SOLUTION_BYTES: usize = 4 * 1024;
pub const MAX_CAPTCHA_ID_BYTES: usize = 128;
pub const MAX_CAPTCHA_IMAGE_PIXELS: u64 = 16_000_000;
const MAX_CAPTCHA_URL_BYTES: usize = 8 * 1024;
const MAX_CAPTCHA_SOLVER_NAME_BYTES: usize = 128;
const MAX_CAPTCHA_SOLVER_ATTEMPTS: usize = 64;
const REDACTED_CAPTCHA_URL: &str = "[redacted]";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CaptchaId(String);

impl CaptchaId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn try_new(value: impl Into<String>) -> Result<Self, DomainError> {
        let id = Self::new(value);
        if id.as_str().trim().is_empty() || id.as_str().len() > MAX_CAPTCHA_ID_BYTES {
            return Err(validation("CAPTCHA id is invalid"));
        }
        Ok(id)
    }
}

impl fmt::Display for CaptchaId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct CaptchaSolution(String);

impl CaptchaSolution {
    pub fn try_new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.trim().is_empty() || value.len() > MAX_CAPTCHA_SOLUTION_BYTES {
            return Err(validation("CAPTCHA solution is invalid"));
        }
        Ok(Self(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CaptchaSolution {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("CaptchaSolution")
            .field(&"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptchaType {
    Image,
    ReCaptchaV2,
    ReCaptchaV3,
    HCaptcha,
    TextInput,
}

impl fmt::Display for CaptchaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            CaptchaType::Image => "image",
            CaptchaType::ReCaptchaV2 => "recaptcha_v2",
            CaptchaType::ReCaptchaV3 => "recaptcha_v3",
            CaptchaType::HCaptcha => "hcaptcha",
            CaptchaType::TextInput => "text_input",
        };
        write!(f, "{name}")
    }
}

impl FromStr for CaptchaType {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "image" => Ok(Self::Image),
            "recaptcha_v2" => Ok(Self::ReCaptchaV2),
            "recaptcha_v3" => Ok(Self::ReCaptchaV3),
            "hcaptcha" => Ok(Self::HCaptcha),
            "text_input" => Ok(Self::TextInput),
            _ => Err(DomainError::ValidationError(format!(
                "unknown CAPTCHA type '{value}'"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptchaStatus {
    Pending,
    Solved,
    Skipped,
    TimedOut,
}

impl fmt::Display for CaptchaStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pending => "pending",
            Self::Solved => "solved",
            Self::Skipped => "skipped",
            Self::TimedOut => "timed_out",
        })
    }
}

impl FromStr for CaptchaStatus {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "solved" => Ok(Self::Solved),
            "skipped" => Ok(Self::Skipped),
            "timed_out" => Ok(Self::TimedOut),
            _ => Err(DomainError::ValidationError(format!(
                "unknown CAPTCHA status '{value}'"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptchaSolverAttemptOutcome {
    Solved,
    Unavailable,
    Rejected,
    Failed,
    InteractionRequired,
}

impl fmt::Display for CaptchaSolverAttemptOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Solved => "solved",
            Self::Unavailable => "unavailable",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
            Self::InteractionRequired => "interaction_required",
        })
    }
}

impl FromStr for CaptchaSolverAttemptOutcome {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "solved" => Ok(Self::Solved),
            "unavailable" => Ok(Self::Unavailable),
            "rejected" => Ok(Self::Rejected),
            "failed" => Ok(Self::Failed),
            "interaction_required" => Ok(Self::InteractionRequired),
            _ => Err(validation("unknown CAPTCHA solver attempt outcome")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptchaSolverAttempt {
    solver: String,
    outcome: CaptchaSolverAttemptOutcome,
    attempted_at: u64,
    duration_ms: u64,
}

impl CaptchaSolverAttempt {
    pub fn new(
        solver: impl Into<String>,
        outcome: CaptchaSolverAttemptOutcome,
        attempted_at: u64,
        duration_ms: u64,
    ) -> Self {
        Self {
            solver: solver.into(),
            outcome,
            attempted_at,
            duration_ms,
        }
    }

    pub fn solver(&self) -> &str {
        &self.solver
    }

    pub fn outcome(&self) -> CaptchaSolverAttemptOutcome {
        self.outcome
    }

    pub fn attempted_at(&self) -> u64 {
        self.attempted_at
    }

    pub fn duration_ms(&self) -> u64 {
        self.duration_ms
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptchaChallenge {
    id: CaptchaId,
    download_id: DownloadId,
    challenge_type: CaptchaType,
    url: String,
    image_data: Option<Vec<u8>>,
    status: CaptchaStatus,
    solver: Option<String>,
    attempts: u32,
    solver_attempts: Vec<CaptchaSolverAttempt>,
    created_at: u64,
    expires_at: u64,
    resolved_at: Option<u64>,
    duration_ms: Option<u64>,
    failure_reason: Option<String>,
}

pub struct CaptchaChallengeRecord {
    pub id: CaptchaId,
    pub download_id: DownloadId,
    pub challenge_type: CaptchaType,
    pub url: String,
    pub image_data: Option<Vec<u8>>,
    pub status: CaptchaStatus,
    pub solver: Option<String>,
    pub attempts: u32,
    pub solver_attempts: Vec<CaptchaSolverAttempt>,
    pub created_at: u64,
    pub expires_at: u64,
    pub resolved_at: Option<u64>,
    pub duration_ms: Option<u64>,
    pub failure_reason: Option<String>,
}

impl CaptchaChallenge {
    pub fn new(
        id: CaptchaId,
        download_id: DownloadId,
        challenge_type: CaptchaType,
        url: String,
        created_at: u64,
        expires_at: u64,
    ) -> Result<Self, DomainError> {
        if id.as_str().trim().is_empty() || id.as_str().len() > MAX_CAPTCHA_ID_BYTES {
            return Err(validation("CAPTCHA id is invalid"));
        }
        if url.trim().is_empty() || url.len() > MAX_CAPTCHA_URL_BYTES {
            return Err(validation("CAPTCHA URL is invalid"));
        }
        if expires_at <= created_at {
            return Err(validation("CAPTCHA deadline must be after creation"));
        }
        Ok(Self {
            id,
            download_id,
            challenge_type,
            url,
            image_data: None,
            status: CaptchaStatus::Pending,
            solver: None,
            attempts: 0,
            solver_attempts: Vec::new(),
            created_at,
            expires_at,
            resolved_at: None,
            duration_ms: None,
            failure_reason: None,
        })
    }

    pub fn reconstruct(record: CaptchaChallengeRecord) -> Result<Self, DomainError> {
        let mut challenge = Self::new(
            record.id,
            record.download_id,
            record.challenge_type,
            record.url,
            record.created_at,
            record.expires_at,
        )?;
        if let Some(image) = record.image_data {
            challenge = challenge.with_image_data(image)?;
        }
        for attempt in record.solver_attempts {
            challenge.record_solver_attempt(attempt)?;
        }
        challenge.status = record.status;
        challenge.solver = record.solver;
        challenge.attempts = record.attempts;
        challenge.resolved_at = record.resolved_at;
        challenge.duration_ms = record.duration_ms;
        challenge.failure_reason = record.failure_reason;
        Ok(challenge)
    }

    pub fn with_image_data(mut self, data: Vec<u8>) -> Result<Self, DomainError> {
        if data.is_empty()
            || data.len() > MAX_CAPTCHA_IMAGE_BYTES
            || captcha_image_mime_type(&data).is_none()
        {
            return Err(validation(
                "CAPTCHA image is invalid or exceeds safety limits",
            ));
        }
        self.image_data = Some(data);
        Ok(self)
    }

    pub fn solve(&mut self, now_ms: u64, solver: &str) -> Result<(), DomainError> {
        self.ensure_pending()?;
        if solver.trim().is_empty() {
            return Err(validation("CAPTCHA solver cannot be empty"));
        }
        self.status = CaptchaStatus::Solved;
        self.solver = Some(solver.to_string());
        self.resolve(now_ms);
        Ok(())
    }

    pub fn skip(&mut self, now_ms: u64, reason: &str) -> Result<(), DomainError> {
        self.ensure_pending()?;
        if reason.trim().is_empty() {
            return Err(validation("CAPTCHA skip reason cannot be empty"));
        }
        self.status = CaptchaStatus::Skipped;
        self.failure_reason = Some(reason.to_string());
        self.resolve(now_ms);
        Ok(())
    }

    pub fn timeout(&mut self, now_ms: u64) -> Result<(), DomainError> {
        self.ensure_pending()?;
        self.status = CaptchaStatus::TimedOut;
        self.failure_reason = Some("CAPTCHA timed out".to_string());
        self.resolve(now_ms);
        Ok(())
    }

    pub fn retry(&mut self, now_ms: u64, expires_at: u64) -> Result<(), DomainError> {
        self.ensure_pending()?;
        if expires_at <= now_ms {
            return Err(validation("CAPTCHA retry deadline must be in the future"));
        }
        self.attempts = self.attempts.saturating_add(1);
        self.expires_at = expires_at;
        Ok(())
    }

    pub fn record_solver_attempt(
        &mut self,
        attempt: CaptchaSolverAttempt,
    ) -> Result<(), DomainError> {
        if attempt.solver().trim().is_empty()
            || attempt.solver().len() > MAX_CAPTCHA_SOLVER_NAME_BYTES
        {
            return Err(validation("CAPTCHA solver name is invalid"));
        }
        let retained_before_push = MAX_CAPTCHA_SOLVER_ATTEMPTS.saturating_sub(1);
        if self.solver_attempts.len() > retained_before_push {
            let discard = self.solver_attempts.len() - retained_before_push;
            self.solver_attempts.drain(..discard);
        }
        self.solver_attempts.push(attempt);
        Ok(())
    }

    fn ensure_pending(&self) -> Result<(), DomainError> {
        if self.status != CaptchaStatus::Pending {
            return Err(validation("CAPTCHA challenge is no longer pending"));
        }
        Ok(())
    }

    fn resolve(&mut self, now_ms: u64) {
        self.resolved_at = Some(now_ms);
        self.duration_ms = Some(now_ms.saturating_sub(self.created_at));
        self.image_data = None;
        self.url = REDACTED_CAPTCHA_URL.to_string();
    }

    pub fn id(&self) -> &CaptchaId {
        &self.id
    }

    pub fn download_id(&self) -> DownloadId {
        self.download_id
    }

    pub fn challenge_type(&self) -> CaptchaType {
        self.challenge_type
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn image_data(&self) -> Option<&[u8]> {
        self.image_data.as_deref()
    }

    pub fn status(&self) -> CaptchaStatus {
        self.status
    }

    pub fn solver(&self) -> Option<&str> {
        self.solver.as_deref()
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    pub fn solver_attempts(&self) -> &[CaptchaSolverAttempt] {
        &self.solver_attempts
    }

    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    pub fn expires_at(&self) -> u64 {
        self.expires_at
    }

    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_at
    }

    pub fn resolved_at(&self) -> Option<u64> {
        self.resolved_at
    }

    pub fn duration_ms(&self) -> Option<u64> {
        self.duration_ms
    }

    pub fn failure_reason(&self) -> Option<&str> {
        self.failure_reason.as_deref()
    }
}

fn validation(message: &str) -> DomainError {
    DomainError::ValidationError(message.to_string())
}

pub fn captcha_image_mime_type(data: &[u8]) -> Option<&'static str> {
    let (mime, width, height) = image_metadata(data)?;
    let pixels = u64::from(width).checked_mul(u64::from(height))?;
    (width > 0 && height > 0 && pixels <= MAX_CAPTCHA_IMAGE_PIXELS).then_some(mime)
}

fn image_metadata(data: &[u8]) -> Option<(&'static str, u32, u32)> {
    if data.len() >= 24 && data.starts_with(b"\x89PNG\r\n\x1a\n") && &data[12..16] == b"IHDR" {
        return Some((
            "image/png",
            u32::from_be_bytes(data[16..20].try_into().ok()?),
            u32::from_be_bytes(data[20..24].try_into().ok()?),
        ));
    }
    if data.len() >= 10 && (data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")) {
        return Some((
            "image/gif",
            u32::from(u16::from_le_bytes(data[6..8].try_into().ok()?)),
            u32::from(u16::from_le_bytes(data[8..10].try_into().ok()?)),
        ));
    }
    jpeg_dimensions(data).map(|(width, height)| ("image/jpeg", width, height))
}

fn jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if !data.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut cursor = 2;
    while cursor + 1 < data.len() {
        while cursor < data.len() && data[cursor] == 0xff {
            cursor += 1;
        }
        let marker = *data.get(cursor)?;
        cursor += 1;
        if marker == 0xd9 || marker == 0xda {
            return None;
        }
        if marker == 0x01 || (0xd0..=0xd8).contains(&marker) {
            continue;
        }
        let segment_len = usize::from(u16::from_be_bytes([
            *data.get(cursor)?,
            *data.get(cursor + 1)?,
        ]));
        if segment_len < 2 || cursor.checked_add(segment_len)? > data.len() {
            return None;
        }
        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            if segment_len < 7 {
                return None;
            }
            let height = u32::from(u16::from_be_bytes([
                *data.get(cursor + 3)?,
                *data.get(cursor + 4)?,
            ]));
            let width = u32::from(u16::from_be_bytes([
                *data.get(cursor + 5)?,
                *data.get(cursor + 6)?,
            ]));
            return Some((width, height));
        }
        cursor += segment_len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::download::DownloadId;

    fn make_challenge() -> CaptchaChallenge {
        CaptchaChallenge::new(
            CaptchaId::new("captcha-42"),
            DownloadId(7),
            CaptchaType::Image,
            "https://example.com/captcha".to_string(),
            1_000,
            61_000,
        )
        .expect("valid challenge")
    }

    fn png_image(width: u32, height: u32) -> Vec<u8> {
        let mut image = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        image.extend_from_slice(&width.to_be_bytes());
        image.extend_from_slice(&height.to_be_bytes());
        image
    }

    fn gif_image(width: u16, height: u16) -> Vec<u8> {
        let mut image = b"GIF89a".to_vec();
        image.extend_from_slice(&width.to_le_bytes());
        image.extend_from_slice(&height.to_le_bytes());
        image
    }

    fn jpeg_image(width: u16, height: u16) -> Vec<u8> {
        let mut image = vec![0xff, 0xd8, 0xff, 0xc0, 0x00, 0x07, 0x08];
        image.extend_from_slice(&height.to_be_bytes());
        image.extend_from_slice(&width.to_be_bytes());
        image
    }

    #[test]
    fn new_challenge_is_pending_and_has_a_deadline() {
        let c = make_challenge();
        assert_eq!(c.id().as_str(), "captcha-42");
        assert_eq!(c.download_id(), DownloadId(7));
        assert_eq!(c.challenge_type(), CaptchaType::Image);
        assert_eq!(c.url(), "https://example.com/captcha");
        assert!(c.image_data().is_none());
        assert_eq!(c.status(), CaptchaStatus::Pending);
        assert_eq!(c.expires_at(), 61_000);
        assert_eq!(c.attempts(), 0);
    }

    #[test]
    fn solve_records_solver_and_duration_without_solution() {
        let mut c = make_challenge();
        c.solve(4_000, "manual").expect("pending can be solved");

        assert_eq!(c.status(), CaptchaStatus::Solved);
        assert_eq!(c.solver(), Some("manual"));
        assert_eq!(c.resolved_at(), Some(4_000));
        assert_eq!(c.duration_ms(), Some(3_000));
    }

    #[test]
    fn captcha_solution_validates_and_redacts_debug_output() {
        let solution = CaptchaSolution::try_new("secret-answer").expect("valid solution");

        assert_eq!(solution.expose(), "secret-answer");
        assert_eq!(format!("{solution:?}"), "CaptchaSolution(\"<redacted>\")");
        assert!(CaptchaSolution::try_new("  ").is_err());
        assert!(CaptchaSolution::try_new("x".repeat(MAX_CAPTCHA_SOLUTION_BYTES + 1)).is_err());
    }

    #[test]
    fn solver_attempts_are_ordered_and_reconstructed_without_solutions() {
        let mut challenge = make_challenge();
        challenge
            .record_solver_attempt(CaptchaSolverAttempt::new(
                "vortex-mod-captcha-ocr",
                CaptchaSolverAttemptOutcome::Unavailable,
                1_100,
                25,
            ))
            .expect("record OCR attempt");
        challenge
            .record_solver_attempt(CaptchaSolverAttempt::new(
                "vortex-mod-captcha-anticaptcha",
                CaptchaSolverAttemptOutcome::Solved,
                1_200,
                75,
            ))
            .expect("record service attempt");

        assert_eq!(challenge.solver_attempts().len(), 2);
        assert_eq!(
            challenge.solver_attempts()[0].solver(),
            "vortex-mod-captcha-ocr"
        );
        assert_eq!(
            challenge.solver_attempts()[1].outcome(),
            CaptchaSolverAttemptOutcome::Solved
        );

        let record = CaptchaChallengeRecord {
            id: challenge.id().clone(),
            download_id: challenge.download_id(),
            challenge_type: challenge.challenge_type(),
            url: challenge.url().to_string(),
            image_data: None,
            status: challenge.status(),
            solver: None,
            attempts: challenge.attempts(),
            solver_attempts: challenge.solver_attempts().to_vec(),
            created_at: challenge.created_at(),
            expires_at: challenge.expires_at(),
            resolved_at: None,
            duration_ms: None,
            failure_reason: None,
        };
        let restored = CaptchaChallenge::reconstruct(record).expect("reconstruct challenge");

        assert_eq!(restored.solver_attempts(), challenge.solver_attempts());
    }

    #[test]
    fn solver_attempt_rejects_blank_names() {
        let mut challenge = make_challenge();

        assert!(
            challenge
                .record_solver_attempt(CaptchaSolverAttempt::new(
                    " ",
                    CaptchaSolverAttemptOutcome::Failed,
                    1_100,
                    10,
                ))
                .is_err()
        );
        assert!(challenge.solver_attempts().is_empty());
    }

    #[test]
    fn solver_attempt_history_keeps_only_the_latest_entries() {
        let mut challenge = make_challenge();

        for index in 0..=64 {
            challenge
                .record_solver_attempt(CaptchaSolverAttempt::new(
                    format!("solver-{index}"),
                    CaptchaSolverAttemptOutcome::Failed,
                    1_100 + index,
                    10,
                ))
                .expect("record bounded solver attempt");
        }

        assert_eq!(challenge.solver_attempts().len(), 64);
        assert_eq!(challenge.solver_attempts()[0].solver(), "solver-1");
        assert_eq!(challenge.solver_attempts()[63].solver(), "solver-64");
    }

    #[test]
    fn late_solver_attempt_can_be_recorded_after_manual_resolution() {
        let mut challenge = make_challenge();
        challenge
            .solve(1_100, "manual")
            .expect("manual resolution succeeds");

        challenge
            .record_solver_attempt(CaptchaSolverAttempt::new(
                "vortex-mod-captcha-ocr",
                CaptchaSolverAttemptOutcome::Rejected,
                1_050,
                100,
            ))
            .expect("completed automatic attempt remains auditable");

        assert_eq!(challenge.solver_attempts().len(), 1);
        assert_eq!(
            challenge.solver_attempts()[0].outcome(),
            CaptchaSolverAttemptOutcome::Rejected
        );
    }

    #[test]
    fn skip_and_timeout_are_explicit_terminal_states() {
        let mut c = make_challenge();
        c.skip(2_000, "Skipped by user")
            .expect("pending can be skipped");
        assert_eq!(c.status(), CaptchaStatus::Skipped);
        assert_eq!(c.failure_reason(), Some("Skipped by user"));

        let mut timed_out = make_challenge();
        timed_out.timeout(61_000).expect("pending can time out");
        assert_eq!(timed_out.status(), CaptchaStatus::TimedOut);
        assert_eq!(timed_out.failure_reason(), Some("CAPTCHA timed out"));
    }

    #[test]
    fn skip_rejects_a_blank_reason_without_terminalizing_the_challenge() {
        let mut challenge = make_challenge();

        assert!(challenge.skip(2_000, "  ").is_err());
        assert_eq!(challenge.status(), CaptchaStatus::Pending);
        assert!(challenge.failure_reason().is_none());
    }

    #[test]
    fn retry_renews_pending_deadline_and_counts_attempt() {
        let mut c = make_challenge();
        c.retry(5_000, 65_000).expect("pending can be retried");

        assert_eq!(c.status(), CaptchaStatus::Pending);
        assert_eq!(c.attempts(), 1);
        assert_eq!(c.expires_at(), 65_000);
    }

    #[test]
    fn image_payload_is_bounded() {
        let data = png_image(1, 1);
        let c = make_challenge()
            .with_image_data(data.clone())
            .expect("small image");
        assert_eq!(c.image_data(), Some(data.as_slice()));

        let too_large = vec![0; MAX_CAPTCHA_IMAGE_BYTES + 1];
        assert!(make_challenge().with_image_data(too_large).is_err());
        assert!(
            make_challenge()
                .with_image_data(b"<svg/>".to_vec())
                .is_err()
        );
        assert!(
            make_challenge()
                .with_image_data(png_image(5_000, 5_000))
                .is_err()
        );
    }

    #[test]
    fn gif_dimensions_accept_valid_headers_and_reject_malformed_ones() {
        assert_eq!(captcha_image_mime_type(&gif_image(2, 3)), Some("image/gif"));
        assert_eq!(captcha_image_mime_type(b"GIF89a\x02\0\x03"), None);
        assert_eq!(captcha_image_mime_type(&gif_image(0, 3)), None);
    }

    #[test]
    fn jpeg_dimensions_accept_valid_headers_and_reject_malformed_ones() {
        assert_eq!(
            captcha_image_mime_type(&jpeg_image(2, 3)),
            Some("image/jpeg")
        );
        assert_eq!(captcha_image_mime_type(&jpeg_image(0, 3)), None);
        assert_eq!(
            captcha_image_mime_type(&[0xff, 0xd8, 0xff, 0xc0, 0x00, 0x20, 0x08]),
            None
        );
    }

    #[test]
    fn terminal_challenge_drops_ephemeral_image_material() {
        let mut challenge = make_challenge()
            .with_image_data(png_image(1, 1))
            .expect("small image");

        challenge.solve(2_000, "manual").expect("solve");

        assert!(challenge.image_data().is_none());
        assert_eq!(challenge.url(), REDACTED_CAPTCHA_URL);
    }

    #[test]
    fn persisted_enum_values_round_trip() {
        for challenge_type in [
            CaptchaType::Image,
            CaptchaType::ReCaptchaV2,
            CaptchaType::ReCaptchaV3,
            CaptchaType::HCaptcha,
            CaptchaType::TextInput,
        ] {
            assert_eq!(challenge_type.to_string().parse(), Ok(challenge_type));
        }
        for status in [
            CaptchaStatus::Pending,
            CaptchaStatus::Solved,
            CaptchaStatus::Skipped,
            CaptchaStatus::TimedOut,
        ] {
            assert_eq!(status.to_string().parse(), Ok(status));
        }
    }

    #[test]
    fn external_captcha_ids_are_bounded() {
        assert!(CaptchaId::try_new("captcha-1").is_ok());
        assert!(CaptchaId::try_new(" ").is_err());
        assert!(CaptchaId::try_new("x".repeat(MAX_CAPTCHA_ID_BYTES + 1)).is_err());
    }
}
