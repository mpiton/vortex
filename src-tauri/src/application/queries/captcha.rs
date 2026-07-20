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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::application::queries::{CaptchaGetPendingQuery, CaptchaListQuery};
    use crate::application::test_support::{NoopHistoryRepo, make_history_query_bus};
    use crate::domain::error::DomainError;
    use crate::domain::model::captcha::{CaptchaChallenge, CaptchaId, CaptchaType};
    use crate::domain::model::download::DownloadId;
    use crate::domain::ports::driven::CaptchaRepository;

    struct MemoryCaptchaRepo(Mutex<Vec<CaptchaChallenge>>);

    impl CaptchaRepository for MemoryCaptchaRepo {
        fn save(&self, challenge: &CaptchaChallenge) -> Result<(), DomainError> {
            self.0.lock().unwrap().push(challenge.clone());
            Ok(())
        }

        fn find_by_id(&self, id: &CaptchaId) -> Result<Option<CaptchaChallenge>, DomainError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|challenge| challenge.id() == id)
                .cloned())
        }

        fn list(&self) -> Result<Vec<CaptchaChallenge>, DomainError> {
            Ok(self.0.lock().unwrap().clone())
        }

        fn list_pending(&self) -> Result<Vec<CaptchaChallenge>, DomainError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|challenge| challenge.status() == CaptchaStatus::Pending)
                .cloned()
                .collect())
        }
    }

    fn challenge(id: &str, created_at: u64) -> CaptchaChallenge {
        CaptchaChallenge::new(
            CaptchaId::new(id),
            DownloadId(created_at),
            CaptchaType::Image,
            "https://hoster.example/captcha".into(),
            created_at,
            created_at + 60_000,
        )
        .unwrap()
        .with_image_data(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01".to_vec())
        .unwrap()
    }

    fn bus(items: Vec<CaptchaChallenge>) -> QueryBus {
        make_history_query_bus(Arc::new(NoopHistoryRepo))
            .with_captcha_repo(Arc::new(MemoryCaptchaRepo(Mutex::new(items))))
    }

    #[tokio::test]
    async fn list_returns_metadata_without_ephemeral_images() {
        let results = bus(vec![challenge("captcha-1", 1_000)])
            .handle_captcha_list(CaptchaListQuery)
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "captcha-1");
        assert!(results[0].image_data.is_none());
    }

    #[tokio::test]
    async fn get_pending_supports_explicit_id_and_oldest_fallback() {
        let mut solved = challenge("captcha-solved", 500);
        solved.solve(600, "manual").unwrap();
        let bus = bus(vec![challenge("captcha-oldest", 1_000), solved]);

        let explicit = bus
            .handle_captcha_get_pending(CaptchaGetPendingQuery {
                challenge_id: Some(CaptchaId::new("captcha-oldest")),
            })
            .await
            .unwrap()
            .expect("pending by id");
        let solved = bus
            .handle_captcha_get_pending(CaptchaGetPendingQuery {
                challenge_id: Some(CaptchaId::new("captcha-solved")),
            })
            .await
            .unwrap();
        let next = bus
            .handle_captcha_get_pending(CaptchaGetPendingQuery { challenge_id: None })
            .await
            .unwrap()
            .expect("next pending");

        assert_eq!(explicit.id, "captcha-oldest");
        assert!(solved.is_none());
        assert_eq!(next.id, "captcha-oldest");
        assert!(next.image_data.is_some());
    }
}
