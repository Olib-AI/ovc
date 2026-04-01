//! SSH-style key pair management for OVC repository encryption.
//!
//! Provides Ed25519 signing keys and X25519 encryption keys stored in
//! `~/.ssh/ovc/`. Private keys are encrypted at rest with Argon2id +
//! XChaCha20-Poly1305. Multiple key slots allow multi-user access to a
//! single repository.
//!
//! # Key types
//!
//! - **Ed25519** -- signing and identity (256-bit security)
//! - **X25519** -- Diffie-Hellman key exchange for sealing segment keys

use std::collections::HashMap;
use std::fmt::{self, Write as _};
use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::SigningKey;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};
use zeroize::Zeroizing;

use crate::crypto;
use crate::error::{CoreError, CoreResult};
use crate::object::Commit;
use crate::serialize;

/// Argon2id parameters for encrypting private keys at rest.
/// Tuned for interactive use (< 1 second on modern hardware).
const KEY_KDF_TIME_COST: u32 = 3;
const KEY_KDF_MEMORY_COST_KIB: u32 = 65536;
const KEY_KDF_PARALLELISM: u8 = 1;

/// PEM-like delimiters for key file formats.
const PRIVATE_KEY_BEGIN: &str = "-----BEGIN OVC PRIVATE KEY-----";
const PRIVATE_KEY_END: &str = "-----END OVC PRIVATE KEY-----";
const PUBLIC_KEY_BEGIN: &str = "-----BEGIN OVC PUBLIC KEY-----";
const PUBLIC_KEY_END: &str = "-----END OVC PUBLIC KEY-----";

/// Maximum header key length to distinguish headers from base64 data.
const MAX_HEADER_KEY_LEN: usize = 32;

/// Identity associated with a signing key.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct KeyIdentity {
    /// Display name of the key holder.
    pub name: String,
    /// Email address of the key holder.
    pub email: String,
}

impl std::fmt::Display for KeyIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} <{}>", self.name, self.email)
    }
}

impl KeyIdentity {
    /// Parses an identity string of the form `"Name <email>"`.
    pub fn parse(s: &str) -> CoreResult<Self> {
        let s = s.trim();
        let lt_pos = s.find('<').ok_or_else(|| CoreError::FormatError {
            reason: format!("invalid identity format, expected 'Name <email>', got '{s}'"),
        })?;
        let name = s[..lt_pos].trim().to_owned();
        let email = s[lt_pos + 1..].trim_end_matches('>').trim().to_owned();
        if name.is_empty() || email.is_empty() {
            return Err(CoreError::FormatError {
                reason: format!("invalid identity format, expected 'Name <email>', got '{s}'"),
            });
        }
        Ok(Self { name, email })
    }
}

/// Result of verifying a commit signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyResult {
    /// Signature verified against a known authorized key.
    Verified {
        /// Fingerprint of the key that verified the signature.
        fingerprint: String,
        /// Identity attached to the verifying key, if available.
        identity: Option<KeyIdentity>,
    },
    /// Signature present but does not match any authorized key.
    Unverified {
        /// Reason verification failed.
        reason: String,
    },
    /// No signature present on the commit.
    NotSigned,
}

/// Verifies the Ed25519 signature on a commit against a set of authorized public keys.
///
/// The `serialized_for_hash` parameter must be the canonical commit bytes
/// produced by [`crate::serialize::serialize_object`] (which excludes the
/// signature field).
#[must_use]
pub fn verify_commit_signature(
    commit: &Commit,
    serialized_for_hash: &[u8],
    authorized_keys: &[OvcPublicKey],
) -> VerifyResult {
    use ed25519_dalek::Verifier;

    let Some(ref sig_bytes) = commit.signature else {
        return VerifyResult::NotSigned;
    };

    let sig_array: [u8; 64] = match sig_bytes.as_slice().try_into() {
        Ok(arr) => arr,
        Err(_) => {
            return VerifyResult::Unverified {
                reason: format!(
                    "invalid signature length: expected 64 bytes, got {}",
                    sig_bytes.len()
                ),
            };
        }
    };

    let signature = ed25519_dalek::Signature::from_bytes(&sig_array);

    for key in authorized_keys {
        if key
            .signing_public
            .verify(serialized_for_hash, &signature)
            .is_ok()
        {
            return VerifyResult::Verified {
                fingerprint: key.fingerprint.clone(),
                identity: key.identity.clone(),
            };
        }
    }

    VerifyResult::Unverified {
        reason: "signature does not match any authorized key".into(),
    }
}

/// Convenience wrapper that serializes the commit and then verifies.
///
/// Returns `VerifyResult::Unverified` if serialization fails.
#[must_use]
pub fn verify_commit(commit: &Commit, authorized_keys: &[OvcPublicKey]) -> VerifyResult {
    let obj = crate::object::Object::Commit(commit.clone());
    let Ok(serialized) = serialize::serialize_object(&obj) else {
        return VerifyResult::Unverified {
            reason: "failed to serialize commit for verification".into(),
        };
    };
    verify_commit_signature(commit, &serialized, authorized_keys)
}

