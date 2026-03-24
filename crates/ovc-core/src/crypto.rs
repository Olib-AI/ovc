//! Cryptographic primitives for OVC repository encryption.
//!
//! OVC encrypts every repository as a single `.ovc` blob file. This module
//! provides key derivation (Argon2id), authenticated encryption
//! (XChaCha20-Poly1305), and secure random generation.

use chacha20poly1305::aead::Aead;
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

use crate::error::{CoreError, CoreResult};

/// Key derivation function algorithm identifier (for format header).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum KdfAlgorithm {
    /// Argon2id — memory-hard KDF, resistant to GPU/ASIC attacks.
    Argon2id = 1,
}

impl KdfAlgorithm {
    /// Creates a `KdfAlgorithm` from its `u8` discriminant.
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Argon2id),
            _ => None,
        }
    }
}

/// Cipher algorithm identifier (for format header).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum CipherAlgorithm {
    /// XChaCha20-Poly1305 — 256-bit key, 192-bit nonce, AEAD.
    XChaCha20Poly1305 = 1,
}

impl CipherAlgorithm {
    /// Creates a `CipherAlgorithm` from its `u8` discriminant.
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::XChaCha20Poly1305),
            _ => None,
        }
    }
}

/// An encrypted segment: nonce + ciphertext (including Poly1305 tag).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedSegment {
    /// The 24-byte `XChaCha20` nonce used for this segment.
    pub nonce: [u8; 24],
    /// The ciphertext including the 16-byte Poly1305 authentication tag.
    pub ciphertext: Vec<u8>,
}

/// Derives a 256-bit master key from a password using Argon2id.
///
/// # Parameters
/// - `password`: the user-supplied password
/// - `salt`: a 32-byte random salt (stored in the file header)
/// - `time_cost`: number of Argon2 iterations
/// - `memory_cost_kib`: memory usage in KiB
/// - `parallelism`: degree of parallelism
pub fn derive_master_key(
    password: &[u8],
    salt: &[u8; 32],
    time_cost: u32,
    memory_cost_kib: u32,
    parallelism: u8,
) -> CoreResult<Zeroizing<[u8; 32]>> {
    let params = argon2::Params::new(memory_cost_kib, time_cost, u32::from(parallelism), Some(32))
        .map_err(|e| CoreError::KeyDerivationFailed {
            reason: e.to_string(),
        })?;

    let argon = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut key = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(password, salt, key.as_mut())
        .map_err(|e| CoreError::KeyDerivationFailed {
            reason: e.to_string(),
        })?;

    Ok(key)
}

/// Encrypts a plaintext segment with XChaCha20-Poly1305.
///
/// A fresh 24-byte nonce is generated for each call. The `aad` (additional
/// authenticated data) is integrity-checked but not encrypted.
pub fn encrypt_segment(
    key: &[u8; 32],
    plaintext: &[u8],
    aad: &[u8],
) -> CoreResult<EncryptedSegment> {
    let nonce_bytes = generate_nonce();
    let nonce = XNonce::from(nonce_bytes);

    let cipher =
        XChaCha20Poly1305::new_from_slice(key).map_err(|e| CoreError::EncryptionFailed {
            reason: e.to_string(),
        })?;

    let payload = chacha20poly1305::aead::Payload {
        msg: plaintext,
        aad,
    };

    let ciphertext = cipher
        .encrypt(&nonce, payload)
        .map_err(|e| CoreError::EncryptionFailed {
            reason: e.to_string(),
        })?;

    Ok(EncryptedSegment {
        nonce: nonce_bytes,
        ciphertext,
    })
}

/// Decrypts a ciphertext segment with XChaCha20-Poly1305.
///
/// Returns the plaintext on success. Fails if the key, nonce, or AAD
/// do not match those used during encryption (authentication failure).
pub fn decrypt_segment(
    key: &[u8; 32],
    nonce: &[u8; 24],
    ciphertext: &[u8],
    aad: &[u8],
) -> CoreResult<Vec<u8>> {
    let xnonce = XNonce::from(*nonce);
    let cipher =
        XChaCha20Poly1305::new_from_slice(key).map_err(|e| CoreError::DecryptionFailed {
            reason: e.to_string(),
        })?;

    let payload = chacha20poly1305::aead::Payload {
        msg: ciphertext,
        aad,
    };

    cipher
        .decrypt(&xnonce, payload)
        .map_err(|e| CoreError::DecryptionFailed {
            reason: e.to_string(),
        })
}

/// Generates a cryptographically secure random 256-bit key.
///
/// The returned key is wrapped in [`Zeroizing`] to ensure it is securely
/// erased from memory when dropped.
#[must_use]
pub fn generate_key() -> Zeroizing<[u8; 32]> {
    let mut key = Zeroizing::new([0u8; 32]);
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, key.as_mut());
    key
}

/// Generates a cryptographically secure random 256-bit salt.
#[must_use]
pub fn generate_salt() -> [u8; 32] {
    let mut salt = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut salt);
    salt
}

/// Generates a cryptographically secure random 192-bit nonce.
#[must_use]
pub fn generate_nonce() -> [u8; 24] {
    let mut nonce = [0u8; 24];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce);
    nonce
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_round_trip() {
        let key = generate_key();
        let plaintext = b"confidential data";
        let aad = b"segment-001";

        let encrypted = encrypt_segment(&key, plaintext, aad).unwrap();
        let decrypted =
            decrypt_segment(&key, &encrypted.nonce, &encrypted.ciphertext, aad).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_key_fails() {
        let key = generate_key();
        let wrong_key = generate_key();
        let encrypted = encrypt_segment(&key, b"secret", b"").unwrap();

        let result = decrypt_segment(&wrong_key, &encrypted.nonce, &encrypted.ciphertext, b"");
        assert!(result.is_err());
    }

    #[test]
    fn wrong_aad_fails() {
        let key = generate_key();
        let encrypted = encrypt_segment(&key, b"secret", b"correct-aad").unwrap();

        let result = decrypt_segment(&key, &encrypted.nonce, &encrypted.ciphertext, b"wrong-aad");
        assert!(result.is_err());
    }

    #[test]
    fn key_derivation_works() {
        let password = b"hunter2";
        let salt = generate_salt();
        // Use minimal params for test speed.
        let key = derive_master_key(password, &salt, 1, 64, 1).unwrap();
        assert_eq!(key.len(), 32);

        // Deterministic: same inputs produce the same key.
        let key2 = derive_master_key(password, &salt, 1, 64, 1).unwrap();
        assert_eq!(*key, *key2);

        // Different salt produces a different key.
        let salt2 = generate_salt();
        let key3 = derive_master_key(password, &salt2, 1, 64, 1).unwrap();
        assert_ne!(*key, *key3);
    }
}
