use crate::domain::error::DomainError;
use crate::domain::model::captcha::CaptchaChallenge;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptchaSolverOutcome {
    Solved,
    Unavailable,
    Rejected,
}

pub trait CaptchaSolver: Send + Sync {
    fn name(&self) -> &str;

    fn solve(
        &self,
        challenge: &CaptchaChallenge,
        solution: &str,
    ) -> Result<CaptchaSolverOutcome, DomainError>;
}