/// A complete OVC key pair: Ed25519 signing + X25519 encryption.
///
/// The X25519 secret is derived deterministically from the Ed25519 signing
/// key via SHA-256 clamping, so only one secret needs to be stored.
pub struct OvcKeyPair {
    /// Ed25519 signing key (private).
    signing_key: SigningKey,
    /// Ed25519 verifying key (public).
    signing_public: ed25519_dalek::VerifyingKey,
    /// X25519 static secret (derived from Ed25519 seed).
    encryption_secret: StaticSecret,
    /// X25519 public key.
    encryption_public: X25519PublicKey,
    /// Fingerprint string: `SHA256:<base64>`.
    fingerprint: String,
    /// Optional identity associated with this key pair.
    identity: Option<KeyIdentity>,
}

impl fmt::Debug for OvcKeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OvcKeyPair")
            .field("fingerprint", &self.fingerprint)
            .field("identity", &self.identity)
            .field("signing_key", &"[REDACTED]")
            .field("signing_public", &self.signing_public)
            .field("encryption_secret", &"[REDACTED]")
            .field("encryption_public", &self.encryption_public)
            .finish()
    }
}

/// A public-only OVC key (for adding authorized users to a repository).
#[derive(Debug, Clone)]
pub struct OvcPublicKey {
    /// Ed25519 verifying key.
    pub signing_public: ed25519_dalek::VerifyingKey,
    /// X25519 public key.
    pub encryption_public: X25519PublicKey,
    /// Fingerprint string: `SHA256:<base64>`.
    pub fingerprint: String,
    /// Optional identity associated with this key.
    pub identity: Option<KeyIdentity>,
}

/// A sealed (encrypted) copy of a segment encryption key for one recipient.
///
/// Uses X25519 ECDH: an ephemeral keypair is generated for each seal
/// operation, the shared secret is derived, and the segment key is
/// encrypted with XChaCha20-Poly1305 keyed by the HKDF of the shared
/// secret.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SealedKey {
    /// Fingerprint of the recipient whose private key can unseal this slot.
    pub recipient_fingerprint: String,
    /// Ephemeral X25519 public key (generated per seal operation).
    pub ephemeral_public: [u8; 32],
    /// XChaCha20-Poly1305 encrypted segment key (32 bytes + 16-byte tag).
    pub encrypted_key: Vec<u8>,
    /// Nonce used for the XChaCha20-Poly1305 encryption.
    pub nonce: [u8; 24],
}

