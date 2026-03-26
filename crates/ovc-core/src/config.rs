//! Repository configuration types.
//!
//! [`RepositoryConfig`] holds user identity, remote definitions, and
//! tuning parameters. It is serialized inside the encrypted superblock
//! of a `.ovc` file.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Configuration stored inside the encrypted superblock of a `.ovc` file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Optional LLM configuration for AI-powered features.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm: Option<LlmRepoConfig>,
}

impl Default for RepositoryConfig {
    fn default() -> Self {
        Self {
            user_name: String::new(),
            user_email: String::new(),
            default_branch: "main".to_owned(),
            remotes: BTreeMap::new(),
            compression_level: crate::compression::DEFAULT_COMPRESSION_LEVEL,
            llm: None,
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

/// Per-repo LLM configuration stored encrypted in the superblock.
///
/// All fields are optional: `None` means "inherit from server defaults".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmRepoConfig {
    /// Override base URL for the OpenAI-compatible API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Override model name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Maximum context tokens to send to the LLM.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_context_tokens: Option<usize>,
    /// Temperature for LLM sampling (0.0–2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Per-feature enable/disable toggles.
    #[serde(default)]
    pub enabled_features: LlmFeatureToggles,
}

/// Feature-level toggles for LLM-powered capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct LlmFeatureToggles {
    /// Generate commit messages from staged diffs.
    pub commit_message: bool,
    /// Generate PR descriptions from commits and diffs.
    pub pr_description: bool,
    /// AI-powered code review for PRs.
    pub pr_review: bool,
    /// Explain diffs in plain English.
    pub explain_diff: bool,
}

impl Default for LlmFeatureToggles {
    fn default() -> Self {
        Self {
            commit_message: true,
            pr_description: true,
            pr_review: true,
            explain_diff: true,
        }
    }
}
