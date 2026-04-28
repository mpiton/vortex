//! Authenticated encryption keyed by a user-provided passphrase.
//!
//! The account import / export commands serialize the bundle to bytes,
//! hand it to [`PassphraseCodec::seal`] for encryption, and write the
//! resulting blob to disk. Decryption reverses the flow and refuses
//! tampered ciphertext, so any failure returned by [`open`](
//! PassphraseCodec::open) means the file cannot be trusted.
//!
//! Implementations MUST:
//!
//! - derive the encryption key from the passphrase via a memory-hard or
//!   iteration-stretched KDF (PBKDF2-HMAC-SHA256 with ≥ 200 000 rounds
//!   for the bundled adapter);
//! - generate a fresh random salt and nonce on every call to
//!   [`seal`](PassphraseCodec::seal);
//! - return [`DomainError::ValidationError`] on a wrong passphrase or
//!   any cryptographic check failure (authentication tag mismatch,
//!   truncated input, unsupported version), rather than panicking.

use crate::domain::error::DomainError;

pub trait PassphraseCodec: Send + Sync {
    /// Encrypt and authenticate `plaintext` under `passphrase`. The
    /// returned blob bundles the algorithm version, salt, nonce, and
    /// ciphertext + auth tag — callers treat it as opaque bytes.
    fn seal(&self, passphrase: &str, plaintext: &[u8]) -> Result<Vec<u8>, DomainError>;

    /// Decrypt and authenticate `ciphertext` produced by
    /// [`seal`](PassphraseCodec::seal). Wrong passphrase or any
    /// integrity-check failure yields [`DomainError::ValidationError`].
    fn open(&self, passphrase: &str, ciphertext: &[u8]) -> Result<Vec<u8>, DomainError>;
}
