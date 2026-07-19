use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::application::read_models::captcha_view::CaptchaViewDto;
use crate::domain::model::captcha::CaptchaStatus;

impl QueryBus {
    pub async fn handle_captcha_list(
        &self,
        _query: super::CaptchaListQuery,
    ) -> Result<Vec<CaptchaViewDto>, AppError> {
        Ok(self
            .captcha_repo()?
            .list()?
            .into_iter()
            .map(CaptchaViewDto::metadata)
            .collect())
    }

    pub async fn handle_captcha_get_pending(
        &self,
        query: super::CaptchaGetPendingQuery,
    ) -> Result<Option<CaptchaViewDto>, AppError> {
        let challenge = match query.challenge_id {
            Some(id) => self
                .captcha_repo()?
                .find_by_id(&id)?
                .filter(|challenge| challenge.status() == CaptchaStatus::Pending),
            None => self.captcha_repo()?.find_next_pending()?,
        };
        Ok(challenge.map(CaptchaViewDto::from))
    }
}
