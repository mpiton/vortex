use std::fmt;

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
            CaptchaType::Image => "Image",
            CaptchaType::ReCaptchaV2 => "reCAPTCHA v2",
            CaptchaType::ReCaptchaV3 => "reCAPTCHA v3",
            CaptchaType::HCaptcha => "hCaptcha",
            CaptchaType::TextInput => "TextInput",
        };
        write!(f, "{name}")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaptchaChallenge {
    id: u64,
    challenge_type: CaptchaType,
    url: String,
    image_data: Option<Vec<u8>>,
    solved: bool,
    solution: Option<String>,
}

impl CaptchaChallenge {
    pub fn new(id: u64, challenge_type: CaptchaType, url: String) -> Self {
        Self {
            id,
            challenge_type,
            url,
            image_data: None,
            solved: false,
            solution: None,
        }
    }

    pub fn with_image_data(mut self, data: Vec<u8>) -> Self {
        self.image_data = Some(data);
        self
    }

    pub fn solve(&mut self, solution: String) {
        self.solved = true;
        self.solution = Some(solution);
    }

    pub fn is_solved(&self) -> bool {
        self.solved
    }

    pub fn id(&self) -> u64 {
        self.id
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

    pub fn solution(&self) -> Option<&str> {
        self.solution.as_deref()
    }
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
    fn retry_renews_pending_deadline_and_counts_attempt() {
        let mut c = make_challenge();
        c.retry(5_000, 65_000).expect("pending can be retried");

        assert_eq!(c.status(), CaptchaStatus::Pending);
        assert_eq!(c.attempts(), 1);
        assert_eq!(c.expires_at(), 65_000);
    }

    #[test]
    fn image_payload_is_bounded() {
        let data = vec![0u8, 1, 2, 3];
        let c = make_challenge()
            .with_image_data(data.clone())
            .expect("small image");
        assert_eq!(c.image_data(), Some(data.as_slice()));

        let too_large = vec![0; MAX_CAPTCHA_IMAGE_BYTES + 1];
        assert!(make_challenge().with_image_data(too_large).is_err());
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
}
