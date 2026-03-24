//! Error types for the OVC actions engine.

/// Result alias for actions operations.
pub type ActionsResult<T> = Result<T, ActionsError>;

/// Errors that can occur during action execution.
#[derive(Debug, thiserror::Error)]
pub enum ActionsError {
    /// Invalid or malformed configuration.
    #[error("config error: {reason}")]
    Config { reason: String },
    /// Referenced action does not exist in configuration.
    #[error("action not found: {name}")]
    ActionNotFound { name: String },
    /// Action exceeded its configured timeout.
    #[error("action '{name}' timed out after {timeout_secs}s")]
    Timeout { name: String, timeout_secs: u64 },
    /// Action process exited with a non-zero code.
    #[error("action '{name}' failed with exit code {code}")]
    ActionFailed { name: String, code: i32 },
    /// Error inside a built-in action.
    #[error("builtin error: {reason}")]
    BuiltinError { reason: String },
    /// YAML deserialization error.
    #[error("YAML parse error: {0}")]
    YamlParse(#[from] serde_yaml::Error),
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Rejected path traversal attempt.
    #[error("path traversal rejected: {path}")]
    PathTraversal { path: String },
    /// Dependency cycle detected among actions.
    #[error("dependency cycle detected: {details}")]
    DependencyCycle { details: String },
    /// A referenced dependency does not exist.
    #[error("action '{action}' depends on unknown action '{dependency}'")]
    UnknownDependency { action: String, dependency: String },
    /// Regex compilation error.
    #[error("invalid regex pattern: {reason}")]
    InvalidRegex { reason: String },
    /// Docker daemon is not available.
    #[error("Docker is not available: {reason}")]
    DockerUnavailable { reason: String },
    /// Docker image pull failed.
    #[error("Docker image pull failed for '{image}': {reason}")]
    DockerPullFailed { image: String, reason: String },
}
