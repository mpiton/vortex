use crate::domain::error::DomainError;
use crate::domain::model::captcha::CaptchaChallenge;

/// Requests a local human-assisted UI for a pending CAPTCHA challenge.
pub trait CaptchaInteraction: Send + Sync {
    fn request(&self, challenge: &CaptchaChallenge) -> Result<(), DomainError>;
}
