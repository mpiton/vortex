use std::sync::Arc;

use crate::domain::error::DomainError;
use crate::domain::model::captcha::CaptchaChallenge;
use crate::domain::ports::driven::{CaptchaSolver, CaptchaSolverOutcome, PluginLoader};

pub struct PluginCaptchaSolver {
    plugin_name: String,
    loader: Arc<dyn PluginLoader>,
}

impl PluginCaptchaSolver {
    pub fn new(plugin_name: impl Into<String>, loader: Arc<dyn PluginLoader>) -> Self {
        Self {
            plugin_name: plugin_name.into(),
            loader,
        }
    }
}

impl CaptchaSolver for PluginCaptchaSolver {
    fn name(&self) -> &str {
        &self.plugin_name
    }

    fn solve(
        &self,
        challenge: &CaptchaChallenge,
        _solution: &str,
    ) -> Result<CaptchaSolverOutcome, DomainError> {
        self.loader.solve_captcha(&self.plugin_name, challenge)
    }
}