impl OvcKeyPair {
    /// Generates a fresh key pair using the OS cryptographic RNG.
    #[must_use]
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
        Self::from_signing_key_with_identity(signing_key, None)
    }

    /// Generates a fresh key pair with an associated identity.
    #[must_use]
    pub fn generate_with_identity(identity: KeyIdentity) -> Self {
        let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
        Self::from_signing_key_with_identity(signing_key, Some(identity))
    }

    /// Reconstructs an `OvcKeyPair` from an Ed25519 signing key with optional identity.
    ///
    /// The X25519 secret is derived deterministically from the Ed25519 seed.
    fn from_signing_key_with_identity(
        signing_key: SigningKey,
        identity: Option<KeyIdentity>,
    ) -> Self {
        let signing_public = signing_key.verifying_key();
        let encryption_secret = ed25519_to_x25519_secret(&signing_key);
        let encryption_public = X25519PublicKey::from(&encryption_secret);
        let fingerprint = compute_fingerprint(&signing_public, &encryption_public);

        Self {
            signing_key,
            signing_public,
            encryption_secret,
            encryption_public,
            fingerprint,
            identity,
        }
    }

    /// Returns the fingerprint string (`SHA256:<base64>`).
    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// Returns the Ed25519 signing key.
    #[must_use]
    pub const fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }

    /// Returns the Ed25519 verifying (public) key.
    #[must_use]
    pub const fn signing_public(&self) -> &ed25519_dalek::VerifyingKey {
        &self.signing_public
    }

    /// Returns the X25519 encryption public key.
    #[must_use]
    pub const fn encryption_public(&self) -> &X25519PublicKey {
        &self.encryption_public
    }

    /// Returns the identity associated with this key pair, if any.
    #[must_use]
    pub const fn identity(&self) -> Option<&KeyIdentity> {
        self.identity.as_ref()
    }

    /// Sets the identity associated with this key pair.
    pub fn set_identity(&mut self, identity: Option<KeyIdentity>) {
        self.identity = identity;
    }

    /// Extracts the public key portion.
    #[must_use]
    pub fn public_key(&self) -> OvcPublicKey {
        OvcPublicKey {
            signing_public: self.signing_public,
            encryption_public: self.encryption_public,
            fingerprint: self.fingerprint.clone(),
            identity: self.identity.clone(),
        }
    }

    /// Saves the private key to disk, encrypted with a passphrase.
    ///
    /// Format:
    /// ```text
    /// -----BEGIN OVC PRIVATE KEY-----
    /// Version: 1
    /// Algorithm: Ed25519+X25519
    /// KDF: Argon2id
    /// Salt: <base64>
    /// Nonce: <base64>
    /// <base64 encoded encrypted key material>
    /// -----END OVC PRIVATE KEY-----
    /// ```
    pub fn save_private(&self, path: &Path, passphrase: &[u8]) -> CoreResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let output = self.format_private_pem(passphrase)?;
        fs::write(path, &output)?;

        // Set restrictive permissions on Unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    /// Saves the public key to disk in plaintext PEM-like format.
    pub fn save_public(&self, path: &Path) -> CoreResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let output = self.format_public_pem();
        fs::write(path, output)?;
        Ok(())
    }

    /// Loads and decrypts a private key from disk.
    pub fn load_private(path: &Path, passphrase: &[u8]) -> CoreResult<Self> {
        let contents = fs::read_to_string(path)?;
        Self::parse_private(&contents, passphrase)
    }

    /// Parses a private key from its PEM-like text representation.
    fn parse_private(text: &str, passphrase: &[u8]) -> CoreResult<Self> {
        let body = extract_pem_body(text, PRIVATE_KEY_BEGIN, PRIVATE_KEY_END)?;
        let headers = parse_headers(body);

        let version = headers
            .get("Version")
            .ok_or_else(|| CoreError::FormatError {
                reason: "missing Version header in private key".into(),
            })?;
        if version != "1" {
            return Err(CoreError::FormatError {
                reason: format!("unsupported private key version: {version}"),
            });
        }

        let salt_b64 = headers.get("Salt").ok_or_else(|| CoreError::FormatError {
            reason: "missing Salt header in private key".into(),
        })?;
        let nonce_b64 = headers.get("Nonce").ok_or_else(|| CoreError::FormatError {
            reason: "missing Nonce header in private key".into(),
        })?;

        let salt_bytes = BASE64
            .decode(salt_b64)
            .map_err(|e| CoreError::FormatError {
                reason: format!("invalid Salt base64: {e}"),
            })?;
        let salt: [u8; 32] = salt_bytes.try_into().map_err(|_| CoreError::FormatError {
            reason: "Salt must be 32 bytes".into(),
        })?;

        let nonce_bytes = BASE64
            .decode(nonce_b64)
            .map_err(|e| CoreError::FormatError {
                reason: format!("invalid Nonce base64: {e}"),
            })?;
        let nonce: [u8; 24] = nonce_bytes.try_into().map_err(|_| CoreError::FormatError {
            reason: "Nonce must be 24 bytes".into(),
        })?;

        let data_b64 = extract_data_after_headers(body)?;
        let ciphertext = BASE64
            .decode(data_b64.trim())
            .map_err(|e| CoreError::FormatError {
                reason: format!("invalid key data base64: {e}"),
            })?;

        let derived = crypto::derive_master_key(
            passphrase,
            &salt,
            KEY_KDF_TIME_COST,
            KEY_KDF_MEMORY_COST_KIB,
            KEY_KDF_PARALLELISM,
        )?;

        let seed_bytes =
            crypto::decrypt_segment(&derived, &nonce, &ciphertext, b"ovc-private-key")?;

        let seed: [u8; 32] = seed_bytes.try_into().map_err(|_| CoreError::FormatError {
            reason: "decrypted key material must be 32 bytes".into(),
        })?;

        let identity = headers
            .get("Identity")
            .and_then(|v| KeyIdentity::parse(v).ok());

        let signing_key = SigningKey::from_bytes(&seed);
        Ok(Self::from_signing_key_with_identity(signing_key, identity))
    }

    /// Exports the key pair as a single text block suitable for secure notes
    /// in password managers (Bitwarden, 1Password, etc.).
    ///
    /// The exported block contains both the encrypted private key and the
    /// public key, separated by a blank line.
    pub fn export_for_password_manager(&self, passphrase: &[u8]) -> CoreResult<String> {
        let mut output = self.format_private_pem(passphrase)?;
        output.push('\n');
        output.push_str(&self.format_public_pem());
        Ok(output)
    }

    /// Imports a key pair from a password manager export text block.
    pub fn import_from_password_manager(text: &str, passphrase: &[u8]) -> CoreResult<Self> {
        Self::parse_private(text, passphrase)
    }

    /// Formats the private key as an encrypted PEM-like string.
    fn format_private_pem(&self, passphrase: &[u8]) -> CoreResult<String> {
        let salt = crypto::generate_salt();
        let derived = crypto::derive_master_key(
            passphrase,
            &salt,
            KEY_KDF_TIME_COST,
            KEY_KDF_MEMORY_COST_KIB,
            KEY_KDF_PARALLELISM,
        )?;

        let seed = self.signing_key.to_bytes();
        let encrypted = crypto::encrypt_segment(&derived, &seed, b"ovc-private-key")?;

        let mut output = String::new();
        output.push_str(PRIVATE_KEY_BEGIN);
        output.push('\n');
        output.push_str("Version: 1\n");
        output.push_str("Algorithm: Ed25519+X25519\n");
        output.push_str("KDF: Argon2id\n");
        let _ = writeln!(output, "Salt: {}", BASE64.encode(salt));
        let _ = writeln!(output, "Nonce: {}", BASE64.encode(encrypted.nonce));
        if let Some(ref id) = self.identity {
            let _ = writeln!(output, "Identity: {} <{}>", id.name, id.email);
        }
        output.push_str(&BASE64.encode(&encrypted.ciphertext));
        output.push('\n');
        output.push_str(PRIVATE_KEY_END);
        output.push('\n');

        Ok(output)
    }

    /// Formats the public key as a PEM-like string.
    fn format_public_pem(&self) -> String {
        let pub_bytes = encode_public_bytes(&self.signing_public, &self.encryption_public);

        let mut output = String::new();
        output.push_str(PUBLIC_KEY_BEGIN);
        output.push('\n');
        output.push_str("Version: 1\n");
        output.push_str("Algorithm: Ed25519+X25519\n");
        let _ = writeln!(output, "Fingerprint: {}", self.fingerprint);
        if let Some(ref id) = self.identity {
            let _ = writeln!(output, "Identity: {} <{}>", id.name, id.email);
        }
        output.push_str(&BASE64.encode(pub_bytes));
        output.push('\n');
        output.push_str(PUBLIC_KEY_END);
        output.push('\n');

        output
    }
}

