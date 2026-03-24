//! Google Cloud Storage backend.
//!
//! Implements [`StorageBackend`] using the GCS JSON API over HTTP.
//! Supports generation-based optimistic concurrency via the
//! `x-goog-if-generation-match` header.

use reqwest::StatusCode;

use crate::backend::StorageBackend;
use crate::error::{CloudError, CloudResult};

/// Maximum number of retry attempts for transient GCS errors.
const GCS_MAX_RETRIES: u32 = 3;

/// Initial retry delay in milliseconds; doubled on each subsequent attempt.
const GCS_RETRY_INITIAL_DELAY_MS: u64 = 100;

/// Returns `true` for HTTP status codes that are safe to retry.
///
/// 429 (Too Many Requests), 500 (Internal Server Error), 502 (Bad Gateway),
/// and 503 (Service Unavailable) are transient and warrant a retry with
/// exponential backoff.
const fn is_retryable_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::TOO_MANY_REQUESTS
            | StatusCode::INTERNAL_SERVER_ERROR
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
    )
}

/// Base URL for the GCS JSON API (upload endpoint).
const GCS_UPLOAD_BASE: &str = "https://storage.googleapis.com/upload/storage/v1";
/// Base URL for the GCS JSON API (metadata/download endpoint).
const GCS_API_BASE: &str = "https://storage.googleapis.com/storage/v1";

/// A [`StorageBackend`] backed by Google Cloud Storage.
///
/// Uses the GCS JSON API with Bearer token authentication.
/// Keys are prefixed with an optional path prefix within the bucket.
pub struct GcsBackend {
    bucket: String,
    prefix: String,
    auth_token: String,
    client: reqwest::Client,
}

impl GcsBackend {
    /// Creates a new GCS backend targeting `bucket` with an optional key `prefix`.
    ///
    /// The `auth_token` is a Bearer token for GCS API authentication.
    ///
    /// # Errors
    ///
    /// Returns an error if the bucket name contains characters that could allow
    /// URL injection (e.g., `/`, `?`, `#`, `\n`). Per the GCS naming rules,
    /// bucket names must consist of lowercase letters, digits, hyphens, underscores,
    /// and dots only.
    pub fn new(bucket: String, prefix: String, auth_token: String) -> Result<Self, CloudError> {
        // Validate bucket name to prevent URL injection. GCS bucket names
        // may only contain lowercase letters, digits, hyphens, underscores, and dots.
        if bucket.is_empty()
            || !bucket.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' || c == '.'
            })
        {
            return Err(CloudError::Storage(format!(
                "invalid GCS bucket name: '{bucket}' — must contain only [a-z0-9._-]"
            )));
        }
        let client = reqwest::Client::new();
        Ok(Self {
            bucket,
            prefix,
            auth_token,
            client,
        })
    }

    /// Builds the full object name by combining prefix and key.
    fn full_key(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            key.to_owned()
        } else {
            format!("{}/{key}", self.prefix)
        }
    }

    /// URL-encodes an object name for use in GCS API paths.
    fn encode_object_name(name: &str) -> String {
        // GCS requires percent-encoding of slashes in object names within URL paths.
        name.replace('/', "%2F")
    }

    /// Maps an HTTP status code to the appropriate `CloudError`.
    fn map_status(status: StatusCode, key: &str, body: &str) -> CloudError {
        match status {
            StatusCode::NOT_FOUND => CloudError::NotFound(format!("key '{key}' not found")),
            StatusCode::PRECONDITION_FAILED => {
                CloudError::PreconditionFailed(format!("key '{key}': generation mismatch"))
            }
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                CloudError::AuthError(format!("authentication failed: {body}"))
            }
            _ => CloudError::Storage(format!("GCS API error {status} for key '{key}': {body}")),
        }
    }
}

