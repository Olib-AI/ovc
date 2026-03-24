//! Encrypted secrets vault for actions.
//!
//! Secrets are stored in `.ovc/secrets.enc` as XChaCha20-Poly1305
//! authenticated-encrypted JSON. Each save generates a fresh 24-byte
//! nonce prepended to the ciphertext. The encryption key is derived
//! from the caller-supplied passphrase via Argon2id (v2 format) or
//! SHA-256 (v1 legacy format, read-only for backward compatibility).
//!
//! File format versions:
//!   - `OVCS` (4 bytes): v1 legacy — SHA-256 KDF, nonce(24), ciphertext
//!   - `OVCA` (4 bytes): v2 current — Argon2id KDF, salt(32), nonce(24), ciphertext
//!
//! When no passphrase is available, the vault falls back to
//! base64 encoding for backward compatibility, but a warning is
//! logged. Secrets are injected as environment variables with the
//! `OVC_SECRET_` prefix.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{ActionsError, ActionsResult};

/// Magic header for v1 legacy format (SHA-256 KDF). Read-only; new writes use v2.
const LEGACY_MAGIC: &[u8; 4] = b"OVCS";

/// Magic header for v2 format (Argon2id KDF).
const ENCRYPTED_MAGIC: &[u8; 4] = b"OVCA";

/// Nonce size for XChaCha20-Poly1305 (192-bit / 24 bytes).
const NONCE_SIZE: usize = 24;

/// Salt size for Argon2id (256-bit / 32 bytes).
const SALT_SIZE: usize = 32;

/// Argon2id parameters for the v2 secrets vault KDF.
///
/// These match the rest of the OVC codebase's Argon2id usage.
const ARGON2_M_COST: u32 = 65_536; // 64 MiB
const ARGON2_T_COST: u32 = 2;
const ARGON2_P_COST: u32 = 1;

/// Derives a 256-bit key from a passphrase using Argon2id (v2 format).
///
/// Returns `Err` if the argon2 library rejects the parameters (should not
/// happen with the compile-time constants above).
fn derive_key_argon2(passphrase: &str, salt: &[u8; SALT_SIZE]) -> ActionsResult<[u8; 32]> {
    let params = argon2::Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(32))
        .map_err(|e| ActionsError::Config {
            reason: format!("argon2 params error: {e}"),
        })?;
    let argon = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut key = [0u8; 32];
    argon
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| ActionsError::Config {
            reason: format!("argon2 key derivation failed: {e}"),
        })?;
    Ok(key)
}

/// Derives a 256-bit encryption key from a passphrase using SHA-256.
///
/// Used only for reading v1 legacy vaults. New vaults always use Argon2id.
fn derive_key_sha256_legacy(passphrase: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"ovc-secrets-vault-v1:");
    hasher.update(passphrase.as_bytes());
    hasher.finalize().into()
}

/// In-memory representation of the secrets vault.
///
/// `Debug` is manually implemented to avoid leaking secret values in log
/// output, panic messages, or error reports.
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct SecretsVault {
    /// Secret name to encoded value mapping.
    secrets: BTreeMap<String, String>,
}

impl std::fmt::Debug for SecretsVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretsVault")
            .field(
                "secrets",
                &format_args!("[{} entries, values redacted]", self.secrets.len()),
            )
            .finish()
    }
}

impl SecretsVault {
    /// Path to the secrets file relative to the repository root.
    fn secrets_path(repo_root: &Path) -> PathBuf {
        repo_root.join(".ovc").join("secrets.enc")
    }

    /// Load secrets from disk. Returns an empty vault if the file does not exist.
    ///
    /// If `passphrase` is provided, attempts authenticated decryption first.
    /// Falls back to legacy base64 decoding for vaults created before encryption
    /// was added.
    pub fn load(repo_root: &Path) -> ActionsResult<Self> {
        Self::load_with_passphrase(repo_root, None)
    }