impl OvcPublicKey {
    /// Loads a public key from a PEM-like file.
    pub fn load(path: &Path) -> CoreResult<Self> {
        let contents = fs::read_to_string(path)?;
        Self::parse(&contents)
    }

    /// Parses a public key from its PEM-like text representation.
    pub fn parse(text: &str) -> CoreResult<Self> {
        let body = extract_pem_body(text, PUBLIC_KEY_BEGIN, PUBLIC_KEY_END)?;
        let headers = parse_headers(body);

        let version = headers
            .get("Version")
            .ok_or_else(|| CoreError::FormatError {
                reason: "missing Version header in public key".into(),
            })?;
        if version != "1" {
            return Err(CoreError::FormatError {
                reason: format!("unsupported public key version: {version}"),
            });
        }

        let data_b64 = extract_data_after_headers(body)?;
        let pub_bytes = BASE64
            .decode(data_b64.trim())
            .map_err(|e| CoreError::FormatError {
                reason: format!("invalid public key base64: {e}"),
            })?;

        if pub_bytes.len() != 64 {
            return Err(CoreError::FormatError {
                reason: format!(
                    "public key material must be 64 bytes (32 Ed25519 + 32 X25519), got {}",
                    pub_bytes.len()
                ),
            });
        }

        let ed_bytes: [u8; 32] = pub_bytes[..32].try_into().expect("checked length above");
        let x_bytes: [u8; 32] = pub_bytes[32..].try_into().expect("checked length above");

        let signing_public = ed25519_dalek::VerifyingKey::from_bytes(&ed_bytes).map_err(|e| {
            CoreError::FormatError {
                reason: format!("invalid Ed25519 public key: {e}"),
            }
        })?;

        let encryption_public = X25519PublicKey::from(x_bytes);
        let fingerprint = compute_fingerprint(&signing_public, &encryption_public);
        let identity = headers
            .get("Identity")
            .and_then(|v| KeyIdentity::parse(v).ok());

        Ok(Self {
            signing_public,
            encryption_public,
            fingerprint,
            identity,
        })
    }
}

/// Seals (encrypts) a segment encryption key for a specific recipient.
///
/// Uses X25519 ephemeral ECDH: generates a fresh ephemeral keypair,
/// computes the shared secret with the recipient's public key, derives
/// an encryption key via SHA-256, and encrypts the segment key with
/// XChaCha20-Poly1305.
pub fn seal_key(segment_key: &[u8; 32], recipient: &OvcPublicKey) -> CoreResult<SealedKey> {
    let ephemeral_secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
    let ephemeral_public = X25519PublicKey::from(&ephemeral_secret);

    let shared_secret = ephemeral_secret.diffie_hellman(&recipient.encryption_public);
    let encryption_key = derive_seal_key(shared_secret.as_bytes(), ephemeral_public.as_bytes());

    let encrypted = crypto::encrypt_segment(&encryption_key, segment_key, b"ovc-sealed-key")?;

    Ok(SealedKey {
        recipient_fingerprint: recipient.fingerprint.clone(),
        ephemeral_public: *ephemeral_public.as_bytes(),
        encrypted_key: encrypted.ciphertext,
        nonce: encrypted.nonce,
    })
}

