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

    fn make_challenge() -> CaptchaChallenge {
        CaptchaChallenge::new(
            42,
            CaptchaType::Image,
            "https://example.com/captcha".to_string(),
        )
    }

    #[test]
    fn test_captcha_new() {
        let c = make_challenge();
        assert_eq!(c.id(), 42);
        assert_eq!(c.challenge_type(), CaptchaType::Image);
        assert_eq!(c.url(), "https://example.com/captcha");
        assert!(c.image_data().is_none());
        assert!(!c.is_solved());
        assert!(c.solution().is_none());
    }

    #[test]
    fn test_captcha_solve() {
        let mut c = make_challenge();
        c.solve("abc123".to_string());
        assert!(c.is_solved());
        assert_eq!(c.solution(), Some("abc123"));
    }

    #[test]
    fn test_captcha_is_solved() {
        let mut c = make_challenge();
        assert!(!c.is_solved());
        c.solve("x".to_string());
        assert!(c.is_solved());
    }

    #[test]
    fn test_captcha_with_image_data() {
        let data = vec![0u8, 1, 2, 3];
        let c = make_challenge().with_image_data(data.clone());
        assert_eq!(c.image_data(), Some(data.as_slice()));
    }

    #[test]
    fn test_captcha_type_display() {
        assert_eq!(CaptchaType::Image.to_string(), "Image");
        assert_eq!(CaptchaType::ReCaptchaV2.to_string(), "reCAPTCHA v2");
        assert_eq!(CaptchaType::ReCaptchaV3.to_string(), "reCAPTCHA v3");
        assert_eq!(CaptchaType::HCaptcha.to_string(), "hCaptcha");
        assert_eq!(CaptchaType::TextInput.to_string(), "TextInput");
    }
}
