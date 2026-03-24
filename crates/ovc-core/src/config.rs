//! Repository configuration types.
//!
//! [`RepositoryConfig`] holds user identity, remote definitions, and
//! tuning parameters. It is serialized inside the encrypted superblock
//! of a `.ovc` file.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Configuration stored inside the encrypted superblock of a `.ovc` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryConfig {
    /// The user's display name for new commits.
    pub user_name: String,
    /// The user's email address for new commits.
    pub user_email: String,
    /// The name of the default branch (e.g., `"main"`).
    pub default_branch: String,
    /// Named remote repositories.
    pub remotes: BTreeMap<String, RemoteConfig>,
    /// Zstandard compression level (1–22).
    pub compression_level: i32,
}

impl Default for RepositoryConfig {
    fn default() -> Self {
        Self {
            user_name: String::new(),
            user_email: String::new(),
            default_branch: "main".to_owned(),
            remotes: BTreeMap::new(),
            compression_level: crate::compression::DEFAULT_COMPRESSION_LEVEL,
        }
    }
}

/// Configuration for a single remote repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteConfig {
    /// The URL of the remote (HTTPS, SSH, or cloud-provider URI).
    pub url: String,
    /// The backend type identifier (e.g., `"s3"`, `"gcs"`, `"ssh"`).
    pub backend_type: String,
}