#[async_trait::async_trait]
impl StorageBackend for GcsBackend {
    async fn put(&self, key: &str, data: &[u8], precondition: Option<u64>) -> CloudResult<u64> {
        let full_key = self.full_key(key);
        let encoded = Self::encode_object_name(&full_key);

        let url = format!(
            "{GCS_UPLOAD_BASE}/b/{}/o?uploadType=media&name={encoded}",
            self.bucket,
        );

        let mut attempt = 0u32;
        let mut delay_ms = GCS_RETRY_INITIAL_DELAY_MS;

        loop {
            attempt += 1;

            let mut request = self
                .client
                .post(&url)
                .bearer_auth(&self.auth_token)
                .header("Content-Type", "application/octet-stream")
                .body(data.to_vec());

            if let Some(expected) = precondition {
                request = request.header("x-goog-if-generation-match", expected.to_string());
            }

            let response = match request.send().await {
                Ok(r) => r,
                Err(e) if attempt < GCS_MAX_RETRIES => {
                    tracing::warn!(attempt, "GCS PUT network error ({e}), retrying");
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    delay_ms *= 4;
                    continue;
                }
                Err(e) => return Err(CloudError::Network(format!("PUT request failed: {e}"))),
            };

            let status = response.status();
            if is_retryable_status(status) && attempt < GCS_MAX_RETRIES {
                let body = response.text().await.unwrap_or_default();
                tracing::warn!(attempt, %status, "GCS PUT transient error ({body}), retrying");
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                delay_ms *= 4;
                continue;
            }
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(Self::map_status(status, key, &body));
            }

            // Parse the generation from the response JSON.
            let body: serde_json::Value = response.json().await.map_err(|e| {
                CloudError::Serialization(format!("failed to parse PUT response: {e}"))
            })?;

            let generation = body["generation"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(1);

            return Ok(generation);
        }
    }

    async fn get(&self, key: &str) -> CloudResult<(Vec<u8>, u64)> {
        let full_key = self.full_key(key);
        let encoded = Self::encode_object_name(&full_key);

        let meta_url = format!("{GCS_API_BASE}/b/{}/o/{encoded}", self.bucket,);
        let data_url = format!("{GCS_API_BASE}/b/{}/o/{encoded}?alt=media", self.bucket,);

        let mut attempt = 0u32;
        let mut delay_ms = GCS_RETRY_INITIAL_DELAY_MS;

        loop {
            attempt += 1;

            // First, get metadata to obtain generation.
            let meta_response = match self
                .client
                .get(&meta_url)
                .bearer_auth(&self.auth_token)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) if attempt < GCS_MAX_RETRIES => {
                    tracing::warn!(attempt, "GCS GET metadata network error ({e}), retrying");
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    delay_ms *= 4;
                    continue;
                }
                Err(e) => return Err(CloudError::Network(format!("metadata GET failed: {e}"))),
            };

            let meta_status = meta_response.status();
            if is_retryable_status(meta_status) && attempt < GCS_MAX_RETRIES {
                let body = meta_response.text().await.unwrap_or_default();
                tracing::warn!(attempt, %meta_status, "GCS GET metadata transient error ({body}), retrying");
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                delay_ms *= 4;
                continue;
            }
            if !meta_status.is_success() {
                let body = meta_response.text().await.unwrap_or_default();
                return Err(Self::map_status(meta_status, key, &body));
            }

            let meta: serde_json::Value = meta_response
                .json()
                .await
                .map_err(|e| CloudError::Serialization(format!("failed to parse metadata: {e}")))?;

            let generation = meta["generation"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);