/// Unseals (decrypts) a segment encryption key using the recipient's private key.
pub fn unseal_key(sealed: &SealedKey, keypair: &OvcKeyPair) -> CoreResult<[u8; 32]> {
    let ephemeral_public = X25519PublicKey::from(sealed.ephemeral_public);
    let shared_secret = keypair.encryption_secret.diffie_hellman(&ephemeral_public);
    let encryption_key = derive_seal_key(shared_secret.as_bytes(), &sealed.ephemeral_public);

    let plaintext = crypto::decrypt_segment(
        &encryption_key,
        &sealed.nonce,
        &sealed.encrypted_key,
        b"ovc-sealed-key",
    )?;

    let key: [u8; 32] = plaintext
        .try_into()
        .map_err(|_| CoreError::DecryptionFailed {
            reason: "unsealed key must be exactly 32 bytes".into(),
        })?;

    Ok(key)
}

// ── Key directory management ──────────────────────────────────────────

/// Returns the default OVC key directory: `~/.ssh/ovc/`.
///
/// Returns `None` if the home directory cannot be determined.
#[must_use]
pub fn ovc_keys_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".ssh").join("ovc"))
}

/// Lists all key pairs found in `~/.ssh/ovc/`.
///
/// Returns a list of `(name, fingerprint, public_key_path)` tuples.
pub fn list_keys() -> CoreResult<Vec<(String, String, PathBuf)>> {
    let dir = ovc_keys_dir().ok_or_else(|| CoreError::Config {
        reason: "cannot determine home directory for key storage".into(),
    })?;

    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut keys = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "pub")
            && let Ok(pubkey) = OvcPublicKey::load(&path)
        {
            let name = path.file_stem().map_or_else(
                || String::from("unknown"),
                |s| s.to_string_lossy().into_owned(),
            );
            keys.push((name, pubkey.fingerprint.clone(), path));
        }
    }

    keys.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(keys)
}

/// Finds a key by fingerprint (or fingerprint prefix) or by name.
///
/// Searches `~/.ssh/ovc/` for a matching `.pub` file.
pub fn find_key(query: &str) -> CoreResult<Option<PathBuf>> {
    let dir = ovc_keys_dir().ok_or_else(|| CoreError::Config {
        reason: "cannot determine home directory for key storage".into(),
    })?;

    if !dir.exists() {
        return Ok(None);
    }

    // First try exact name match.
    let by_name = dir.join(format!("{query}.pub"));
    if by_name.exists() {
        return Ok(Some(by_name));
    }

    // Then try fingerprint prefix match.
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "pub")
            && let Ok(pubkey) = OvcPublicKey::load(&path)
            && (pubkey.fingerprint == query || pubkey.fingerprint.starts_with(query))
        {
            return Ok(Some(path));
        }
    }

    Ok(None)
}

/// Finds the private key file corresponding to a public key file.
///
/// Replaces the `.pub` extension with `.key`.
#[must_use]
pub fn private_key_path_for(pub_path: &Path) -> PathBuf {
    pub_path.with_extension("key")
}

// ── Internal helpers ──────────────────────────────────────────────────

/// Converts an Ed25519 signing key to an X25519 static secret.
///
/// Converts an Ed25519 signing key to an X25519 static secret using SHA-256.
///
/// The Ed25519 seed is hashed with SHA-256 and clamped per RFC 7748 to
/// produce the X25519 scalar.
fn ed25519_to_x25519_secret(signing_key: &SigningKey) -> StaticSecret {
    use sha2::Sha256;

    let expanded = Sha256::digest(signing_key.to_bytes());
    let mut x_bytes = [0u8; 32];
    x_bytes.copy_from_slice(&expanded[..32]);

    // Clamp per RFC 7748 / Curve25519 convention.
    x_bytes[0] &= 0b1111_1000;
    x_bytes[31] &= 0b0111_1111;
    x_bytes[31] |= 0b0100_0000;

    StaticSecret::from(x_bytes)
}

/// Derives the X25519 public key from an Ed25519 verifying key via the
/// standard Edwards-to-Montgomery point conversion.
///
/// This is the canonical way to obtain an X25519 public key from an Ed25519
/// public key without access to the private seed. It uses
/// `CompressedEdwardsY::decompress()` followed by `EdwardsPoint::to_montgomery()`.
///
/// Note: The resulting X25519 public key may not match a key derived from the
/// private seed via `ed25519_to_x25519_secret` (which uses SHA-256 clamping).
/// This function is intended for contexts where only signature verification is
/// needed and the X25519 key is not used for ECDH sealing operations.
///
/// Returns `None` if the Ed25519 public key bytes are not a valid compressed
/// Edwards Y point (this should never happen for a well-formed key).
pub(crate) fn ed25519_verifying_to_x25519_public(
    verifying: &ed25519_dalek::VerifyingKey,
) -> Option<X25519PublicKey> {
    use curve25519_dalek::edwards::CompressedEdwardsY;

    let compressed = CompressedEdwardsY::from_slice(verifying.as_bytes()).ok()?;
    let edwards = compressed.decompress()?;
    let montgomery = edwards.to_montgomery();
    Some(X25519PublicKey::from(*montgomery.as_bytes()))
}

