//! Error types for the LLM integration.

/// Errors that can occur during LLM operations.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    /// LLM is not configured (no base URL or model set).
    #[error("LLM is not configured")]
    NotConfigured,

    /// The requested LLM feature is disabled in the repo config.
    #[error("LLM feature is disabled")]
    FeatureDisabled,

    /// HTTP request to the LLM API failed.
    #[error("LLM request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),

    /// Failed to parse the LLM response.
    #[error("LLM response parse error: {0}")]
    ParseError(String),

    /// The input context exceeds the configured token budget.
    #[error("context too large to process")]
    ContextTooLarge,

    /// The LLM request timed out.
    #[error("LLM request timed out")]
    Timeout,
}
