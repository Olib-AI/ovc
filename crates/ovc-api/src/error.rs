//! API error types and HTTP response mapping.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// Structured error returned by all API endpoints.
#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    /// Machine-readable error code.
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// HTTP status code (not serialized in the body).
    #[serde(skip)]
    pub status: StatusCode,
}

impl ApiError {
    /// Creates a 401 Unauthorized error.
    #[must_use]
    pub fn unauthorized(message: &str) -> Self {
        Self {
            code: "UNAUTHORIZED".to_owned(),
            message: message.to_owned(),
            status: StatusCode::UNAUTHORIZED,
        }
    }

    /// Creates a 404 Not Found error.
    #[must_use]
    pub fn not_found(message: &str) -> Self {
        Self {
            code: "NOT_FOUND".to_owned(),
            message: message.to_owned(),
            status: StatusCode::NOT_FOUND,
        }
    }

    /// Creates a 400 Bad Request error.
    #[must_use]
    pub fn bad_request(message: &str) -> Self {
        Self {
            code: "BAD_REQUEST".to_owned(),
            message: message.to_owned(),
            status: StatusCode::BAD_REQUEST,
        }
    }

    /// Creates a 403 Forbidden error.
    #[must_use]
    pub fn forbidden(message: &str) -> Self {
        Self {
            code: "FORBIDDEN".to_owned(),
            message: message.to_owned(),
            status: StatusCode::FORBIDDEN,
        }
    }

    /// Creates a 409 Conflict error.
    #[must_use]
    pub fn conflict(message: &str) -> Self {
        Self {
            code: "CONFLICT".to_owned(),
            message: message.to_owned(),
            status: StatusCode::CONFLICT,
        }
    }

    /// Creates a 501 Not Implemented error.
    #[must_use]
    pub fn not_implemented(message: &str) -> Self {
        Self {
            code: "NOT_IMPLEMENTED".to_owned(),
            message: message.to_owned(),
            status: StatusCode::NOT_IMPLEMENTED,
        }
    }

    /// Creates a 429 Too Many Requests error.
    #[must_use]
    pub fn rate_limited(message: &str) -> Self {
        Self {
            code: "RATE_LIMITED".to_owned(),
            message: message.to_owned(),
            status: StatusCode::TOO_MANY_REQUESTS,
        }
    }

    /// Creates a 503 Service Unavailable error.
    #[must_use]
    pub fn service_unavailable(message: &str) -> Self {
        Self {
            code: "SERVICE_UNAVAILABLE".to_owned(),
            message: message.to_owned(),
            status: StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// Creates a 500 Internal Server Error.
    #[must_use]
    pub fn internal(message: &str) -> Self {
        Self {
            code: "INTERNAL_ERROR".to_owned(),
            message: message.to_owned(),
            status: StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Maps a `CoreError` into an appropriate `ApiError`.
    ///
    /// Consumes the error to allow use with `.map_err(ApiError::from_core)`.
    ///
    /// Internal errors (I/O, encryption failures, etc.) are logged but NOT
    /// exposed to the API client. Only a generic message is returned to
    /// prevent information disclosure of filesystem paths, OS error details,
    /// or internal state.
    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
    pub fn from_core(err: ovc_core::error::CoreError) -> Self {
        use ovc_core::error::CoreError;
        match err {
            CoreError::NotInitialized { .. } => Self::not_found("repository not found"),
            CoreError::ObjectNotFound(_) => Self::not_found("object not found"),
            CoreError::AlreadyExists { .. } => Self::conflict("resource already exists"),
            CoreError::DecryptionFailed { .. } | CoreError::KeyDerivationFailed { .. } => {
                Self::unauthorized("authentication failed")
            }
            CoreError::FormatError { reason } | CoreError::Config { reason } => {
                Self::bad_request(&reason)
            }
            CoreError::CorruptObject { .. } => Self::bad_request("corrupt or invalid object"),
            CoreError::Serialization { .. } => Self::bad_request("serialization error"),
            CoreError::KeyError { .. } => Self::bad_request("key error"),
            // Internal errors: log the real error but return a generic message
            // to the client to prevent information disclosure.
            CoreError::IntegrityError { .. } => {
                tracing::error!("integrity error: {err}");
                Self::internal("internal integrity error")
            }
            CoreError::EncryptionFailed { .. } => {
                tracing::error!("encryption error: {err}");
                Self::internal("internal encryption error")
            }
            CoreError::Io(ref _io_err) => {
                tracing::error!("I/O error: {err}");
                Self::internal("internal I/O error")
            }
            CoreError::Compression { .. } => {
                tracing::error!("compression error: {err}");
                Self::internal("internal compression error")
            }
            CoreError::LockError { .. } => {
                tracing::error!("lock error: {err}");
                Self::internal("repository is locked — try again later")
            }
            CoreError::ConflictDetected { .. } => {
                Self::conflict("repository was modified externally — retry the operation")
            }
        }
    }

    /// Maps a `CloudError` into an appropriate `ApiError`.
    ///
    /// Consumes the error to allow use with `.map_err(ApiError::from_cloud)`.
    ///
    /// Internal errors (I/O, storage, network, etc.) are logged but NOT
    /// exposed to the API client. Only a generic message is returned to
    /// prevent information disclosure of storage backend URLs, filesystem
    /// paths, or OS error details.
    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
    pub fn from_cloud(err: ovc_cloud::CloudError) -> Self {
        use ovc_cloud::CloudError;
        match err {
            CloudError::NotFound(_) => Self::not_found("cloud resource not found"),
            CloudError::AuthError(_) => Self::unauthorized("cloud authentication failed"),
            CloudError::SyncConflict | CloudError::PreconditionFailed(_) => {
                Self::conflict("cloud sync conflict — retry the operation")
            }
            CloudError::Storage(_) => {
                tracing::error!("cloud storage error: {err}");
                Self::internal("cloud storage error")
            }
            CloudError::Network(_) => {
                tracing::error!("cloud network error: {err}");
                Self::internal("cloud network error")
            }
            CloudError::Io(_) => {
                tracing::error!("cloud I/O error: {err}");
                Self::internal("cloud I/O error")
            }
            CloudError::Serialization(_) => {
                tracing::error!("cloud serialization error: {err}");
                Self::internal("cloud serialization error")
            }
            CloudError::Core(_) => {
                tracing::error!("cloud core error: {err}");
                Self::internal("internal error")
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status;
        let body = serde_json::json!({
            "error": {
                "code": self.code,
                "message": self.message,
            }
        });
        (status, axum::Json(body)).into_response()
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {}: {}",
            self.status.as_u16(),
            self.code,
            self.message
        )
    }
}
