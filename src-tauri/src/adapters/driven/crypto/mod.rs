//! Cryptographic adapters.
//!
//! Implements domain-level cryptographic ports (passphrase-keyed
//! authenticated encryption today, more as the import / export and
//! plugin-signing surfaces grow).

mod aes_gcm_codec;

pub use aes_gcm_codec::AesGcmPbkdf2Codec;