            // Download the actual data.
            let data_response = match self
                .client
                .get(&data_url)
                .bearer_auth(&self.auth_token)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) if attempt < GCS_MAX_RETRIES => {
                    tracing::warn!(attempt, "GCS GET data network error ({e}), retrying");
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    delay_ms *= 4;
                    continue;
                }
                Err(e) => return Err(CloudError::Network(format!("data GET failed: {e}"))),
            };

            let data_status = data_response.status();
            if is_retryable_status(data_status) && attempt < GCS_MAX_RETRIES {
                let body = data_response.text().await.unwrap_or_default();
                tracing::warn!(attempt, %data_status, "GCS GET data transient error ({body}), retrying");
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                delay_ms *= 4;
                continue;
            }
            if !data_status.is_success() {
                let body = data_response.text().await.unwrap_or_default();
                return Err(Self::map_status(data_status, key, &body));
            }

            let data = data_response
                .bytes()
                .await
                .map_err(|e| CloudError::Network(format!("failed to read response body: {e}")))?;

            return Ok((data.to_vec(), generation));
        }
    }

    async fn exists(&self, key: &str) -> CloudResult<Option<u64>> {
        let full_key = self.full_key(key);
        let encoded = Self::encode_object_name(&full_key);

        let url = format!("{GCS_API_BASE}/b/{}/o/{encoded}", self.bucket,);

        let mut attempt = 0u32;
        let mut delay_ms = GCS_RETRY_INITIAL_DELAY_MS;

        loop {
            attempt += 1;

            let response = match self
                .client
                .get(&url)
                .bearer_auth(&self.auth_token)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) if attempt < GCS_MAX_RETRIES => {
                    tracing::warn!(attempt, "GCS exists network error ({e}), retrying");
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    delay_ms *= 4;
                    continue;
                }
                Err(e) => return Err(CloudError::Network(format!("exists check failed: {e}"))),
            };

            let status = response.status();

            if status == StatusCode::NOT_FOUND {
                return Ok(None);
            }

            if is_retryable_status(status) && attempt < GCS_MAX_RETRIES {
                let body = response.text().await.unwrap_or_default();
                tracing::warn!(attempt, %status, "GCS exists transient error ({body}), retrying");
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                delay_ms *= 4;
                continue;
            }
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(Self::map_status(status, key, &body));
            }

            let meta: serde_json::Value = response
                .json()
                .await
                .map_err(|e| CloudError::Serialization(format!("failed to parse metadata: {e}")))?;

            let generation = meta["generation"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);

            return Ok(Some(generation));
        }
    }

    async fn delete(&self, key: &str) -> CloudResult<()> {
        let full_key = self.full_key(key);
        let encoded = Self::encode_object_name(&full_key);

        let url = format!("{GCS_API_BASE}/b/{}/o/{encoded}", self.bucket,);

        let response = self
            .client
            .delete(&url)
            .bearer_auth(&self.auth_token)
            .send()
            .await
            .map_err(|e| CloudError::Network(format!("DELETE failed: {e}")))?;

        let status = response.status();
        // GCS returns 204 No Content on successful delete.
        if status == StatusCode::NO_CONTENT || status.is_success() {
            return Ok(());
        }

        let body = response.text().await.unwrap_or_default();
        Err(Self::map_status(status, key, &body))
    }

    async fn list(&self, prefix: &str) -> CloudResult<Vec<String>> {
        let full_prefix = self.full_key(prefix);
        let mut keys = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut url = format!("{GCS_API_BASE}/b/{}/o?prefix={}", self.bucket, &full_prefix,);

            if let Some(ref token) = page_token {
                use std::fmt::Write;
                let _ = write!(url, "&pageToken={token}");
            }

            let response = self
                .client
                .get(&url)
                .bearer_auth(&self.auth_token)
                .send()
                .await
                .map_err(|e| CloudError::Network(format!("list request failed: {e}")))?;

            let list_status = response.status();
            if !list_status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(Self::map_status(list_status, prefix, &body));
            }

            let body: serde_json::Value = response.json().await.map_err(|e| {
                CloudError::Serialization(format!("failed to parse list response: {e}"))
            })?;

            if let Some(items) = body["items"].as_array() {
                for item in items {
                    if let Some(name) = item["name"].as_str() {
                        // Strip the prefix from the full GCS object name to return
                        // the key relative to the backend's configured prefix.
                        let key = if self.prefix.is_empty() {
                            name.to_owned()
                        } else {
                            name.strip_prefix(&format!("{}/", self.prefix))
                                .unwrap_or(name)
                                .to_owned()
                        };
                        keys.push(key);
                    }
                }
            }

            page_token = body["nextPageToken"].as_str().map(String::from);
            if page_token.is_none() {
                break;
            }
        }

        keys.sort();
        Ok(keys)
    }

    fn name(&self) -> &'static str {
        "gcs"
    }
}