/// Encodes both public keys into a single 64-byte blob.
fn encode_public_bytes(ed: &ed25519_dalek::VerifyingKey, x: &X25519PublicKey) -> [u8; 64] {
    let mut buf = [0u8; 64];
    buf[..32].copy_from_slice(ed.as_bytes());
    buf[32..].copy_from_slice(x.as_bytes());
    buf
}

/// Computes the fingerprint for a key pair: `SHA256:<base64 of hash>`.
///
/// The hash is over the concatenation of both public keys (64 bytes).
fn compute_fingerprint(ed: &ed25519_dalek::VerifyingKey, x: &X25519PublicKey) -> String {
    let pub_bytes = encode_public_bytes(ed, x);
    let hash = Sha256::digest(pub_bytes);
    let encoded = BASE64.encode(hash);
    format!("SHA256:{encoded}")
}

/// Derives a 256-bit symmetric key from an X25519 shared secret for sealing.
///
/// Uses HKDF-SHA256 (RFC 5869) with a two-phase extract-then-expand:
///   - **Extract:** `PRK = HMAC-SHA256(salt=ephemeral_public, IKM=shared_secret)`
///     This concentrates the non-uniform DH output into a uniformly random PRK.
///   - **Expand:** `OKM = HMAC-SHA256(PRK, info || 0x01)` (single block, 32 bytes)
///     The info string `"ovc-seal-v1"` provides domain separation.
///
/// This replaces the previous raw `SHA-256(shared_secret || ephemeral_public || label)`
/// construction, which lacked the extract phase needed to handle the non-uniform
/// distribution of X25519 shared secrets.
fn derive_seal_key(shared_secret: &[u8; 32], ephemeral_public: &[u8]) -> Zeroizing<[u8; 32]> {
    use hmac::{Hmac, KeyInit, Mac};

    type HmacSha256 = Hmac<Sha256>;

    // HKDF-Extract: PRK = HMAC-SHA256(salt=ephemeral_public, IKM=shared_secret)
    let mut extract_mac =
        HmacSha256::new_from_slice(ephemeral_public).expect("HMAC accepts any key length");
    extract_mac.update(shared_secret);
    let prk = extract_mac.finalize().into_bytes();

    // HKDF-Expand: OKM = HMAC-SHA256(PRK, info || 0x01)
    // We only need 32 bytes (one HMAC block), so a single iteration suffices.
    let mut expand_mac = HmacSha256::new_from_slice(&prk).expect("HMAC accepts any key length");
    expand_mac.update(b"ovc-seal-v1");
    expand_mac.update(&[0x01]);
    let okm = expand_mac.finalize().into_bytes();

    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&okm);
    key
}

/// Extracts the body between PEM delimiters.
fn extract_pem_body<'a>(text: &'a str, begin: &str, end: &str) -> CoreResult<&'a str> {
    let start = text.find(begin).ok_or_else(|| CoreError::FormatError {
        reason: format!("missing '{begin}' delimiter"),
    })?;
    let after_begin = start + begin.len();

    let end_pos = text[after_begin..]
        .find(end)
        .ok_or_else(|| CoreError::FormatError {
            reason: format!("missing '{end}' delimiter"),
        })?;

    Ok(text[after_begin..after_begin + end_pos].trim())
}

/// Parses `Key: Value` headers from the body text.
fn parse_headers(body: &str) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            // Only parse lines that look like headers (short key, no spaces).
            if !key.is_empty() && key.len() < MAX_HEADER_KEY_LEN && !key.contains(' ') {
                headers.insert(key.to_owned(), value.to_owned());
            }
        }
    }
    headers
}