    /// Load secrets with an optional encryption passphrase.
    pub fn load_with_passphrase(repo_root: &Path, passphrase: Option<&str>) -> ActionsResult<Self> {
        let path = Self::secrets_path(repo_root);
        if !path.is_file() {
            return Ok(Self::default());
        }
        let raw = std::fs::read(&path)?;

        // Resolve passphrase from argument or env var once; bind to a local so
        // the String outlives the borrows taken by as_deref() below.
        let env_passphrase = std::env::var("OVC_KEY_PASSPHRASE").ok();
        let resolved_passphrase = passphrase.or(env_passphrase.as_deref());

        // v2: Argon2id format — OVCA || salt(32) || nonce(24) || ciphertext+tag
        let v2_min_len = ENCRYPTED_MAGIC.len() + SALT_SIZE + NONCE_SIZE;
        if raw.len() > v2_min_len && raw.starts_with(ENCRYPTED_MAGIC) {
            let pw = resolved_passphrase.ok_or_else(|| ActionsError::Config {
                reason: "secrets vault is encrypted but no passphrase provided \
                         (set OVC_KEY_PASSPHRASE or pass --passphrase)"
                    .to_owned(),
            })?;

            let salt_start = ENCRYPTED_MAGIC.len();
            let nonce_start = salt_start + SALT_SIZE;
            let ct_start = nonce_start + NONCE_SIZE;

            let salt: &[u8; SALT_SIZE] = raw[salt_start..nonce_start]
                .try_into()
                .expect("salt slice is exactly SALT_SIZE");
            let nonce_bytes: &[u8; NONCE_SIZE] = raw[nonce_start..ct_start]
                .try_into()
                .expect("nonce slice is exactly NONCE_SIZE");
            let ciphertext = &raw[ct_start..];

            let key = derive_key_argon2(pw, salt)?;
            let cipher = XChaCha20Poly1305::new((&key).into());
            let plaintext = cipher
                .decrypt(nonce_bytes.into(), ciphertext)
                .map_err(|_| ActionsError::Config {
                    reason: "failed to decrypt secrets vault — wrong passphrase?".to_owned(),
                })?;

            let vault: Self =
                serde_json::from_slice(&plaintext).map_err(|e| ActionsError::Config {
                    reason: format!("failed to parse decrypted secrets: {e}"),
                })?;
            return Ok(vault);
        }

        // v1 legacy: OVCS || nonce(24) || ciphertext+tag (SHA-256 KDF).
        let v1_min_len = LEGACY_MAGIC.len() + NONCE_SIZE;
        if raw.len() > v1_min_len && raw.starts_with(LEGACY_MAGIC) {
            let pw = resolved_passphrase.ok_or_else(|| ActionsError::Config {
                reason: "secrets vault is encrypted but no passphrase provided \
                         (set OVC_KEY_PASSPHRASE or pass --passphrase)"
                    .to_owned(),
            })?;

            let nonce_start = LEGACY_MAGIC.len();
            let ct_start = nonce_start + NONCE_SIZE;
            let nonce_bytes: &[u8; NONCE_SIZE] = raw[nonce_start..ct_start]
                .try_into()
                .expect("nonce slice is exactly NONCE_SIZE");
            let ciphertext = &raw[ct_start..];

            let key = derive_key_sha256_legacy(pw);
            let cipher = XChaCha20Poly1305::new((&key).into());
            let plaintext = cipher
                .decrypt(nonce_bytes.into(), ciphertext)
                .map_err(|_| ActionsError::Config {
                    reason: "failed to decrypt legacy secrets vault — wrong passphrase?".to_owned(),
                })?;

            let vault: Self =
                serde_json::from_slice(&plaintext).map_err(|e| ActionsError::Config {
                    reason: format!("failed to parse decrypted legacy secrets: {e}"),
                })?;
            return Ok(vault);
        }

        // Oldest legacy: base64-encoded JSON (unencrypted).
        let content = String::from_utf8_lossy(&raw);
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(content.trim().as_bytes())
            .map_err(|e| ActionsError::Config {
                reason: format!("failed to decode secrets: {e}"),
            })?;
        let vault: Self = serde_json::from_slice(&decoded).map_err(|e| ActionsError::Config {
            reason: format!("failed to parse secrets: {e}"),
        })?;
        Ok(vault)
    }

