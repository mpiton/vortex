use crate::domain::error::DomainError;
use crate::domain::model::captcha::{CaptchaChallenge, CaptchaSolution};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptchaSolverOutcome {
    Solved(CaptchaSolution),
    Unavailable,
    Rejected,
    InteractionRequired,
}

pub trait CaptchaSolver: Send + Sync {
    fn name(&self) -> &str;

    fn solve(
        &self,
        challenge: &CaptchaChallenge,
        solution: &str,
    ) -> Result<CaptchaSolverOutcome, DomainError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::captcha::CaptchaSolution;

    #[test]
    fn solved_outcome_carries_a_redacted_ephemeral_solution() {
        let outcome = CaptchaSolverOutcome::Solved(
            CaptchaSolution::try_new("secret-answer").expect("valid solution"),
        );

        let CaptchaSolverOutcome::Solved(solution) = outcome else {
            panic!("expected solved outcome");
        };
        assert_eq!(solution.expose(), "secret-answer");
        assert!(!format!("{solution:?}").contains("secret-answer"));
    }

    #[test]
    fn interactive_solver_can_request_the_browser_fallback() {
        assert_eq!(
            CaptchaSolverOutcome::InteractionRequired,
            CaptchaSolverOutcome::InteractionRequired
        );
    }
}