/// Extracts the base64 data block after all `Key: Value` headers.
fn extract_data_after_headers(body: &str) -> CoreResult<String> {
    let mut data_lines = Vec::new();
    let mut past_headers = false;

    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if past_headers {
            data_lines.push(trimmed);
            continue;
        }

        if let Some((key, _)) = trimmed.split_once(':') {
            // If it looks like a header (short key, no spaces), skip it.
            if key.len() < MAX_HEADER_KEY_LEN && !key.contains(' ') {
                continue;
            }
        }

        // Not a header line: start collecting data.
        past_headers = true;
        data_lines.push(trimmed);
    }

    if data_lines.is_empty() {
        return Err(CoreError::FormatError {
            reason: "no key data found after headers".into(),
        });
    }

    Ok(data_lines.join(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_and_fingerprint() {
        let kp = OvcKeyPair::generate();
        assert!(kp.fingerprint().starts_with("SHA256:"));
        assert!(kp.fingerprint().len() > 10);
    }

    #[test]
    fn save_load_private_key_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("test.key");
        let passphrase = b"test-passphrase";

        let kp = OvcKeyPair::generate();
        let original_fingerprint = kp.fingerprint().to_owned();

        kp.save_private(&key_path, passphrase).unwrap();
        assert!(key_path.exists());

        let loaded = OvcKeyPair::load_private(&key_path, passphrase).unwrap();
        assert_eq!(loaded.fingerprint(), original_fingerprint);
    }

    #[test]
    fn save_load_public_key_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let pub_path = dir.path().join("test.pub");

        let kp = OvcKeyPair::generate();
        kp.save_public(&pub_path).unwrap();

        let loaded = OvcPublicKey::load(&pub_path).unwrap();
        assert_eq!(loaded.fingerprint, kp.fingerprint());
        assert_eq!(loaded.signing_public, *kp.signing_public());
    }

    #[test]
    fn wrong_passphrase_fails() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("test.key");

        let kp = OvcKeyPair::generate();
        kp.save_private(&key_path, b"correct").unwrap();

        let result = OvcKeyPair::load_private(&key_path, b"wrong");
        assert!(result.is_err());
    }

    #[test]
    fn seal_unseal_round_trip() {
        let kp = OvcKeyPair::generate();
        let pubkey = kp.public_key();

        let segment_key: [u8; 32] = [0xAB; 32];
        let sealed = seal_key(&segment_key, &pubkey).unwrap();

        assert_eq!(sealed.recipient_fingerprint, kp.fingerprint());

        let unsealed = unseal_key(&sealed, &kp).unwrap();
        assert_eq!(unsealed, segment_key);
    }

    #[test]
    fn seal_unseal_wrong_key_fails() {
        let kp1 = OvcKeyPair::generate();
        let kp2 = OvcKeyPair::generate();
        let pubkey1 = kp1.public_key();

        let segment_key: [u8; 32] = [0xCD; 32];
        let sealed = seal_key(&segment_key, &pubkey1).unwrap();

        // Try to unseal with the wrong key pair.
        let result = unseal_key(&sealed, &kp2);
        assert!(result.is_err());
    }

    #[test]
    fn export_import_password_manager_round_trip() {
        let kp = OvcKeyPair::generate();
        let passphrase = b"export-pass";

        let exported = kp.export_for_password_manager(passphrase).unwrap();

        // Verify both sections are present.
        assert!(exported.contains(PRIVATE_KEY_BEGIN));
        assert!(exported.contains(PRIVATE_KEY_END));
        assert!(exported.contains(PUBLIC_KEY_BEGIN));
        assert!(exported.contains(PUBLIC_KEY_END));

        let imported = OvcKeyPair::import_from_password_manager(&exported, passphrase).unwrap();
        assert_eq!(imported.fingerprint(), kp.fingerprint());
    }

    #[test]
    fn public_key_parse_from_text() {
        let kp = OvcKeyPair::generate();
        let dir = tempfile::tempdir().unwrap();
        let pub_path = dir.path().join("test.pub");

        kp.save_public(&pub_path).unwrap();
        let text = fs::read_to_string(&pub_path).unwrap();

        let parsed = OvcPublicKey::parse(&text).unwrap();
        assert_eq!(parsed.fingerprint, kp.fingerprint());
    }

    #[test]
    fn multiple_recipients_seal() {
        let kp1 = OvcKeyPair::generate();
        let kp2 = OvcKeyPair::generate();
        let segment_key: [u8; 32] = [0xEF; 32];

        let sealed1 = seal_key(&segment_key, &kp1.public_key()).unwrap();
        let sealed2 = seal_key(&segment_key, &kp2.public_key()).unwrap();

        assert_eq!(unseal_key(&sealed1, &kp1).unwrap(), segment_key);
        assert_eq!(unseal_key(&sealed2, &kp2).unwrap(), segment_key);

        // Cross-check: kp1 cannot unseal kp2's slot.
        assert!(unseal_key(&sealed2, &kp1).is_err());
    }

    #[test]
    fn identity_round_trip_private() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("id-test.key");
        let passphrase = b"test-passphrase";

        let identity = KeyIdentity {
            name: "Alice".into(),
            email: "alice@example.com".into(),
        };
        let kp = OvcKeyPair::generate_with_identity(identity.clone());
        assert_eq!(kp.identity(), Some(&identity));

        kp.save_private(&key_path, passphrase).unwrap();
        let loaded = OvcKeyPair::load_private(&key_path, passphrase).unwrap();
        assert_eq!(loaded.identity(), Some(&identity));
        assert_eq!(loaded.fingerprint(), kp.fingerprint());
    }

    #[test]
    fn identity_round_trip_public() {
        let dir = tempfile::tempdir().unwrap();
        let pub_path = dir.path().join("id-test.pub");

        let identity = KeyIdentity {
            name: "Bob".into(),
            email: "bob@example.com".into(),
        };
        let kp = OvcKeyPair::generate_with_identity(identity.clone());
        kp.save_public(&pub_path).unwrap();

        let loaded = OvcPublicKey::load(&pub_path).unwrap();
        assert_eq!(loaded.identity, Some(identity));
        assert_eq!(loaded.fingerprint, kp.fingerprint());
    }

    #[test]
    fn old_key_without_identity_loads() {
        // Keys without Identity header should still load with identity = None.
        let kp = OvcKeyPair::generate();
        assert!(kp.identity().is_none());

        let dir = tempfile::tempdir().unwrap();
        let pub_path = dir.path().join("old.pub");
        kp.save_public(&pub_path).unwrap();

        let loaded = OvcPublicKey::load(&pub_path).unwrap();
        assert!(loaded.identity.is_none());
    }

    #[test]
    fn sign_and_verify_commit() {
        use crate::id;
        use crate::object::{Commit, Identity, Object};
        use ed25519_dalek::Signer;

        let kp = OvcKeyPair::generate_with_identity(KeyIdentity {
            name: "Signer".into(),
            email: "signer@example.com".into(),
        });

        let mut commit = Commit {
            tree: id::hash_tree(b"root"),
            parents: vec![],
            author: Identity {
                name: "Alice".into(),
                email: "alice@example.com".into(),
                timestamp: 1_700_000_000,
                tz_offset_minutes: 0,
            },
            committer: Identity {
                name: "Alice".into(),
                email: "alice@example.com".into(),
                timestamp: 1_700_000_000,
                tz_offset_minutes: 0,
            },
            message: "test commit".into(),
            signature: None,
            sequence: 1,
        };

        // Serialize for hashing (without signature).
        let serialized =
            crate::serialize::serialize_object(&Object::Commit(commit.clone())).unwrap();

        // Sign.
        let sig = kp.signing_key().sign(&serialized);
        commit.signature = Some(sig.to_bytes().to_vec());

        // Verify with correct key.
        let pubkeys = vec![kp.public_key()];
        let result = verify_commit_signature(&commit, &serialized, &pubkeys);
        match result {
            VerifyResult::Verified {
                fingerprint,
                identity,
            } => {
                assert_eq!(fingerprint, kp.fingerprint());
                assert_eq!(identity.as_ref().unwrap().name, "Signer");
            }
            other => panic!("expected Verified, got {other:?}"),
        }

        // Verify with wrong key should fail.
        let kp2 = OvcKeyPair::generate();
        let wrong_keys = vec![kp2.public_key()];
        let result2 = verify_commit_signature(&commit, &serialized, &wrong_keys);
        assert!(matches!(result2, VerifyResult::Unverified { .. }));
    }

    #[test]
    fn unsigned_commit_returns_not_signed() {
        use crate::id;
        use crate::object::{Commit, Identity, Object};

        let commit = Commit {
            tree: id::hash_tree(b"root"),
            parents: vec![],
            author: Identity {
                name: "Alice".into(),
                email: "alice@example.com".into(),
                timestamp: 1_700_000_000,
                tz_offset_minutes: 0,
            },
            committer: Identity {
                name: "Alice".into(),
                email: "alice@example.com".into(),
                timestamp: 1_700_000_000,
                tz_offset_minutes: 0,
            },
            message: "test commit".into(),
            signature: None,
            sequence: 1,
        };

        let serialized =
            crate::serialize::serialize_object(&Object::Commit(commit.clone())).unwrap();
        let kp = OvcKeyPair::generate();
        let keys = vec![kp.public_key()];
        let result = verify_commit_signature(&commit, &serialized, &keys);
        assert!(matches!(result, VerifyResult::NotSigned));
    }

    #[test]
    fn verify_commit_convenience() {
        use crate::id;
        use crate::object::{Commit, Identity, Object};
        use ed25519_dalek::Signer;

        let kp = OvcKeyPair::generate();
        let mut commit = Commit {
            tree: id::hash_tree(b"root"),
            parents: vec![],
            author: Identity {
                name: "Alice".into(),
                email: "alice@example.com".into(),
                timestamp: 1_700_000_000,
                tz_offset_minutes: 0,
            },
            committer: Identity {
                name: "Alice".into(),
                email: "alice@example.com".into(),
                timestamp: 1_700_000_000,
                tz_offset_minutes: 0,
            },
            message: "test".into(),
            signature: None,
            sequence: 1,
        };

        let serialized =
            crate::serialize::serialize_object(&Object::Commit(commit.clone())).unwrap();
        let sig = kp.signing_key().sign(&serialized);
        commit.signature = Some(sig.to_bytes().to_vec());

        let keys = vec![kp.public_key()];
        let result = verify_commit(&commit, &keys);
        assert!(matches!(result, VerifyResult::Verified { .. }));
    }

    #[test]
    fn key_identity_parse() {
        let id = KeyIdentity::parse("Alice <alice@example.com>").unwrap();
        assert_eq!(id.name, "Alice");
        assert_eq!(id.email, "alice@example.com");
        assert_eq!(id.to_string(), "Alice <alice@example.com>");

        assert!(KeyIdentity::parse("no-angle-brackets").is_err());
        assert!(KeyIdentity::parse(" <email@x.com>").is_err());
    }
}