    /// Save secrets to disk with XChaCha20-Poly1305 authenticated encryption.
    ///
    /// If `passphrase` is `None`, falls back to the `OVC_KEY_PASSPHRASE` env
    /// var. If neither is available, saves in legacy base64 format with a
    /// warning.
    pub fn save(&self, repo_root: &Path) -> ActionsResult<()> {
        self.save_with_passphrase(repo_root, None)
    }

    /// Save secrets with an optional encryption passphrase.
    pub fn save_with_passphrase(
        &self,
        repo_root: &Path,
        passphrase: Option<&str>,
    ) -> ActionsResult<()> {
        let path = Self::secrets_path(repo_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_vec(self).map_err(|e| ActionsError::Config {
            reason: format!("failed to serialize secrets: {e}"),
        })?;

        let passphrase = passphrase
            .map(String::from)
            .or_else(|| std::env::var("OVC_KEY_PASSPHRASE").ok());

        let output = if let Some(ref pw) = passphrase {
            // v2 encrypted format: OVCA || salt(32) || nonce(24) || ciphertext+tag
            let mut salt = [0u8; SALT_SIZE];
            OsRng.fill_bytes(&mut salt);
            let mut nonce_bytes = [0u8; NONCE_SIZE];
            OsRng.fill_bytes(&mut nonce_bytes);
            let key = derive_key_argon2(pw, &salt)?;
            let cipher = XChaCha20Poly1305::new((&key).into());
            let ciphertext = cipher
                .encrypt((&nonce_bytes).into(), json.as_ref())
                .map_err(|e| ActionsError::Config {
                    reason: format!("failed to encrypt secrets: {e}"),
                })?;
            let mut buf = Vec::with_capacity(
                ENCRYPTED_MAGIC.len() + SALT_SIZE + NONCE_SIZE + ciphertext.len(),
            );
            buf.extend_from_slice(ENCRYPTED_MAGIC);
            buf.extend_from_slice(&salt);
            buf.extend_from_slice(&nonce_bytes);
            buf.extend_from_slice(&ciphertext);
            buf
        } else {
            // Legacy base64 fallback (no passphrase available).
            eprintln!(
                "warning: saving secrets without encryption \
                 (set OVC_KEY_PASSPHRASE to enable encryption)"
            );
            base64::engine::general_purpose::STANDARD
                .encode(&json)
                .into_bytes()
        };

        std::fs::write(&path, &output)?;

        // Restrict file permissions to owner-only on Unix platforms.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&path, perms)?;
        }

        Ok(())
    }

    /// Set a secret value, overwriting any previous value with the same name.
    pub fn set(&mut self, name: String, value: String) {
        self.secrets.insert(name, value);
    }

    /// Remove a secret by name. Returns `true` if the secret existed.
    pub fn remove(&mut self, name: &str) -> bool {
        self.secrets.remove(name).is_some()
    }

    /// List secret names (values are never exposed through this method).
    #[must_use]
    pub fn list_names(&self) -> Vec<&str> {
        self.secrets.keys().map(String::as_str).collect()
    }

    /// Get all secrets as environment variable pairs prefixed with `OVC_SECRET_`.
    #[must_use]
    pub fn as_env_vars(&self) -> BTreeMap<String, String> {
        self.secrets
            .iter()
            .map(|(k, v)| (format!("OVC_SECRET_{}", k.to_uppercase()), v.clone()))
            .collect()
    }
}
