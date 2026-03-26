//! LLM configuration types and resolution logic.
//!
//! Server-level defaults are merged with per-repo overrides to produce a
//! [`ResolvedLlmConfig`] used by the LLM client.

use ovc_core::config::{LlmFeatureToggles, LlmRepoConfig};

use crate::error::LlmError;

/// Server-level LLM configuration, loaded from environment variables or CLI flags.
///
/// These serve as defaults that can be overridden per-repo via [`LlmRepoConfig`].
#[derive(Debug, Clone)]
pub struct LlmServerConfig {
    /// Base URL of the OpenAI-compatible API (e.g., `http://localhost:11434`).
    pub base_url: Option<String>,
    /// Model name to use (e.g., `llama3`, `codestral`).
    pub model: Option<String>,
    /// API key for authentication (most local LLM servers ignore this).
    pub api_key: Option<String>,
    /// Whether LLM features are enabled at the server level.
    pub enabled: bool,
    /// Maximum approximate token count for context sent to the LLM.
    pub max_context_tokens: usize,
    /// Request timeout in seconds.
    pub request_timeout_secs: u64,
}

impl Default for LlmServerConfig {
    fn default() -> Self {
        Self {
            base_url: None,
            model: None,
            api_key: None,
            enabled: false,
            max_context_tokens: 32768,
            request_timeout_secs: 120,
        }
    }
}

/// Fully resolved LLM configuration, ready for use by [`crate::client::LlmClient`].
///
/// Produced by merging server defaults with per-repo overrides.
#[derive(Debug, Clone)]
pub struct ResolvedLlmConfig {
    /// Base URL of the OpenAI-compatible API.
    pub base_url: String,
    /// Model name to use.
    pub model: String,
    /// Optional API key.
    pub api_key: Option<String>,
    /// Maximum approximate token count for context.
    pub max_context_tokens: usize,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
    /// Temperature for LLM sampling (0.0–2.0).
    pub temperature: f32,
    /// Per-feature toggles.
    pub features: LlmFeatureToggles,
}

/// Resolves the effective LLM configuration by merging server defaults with
/// per-repo overrides.
///
/// Returns [`LlmError::NotConfigured`] if no base URL is available from either
/// the server config or the repo config.
pub fn resolve_config(
    server: &LlmServerConfig,
    repo: Option<&LlmRepoConfig>,
) -> Result<ResolvedLlmConfig, LlmError> {
    let base_url = repo
        .and_then(|r| r.base_url.as_deref())
        .or(server.base_url.as_deref())
        .ok_or(LlmError::NotConfigured)?
        .to_owned();

    let model = repo
        .and_then(|r| r.model.as_deref())
        .or(server.model.as_deref())
        .unwrap_or("llama3")
        .to_owned();

    let features = repo.map(|r| r.enabled_features.clone()).unwrap_or_default();

    let max_context_tokens = repo
        .and_then(|r| r.max_context_tokens)
        .unwrap_or(server.max_context_tokens);

    let temperature = repo.and_then(|r| r.temperature).unwrap_or(0.3);

    Ok(ResolvedLlmConfig {
        base_url,
        model,
        api_key: server.api_key.clone(),
        max_context_tokens,
        timeout_secs: server.request_timeout_secs,
        temperature,
        features,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_requires_base_url() {
        let server = LlmServerConfig {
            enabled: true,
            base_url: None,
            ..Default::default()
        };
        assert!(resolve_config(&server, None).is_err());
    }

    #[test]
    fn resolve_uses_server_defaults() {
        let server = LlmServerConfig {
            enabled: true,
            base_url: Some("http://localhost:11434".to_owned()),
            model: Some("codestral".to_owned()),
            ..Default::default()
        };
        let resolved = resolve_config(&server, None).unwrap();
        assert_eq!(resolved.base_url, "http://localhost:11434");
        assert_eq!(resolved.model, "codestral");
    }

    #[test]
    fn resolve_repo_overrides_server() {
        let server = LlmServerConfig {
            enabled: true,
            base_url: Some("http://localhost:11434".to_owned()),
            model: Some("codestral".to_owned()),
            ..Default::default()
        };
        let repo = LlmRepoConfig {
            base_url: Some("http://localhost:1234".to_owned()),
            model: Some("deepseek-coder".to_owned()),
            max_context_tokens: None,
            temperature: None,
            enabled_features: LlmFeatureToggles::default(),
        };
        let resolved = resolve_config(&server, Some(&repo)).unwrap();
        assert_eq!(resolved.base_url, "http://localhost:1234");
        assert_eq!(resolved.model, "deepseek-coder");
    }
}
