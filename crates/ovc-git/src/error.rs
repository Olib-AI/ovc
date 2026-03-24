//! Error types for the `ovc-git` bridge crate.

use std::path::PathBuf;

/// Errors arising from git-to-OVC and OVC-to-git conversion operations.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// The path does not contain a valid git repository.
    #[error("not a git repository: {0}")]
    NotAGitRepo(PathBuf),

    /// A git object referenced by SHA1 was not found on disk.
    #[error("git object not found: {0}")]
    ObjectNotFound(String),

    /// A git object exists but is corrupt or cannot be parsed.
    #[error("corrupt git object: {0}")]
    CorruptObject(String),

    /// The git object type is not recognized.
    #[error("unsupported git object type: {0}")]
    UnsupportedObjectType(String),

    /// A git reference could not be parsed.
    #[error("invalid git ref: {0}")]
    InvalidRef(String),

    /// An error bubbled up from `ovc-core`.
    #[error("ovc error: {0}")]
    Core(#[from] ovc_core::error::CoreError),

    /// A standard I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// An encoding/decoding error (hex, UTF-8, etc.).
    #[error("encoding error: {0}")]
    Encoding(String),
}

/// Convenience alias.
pub type GitResult<T> = Result<T, GitError>;
