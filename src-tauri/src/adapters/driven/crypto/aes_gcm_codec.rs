//! AES-256-GCM passphrase codec used by the account import / export
//! commands.
//!
//! Encryption flow:
//!
//! 1. Generate a fresh 16-byte random salt.
//! 2. Stretch the user passphrase with PBKDF2-HMAC-SHA256
//!    (`PBKDF2_ITERATIONS` rounds) to a 32-byte key.
//! 3. Generate a fresh 12-byte random nonce.
//! 4. AES-256-GCM seals the plaintext under (key, nonce). The
//!    associated data is the bundle header so a downgrade attack
//!    swapping `version` cannot pass authentication.
//! 5. Output bytes: `magic | version | iterations | salt | nonce | ct||tag`.
//!
//! Decryption verifies the magic + version, re-derives the key from
//! the supplied passphrase + stored salt, and returns the plaintext or
//! a [`DomainError::ValidationError`] on any mismatch — wrong
//! passphrase, tampered ciphertext, or unsupported header version.

use aes_gcm::aead::Aead;
use aes_gcm::aead::Payload;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use hmac::Hmac;
use pbkdf2::pbkdf2;
use rand::TryRng;
use rand::rngs::SysRng;
use sha2::Sha256;

use crate::domain::error::DomainError;
use crate::domain::ports::driven::PassphraseCodec;

const MAGIC: &[u8; 7] = b"VORTACC";
const VERSION: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const ITER_LEN: usize = 4;
/// PBKDF2 iteration count. OWASP 2024 minimum for PBKDF2-HMAC-SHA256
/// is 600 000 — we pick 200 000 as a balance between security and the
/// import / export commands running on cold-start without spinning the
/// fan. The value is stored alongside the ciphertext so future bumps
/// remain backward-compatible.
const PBKDF2_ITERATIONS: u32 = 200_000;
const HEADER_LEN: usize = MAGIC.len() + 1 + ITER_LEN + SALT_LEN + NONCE_LEN;

#[derive(Debug, Clone, Default)]
pub struct AesGcmPbkdf2Codec;

impl AesGcmPbkdf2Codec {
    pub fn new() -> Self {
        Self
    }

    fn derive_key(
        passphrase: &str,
        salt: &[u8],
        iterations: u32,
    ) -> Result<[u8; KEY_LEN], DomainError> {
        let mut key = [0u8; KEY_LEN];
        pbkdf2::<Hmac<Sha256>>(passphrase.as_bytes(), salt, iterations, &mut key)
            .map_err(|e| DomainError::StorageError(format!("pbkdf2 derivation failed: {e}")))?;
        Ok(key)
    }

    fn build_header(salt: &[u8; SALT_LEN], nonce: &[u8; NONCE_LEN]) -> Vec<u8> {
        let mut header = Vec::with_capacity(HEADER_LEN);
        header.extend_from_slice(MAGIC);
        header.push(VERSION);
        header.extend_from_slice(&PBKDF2_ITERATIONS.to_be_bytes());
        header.extend_from_slice(salt);
        header.extend_from_slice(nonce);
        header
    }
}

impl PassphraseCodec for AesGcmPbkdf2Codec {
    fn seal(&self, passphrase: &str, plaintext: &[u8]) -> Result<Vec<u8>, DomainError> {
        if passphrase.is_empty() {
            return Err(DomainError::ValidationError(
                "passphrase must not be empty".into(),
            ));
        }

        let mut rng = SysRng;
        let mut salt = [0u8; SALT_LEN];
        rng.try_fill_bytes(&mut salt)
            .map_err(|e| DomainError::StorageError(format!("rng failure: {e}")))?;
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rng.try_fill_bytes(&mut nonce_bytes)
            .map_err(|e| DomainError::StorageError(format!("rng failure: {e}")))?;

        let key = Self::derive_key(passphrase, &salt, PBKDF2_ITERATIONS)?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| DomainError::StorageError(format!("aes init failed: {e}")))?;
        let header = Self::build_header(&salt, &nonce_bytes);

        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext,
                    aad: &header,
                },
            )
            .map_err(|e| DomainError::StorageError(format!("aes encrypt failed: {e}")))?;

        let mut out = header;
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    fn open(&self, passphrase: &str, ciphertext: &[u8]) -> Result<Vec<u8>, DomainError> {
        if passphrase.is_empty() {
            return Err(DomainError::ValidationError(
                "passphrase must not be empty".into(),
            ));
        }
        if ciphertext.len() < HEADER_LEN + 16 {
            return Err(DomainError::ValidationError(
                "ciphertext too short to be a vortex account export".into(),
            ));
        }
        if &ciphertext[..MAGIC.len()] != MAGIC {
            return Err(DomainError::ValidationError(
                "not a vortex account export (magic mismatch)".into(),
            ));
        }
        let version = ciphertext[MAGIC.len()];
        if version != VERSION {
            return Err(DomainError::ValidationError(format!(
                "unsupported export version: {version} (expected {VERSION})"
            )));
        }

        let mut iter_bytes = [0u8; ITER_LEN];
        iter_bytes.copy_from_slice(&ciphertext[MAGIC.len() + 1..MAGIC.len() + 1 + ITER_LEN]);
        let iterations = u32::from_be_bytes(iter_bytes);
        if iterations < 1_000 {
            return Err(DomainError::ValidationError(
                "export header has implausibly low PBKDF2 iteration count".into(),
            ));
        }

        let salt_start = MAGIC.len() + 1 + ITER_LEN;
        let salt = &ciphertext[salt_start..salt_start + SALT_LEN];
        let nonce_start = salt_start + SALT_LEN;
        let nonce_bytes = &ciphertext[nonce_start..nonce_start + NONCE_LEN];
        let body = &ciphertext[HEADER_LEN..];

        let mut salt_arr = [0u8; SALT_LEN];
        salt_arr.copy_from_slice(salt);
        let mut nonce_arr = [0u8; NONCE_LEN];
        nonce_arr.copy_from_slice(nonce_bytes);

        let key = Self::derive_key(passphrase, &salt_arr, iterations)?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| DomainError::StorageError(format!("aes init failed: {e}")))?;
        let header = Self::build_header_with_iterations(&salt_arr, &nonce_arr, iterations);

        let nonce = Nonce::from_slice(&nonce_arr);
        cipher
            .decrypt(
                nonce,
                Payload {
                    msg: body,
                    aad: &header,
                },
            )
            .map_err(|_| {
                // GCM auth failures are indistinguishable from a wrong
                // passphrase by design — surface a single clear message
                // so the UI can route to "passphrase incorrect".
                DomainError::ValidationError("wrong passphrase or corrupted account export".into())
            })
    }
}

