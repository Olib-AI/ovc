//! Error types for the OVC cloud sync layer.

use thiserror::Error;

/// Unified error type for cloud sync operations.
#[derive(Debug, Error)]
pub enum CloudError {
    /// A storage backend operation failed.
    #[error("storage error: {0}")]
    Storage(String),

    /// The requested resource was not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// A precondition check failed, indicating concurrent modification.
    #[error("precondition failed (concurrent modification): {0}")]
    PreconditionFailed(String),

    /// Authentication with the storage backend failed.
    #[error("authentication error: {0}")]
    AuthError(String),

    /// A network-level error occurred.
    #[error("network error: {0}")]
    Network(String),

    /// The remote has changed since the last pull; push cannot proceed.
    #[error("sync conflict: remote has changed since last pull")]
    SyncConflict,

    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization or deserialization failed.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// An error propagated from the core library.
    #[error("core error: {0}")]
    Core(#[from] ovc_core::error::CoreError),
}

/// Convenience alias for cloud operations.
pub type CloudResult<T> = Result<T, CloudError>;
