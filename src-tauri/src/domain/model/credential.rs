//! Credential types for account authentication.
//!
//! Used by `CredentialStore` port for secure credential management.
//! Actual storage is handled by the keychain adapter.

/// A stored credential for a service account.
///
/// Contains the authentication data needed to interact with a remote
/// service (hoster, debrid, captcha solver). The password field may
/// contain a password, API token, or other secret depending on the service.
#[derive(Clone, PartialEq)]
pub struct Credential {
    username: String,
    password: String,
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

impl Credential {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn password(&self) -> &str {
        &self.password
    }
}