impl AesGcmPbkdf2Codec {
    fn build_header_with_iterations(
        salt: &[u8; SALT_LEN],
        nonce: &[u8; NONCE_LEN],
        iterations: u32,
    ) -> Vec<u8> {
        let mut header = Vec::with_capacity(HEADER_LEN);
        header.extend_from_slice(MAGIC);
        header.push(VERSION);
        header.extend_from_slice(&iterations.to_be_bytes());
        header.extend_from_slice(salt);
        header.extend_from_slice(nonce);
        header
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seal_open_round_trip_returns_original_plaintext() {
        let codec = AesGcmPbkdf2Codec::new();
        let plaintext = b"hello, world";
        let ciphertext = codec.seal("passw0rd", plaintext).unwrap();
        let recovered = codec.open("passw0rd", &ciphertext).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn test_seal_produces_unique_outputs_for_same_input() {
        let codec = AesGcmPbkdf2Codec::new();
        let a = codec.seal("k", b"plaintext").unwrap();
        let b = codec.seal("k", b"plaintext").unwrap();
        assert_ne!(a, b, "fresh salt+nonce → different ciphertext");
    }

    #[test]
    fn test_open_with_wrong_passphrase_returns_validation_error() {
        let codec = AesGcmPbkdf2Codec::new();
        let ct = codec.seal("right", b"secret").unwrap();
        let err = codec.open("wrong", &ct).unwrap_err();
        assert!(matches!(err, DomainError::ValidationError(_)));
    }

    #[test]
    fn test_open_rejects_short_input() {
        let codec = AesGcmPbkdf2Codec::new();
        let err = codec.open("k", b"short").unwrap_err();
        assert!(matches!(err, DomainError::ValidationError(_)));
    }

    #[test]
    fn test_open_rejects_wrong_magic() {
        let codec = AesGcmPbkdf2Codec::new();
        let mut ct = codec.seal("k", b"data").unwrap();
        ct[0] ^= 0xFF;
        let err = codec.open("k", &ct).unwrap_err();
        assert!(matches!(err, DomainError::ValidationError(ref m) if m.contains("magic")));
    }

    #[test]
    fn test_open_rejects_unsupported_version() {
        let codec = AesGcmPbkdf2Codec::new();
        let mut ct = codec.seal("k", b"data").unwrap();
        ct[MAGIC.len()] = 99;
        let err = codec.open("k", &ct).unwrap_err();
        assert!(matches!(err, DomainError::ValidationError(ref m) if m.contains("version")));
    }

    #[test]
    fn test_open_rejects_tampered_ciphertext_body() {
        let codec = AesGcmPbkdf2Codec::new();
        let mut ct = codec.seal("k", b"hello").unwrap();
        // Flip a bit in the encrypted body so GCM auth fails.
        let last = ct.len() - 1;
        ct[last] ^= 0x01;
        let err = codec.open("k", &ct).unwrap_err();
        assert!(matches!(err, DomainError::ValidationError(_)));
    }

    #[test]
    fn test_open_rejects_low_iteration_header() {
        let codec = AesGcmPbkdf2Codec::new();
        let mut ct = codec.seal("k", b"hello").unwrap();
        // Overwrite the iteration field with 0 (invalid by policy).
        let iter_offset = MAGIC.len() + 1;
        ct[iter_offset..iter_offset + ITER_LEN].copy_from_slice(&0u32.to_be_bytes());
        let err = codec.open("k", &ct).unwrap_err();
        assert!(matches!(err, DomainError::ValidationError(ref m) if m.contains("iteration")));
    }

    #[test]
    fn test_seal_rejects_empty_passphrase() {
        let codec = AesGcmPbkdf2Codec::new();
        let err = codec.seal("", b"data").unwrap_err();
        assert!(matches!(err, DomainError::ValidationError(_)));
    }
}
