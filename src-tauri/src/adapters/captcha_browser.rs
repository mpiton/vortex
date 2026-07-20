use sha2::{Digest, Sha256};

const CAPTCHA_BROWSER_WINDOW_PREFIX: &str = "captcha-browser-";

pub(crate) fn browser_window_label(challenge_id: &str) -> String {
    let digest = Sha256::digest(challenge_id.as_bytes());
    format!("{CAPTCHA_BROWSER_WINDOW_PREFIX}{}", hex::encode(digest))
}
