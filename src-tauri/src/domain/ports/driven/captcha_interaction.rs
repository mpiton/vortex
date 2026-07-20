use crate::domain::error::DomainError;
use crate::domain::model::captcha::{CaptchaChallenge, CaptchaId};

/// Controls the local human-assisted UI for a CAPTCHA challenge.
pub trait CaptchaInteraction: Send + Sync {
    fn request(&self, challenge: &CaptchaChallenge) -> Result<(), DomainError>;
    fn dismiss(&self, challenge_id: &CaptchaId) -> Result<(), DomainError>;
}
