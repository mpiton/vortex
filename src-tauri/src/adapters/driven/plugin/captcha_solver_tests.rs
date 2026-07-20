use std::sync::Arc;

use super::captcha_solver::PluginCaptchaSolver;
use crate::domain::error::DomainError;
use crate::domain::model::captcha::{CaptchaChallenge, CaptchaId, CaptchaSolution, CaptchaType};
use crate::domain::model::download::DownloadId;
use crate::domain::model::plugin::{PluginInfo, PluginManifest};
use crate::domain::ports::driven::{CaptchaSolver, CaptchaSolverOutcome, PluginLoader};

struct SolvingLoader;

impl PluginLoader for SolvingLoader {
    fn load(&self, _: &PluginManifest) -> Result<(), DomainError> {
        Ok(())
    }

    fn unload(&self, _: &str) -> Result<(), DomainError> {
        Ok(())
    }

    fn resolve_url(&self, _: &str) -> Result<Option<PluginInfo>, DomainError> {
        Ok(None)
    }

    fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
        Ok(Vec::new())
    }

    fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> {
        Ok(())
    }

    fn solve_captcha(
        &self,
        plugin_name: &str,
        _: &CaptchaChallenge,
    ) -> Result<CaptchaSolverOutcome, DomainError> {
        assert_eq!(plugin_name, "vortex-mod-captcha-ocr");
        Ok(CaptchaSolverOutcome::Solved(
            CaptchaSolution::try_new("answer").expect("valid solution"),
        ))
    }
}

fn challenge() -> CaptchaChallenge {
    CaptchaChallenge::new(
        CaptchaId::new("captcha-1"),
        DownloadId(1),
        CaptchaType::Image,
        "https://example.com/captcha".to_string(),
        1_000,
        61_000,
    )
    .expect("valid challenge")
}

#[test]
fn plugin_solver_delegates_to_the_exact_named_plugin() {
    let solver = PluginCaptchaSolver::new("vortex-mod-captcha-ocr", Arc::new(SolvingLoader));

    assert_eq!(solver.name(), "vortex-mod-captcha-ocr");
    let outcome = solver.solve(&challenge(), "").expect("solve");
    let CaptchaSolverOutcome::Solved(solution) = outcome else {
        panic!("expected solved outcome");
    };
    assert_eq!(solution.expose(), "answer");
}
