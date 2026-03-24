//! Storage backend trait for cloud sync.
//!
//! All storage backends (local filesystem, GCS, S3, etc.) implement
//! [`StorageBackend`] to provide a uniform key-value interface with
//! generation-based optimistic concurrency control.

use crate::error::CloudResult;

/// A key-value storage backend with generation-based concurrency control.
///
/// Keys are slash-separated paths (e.g. `"chunks/abc123"`, `"repos/my-repo/manifest.json"`).
/// Each key has an associated generation number that increments on every write,
/// enabling compare-and-swap semantics for conflict detection.
#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync {
    /// Stores `data` at `key`.
    ///
    /// If `precondition` is `Some(gen)`, the write only succeeds when the
    /// current generation of `key` matches `gen`. A precondition of `Some(0)`
    /// means the key must not exist yet. Returns the new generation number
    /// on success.
    async fn put(&self, key: &str, data: &[u8], precondition: Option<u64>) -> CloudResult<u64>;

    /// Retrieves the data and current generation for `key`.
    async fn get(&self, key: &str) -> CloudResult<(Vec<u8>, u64)>;

    /// Checks whether `key` exists. Returns `Some(generation)` if it does.
    async fn exists(&self, key: &str) -> CloudResult<Option<u64>>;

    /// Deletes `key` from the backend.
    async fn delete(&self, key: &str) -> CloudResult<()>;

    /// Lists all keys under the given `prefix`.
    async fn list(&self, prefix: &str) -> CloudResult<Vec<String>>;

    /// Returns a human-readable name for this backend (e.g. `"local"`, `"gcs"`).
    fn name(&self) -> &'static str;
}
