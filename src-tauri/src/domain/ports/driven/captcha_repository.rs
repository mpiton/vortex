use crate::domain::error::DomainError;
use crate::domain::model::captcha::{CaptchaChallenge, CaptchaId};
use crate::domain::model::download::DownloadId;

pub trait CaptchaRepository: Send + Sync {
    fn save(&self, challenge: &CaptchaChallenge) -> Result<(), DomainError>;
    fn find_by_id(&self, id: &CaptchaId) -> Result<Option<CaptchaChallenge>, DomainError>;
    fn list(&self) -> Result<Vec<CaptchaChallenge>, DomainError>;
    fn list_pending(&self) -> Result<Vec<CaptchaChallenge>, DomainError>;

    fn find_next_pending(&self) -> Result<Option<CaptchaChallenge>, DomainError> {
        Ok(self.list_pending()?.into_iter().next())
    }

    fn find_pending_by_download(
        &self,
        download_id: DownloadId,
    ) -> Result<Option<CaptchaChallenge>, DomainError> {
        Ok(self
            .list_pending()?
            .into_iter()
            .find(|challenge| challenge.download_id() == download_id))
    }
}
