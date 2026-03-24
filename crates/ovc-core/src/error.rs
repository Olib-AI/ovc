//! Error types for the OVC core library.
//!
//! Provides [`CoreError`] as the unified error type for all operations
//! within `ovc-core`, and [`CoreResult`] as the standard result alias.

use thiserror::Error;

use crate::id::ObjectId;

/// Unified error type for all `ovc-core` operations.
#[derive(Debug, Error)]
pub enum CoreError {
    /// The requested object was not found in the store.
    #[error("object not found: {0}")]
    ObjectNotFound(ObjectId),

    /// An object's data is corrupt or cannot be parsed.
    #[error("corrupt object: {reason}")]
    CorruptObject {
        /// Human-readable description of the corruption.
        reason: String,
    },

    /// The repository has not been initialized.
    #[error("repository not initialized at path: {path}")]
    NotInitialized {
        /// The path where a repository was expected.
        path: String,
    },

    /// A repository already exists at the given path.
    #[error("repository already exists at path: {path}")]
    AlreadyExists {
        /// The path where the repository was found.
        path: String,
    },

    /// The `.ovc` file format is invalid or unsupported.
    #[error("format error: {reason}")]
    FormatError {
        /// Human-readable description of the format issue.
        reason: String,
    },

    /// Encryption failed.
    #[error("encryption failed: {reason}")]
    EncryptionFailed {
        /// Human-readable description of the failure.
        reason: String,
    },

    /// Decryption failed (wrong password or corrupt data).
    #[error("decryption failed: {reason}")]
    DecryptionFailed {
        /// Human-readable description of the failure.
        reason: String,
    },

    /// Key derivation failed.
    #[error("key derivation failed: {reason}")]
    KeyDerivationFailed {
        /// Human-readable description of the failure.
        reason: String,
    },

    /// Data integrity check failed.
    #[error("integrity error: {reason}")]
    IntegrityError {
        /// Human-readable description of the integrity violation.
        reason: String,
    },

    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization or deserialization error.
    #[error("serialization error: {reason}")]
    Serialization {
        /// Human-readable description of the serialization error.
        reason: String,
    },

    /// Configuration error.
    #[error("configuration error: {reason}")]
    Config {
        /// Human-readable description of the configuration issue.
        reason: String,
    },

    /// Compression or decompression error.
    #[error("compression error: {reason}")]
    Compression {
        /// Human-readable description of the compression issue.
        reason: String,
    },

    /// Key management error (generation, loading, sealing).
    #[error("key error: {reason}")]
    KeyError {
        /// Human-readable description of the key management issue.
        reason: String,
    },

    /// Could not acquire repository lock.
    #[error("lock error: {reason}")]
    LockError {
        /// Human-readable description of why the lock could not be acquired.
        reason: String,
    },

    /// Conflict detected: file was modified externally.
    #[error("conflict detected: {reason}")]
    ConflictDetected {
        /// Human-readable description of the detected conflict.
        reason: String,
    },
}

/// Standard result type for `ovc-core` operations.
pub type CoreResult<T> = Result<T, CoreError>;
