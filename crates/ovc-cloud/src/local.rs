//! Local filesystem storage backend.
//!
//! Stores blobs as files under a base directory. Generation tracking is
//! accomplished via a `.gen` sidecar file that holds a monotonically
//! increasing counter for each key.

use std::path::{Path, PathBuf};

use crate::backend::StorageBackend;
use crate::error::{CloudError, CloudResult};

/// A [`StorageBackend`] backed by the local filesystem.
///
/// Keys are mapped to file paths under `base_path`. Slash-separated key
/// components become directory separators. Each key `K` has a sidecar
/// file `K.gen` containing the current generation number as a decimal string.
pub struct LocalBackend {
    base_path: PathBuf,
}

impl LocalBackend {
    /// Creates a new local backend rooted at `base_path`.
    ///
    /// The directory is created if it does not exist.
    pub fn new(base_path: PathBuf) -> CloudResult<Self> {
        std::fs::create_dir_all(&base_path)?;
        Ok(Self { base_path })
    }

    /// Returns the filesystem path for a given key.
    fn key_path(&self, key: &str) -> PathBuf {
        self.base_path.join(key)
    }

    /// Returns the path to the generation sidecar for a given key.
    fn gen_path(&self, key: &str) -> PathBuf {
        self.base_path.join(format!("{key}.gen"))
    }

    /// Reads the current generation from the sidecar file.
    /// Returns `0` if the sidecar does not exist.
    fn read_generation(&self, key: &str) -> CloudResult<u64> {
        let gen_path = self.gen_path(key);
        match std::fs::read_to_string(&gen_path) {
            Ok(s) => s
                .trim()
                .parse::<u64>()
                .map_err(|e| CloudError::Storage(format!("corrupt generation file: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(e) => Err(CloudError::Io(e)),
        }
    }

    /// Writes the generation number to the sidecar file.
    fn write_generation(&self, key: &str, value: u64) -> CloudResult<()> {
        let path = self.gen_path(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, value.to_string())?;
        Ok(())
    }
}

/// Collects all file paths (relative to `base_path`) under `dir`,
/// filtering out `.gen` sidecar files.
fn collect_keys(dir: &Path, prefix: &str, out: &mut Vec<String>) -> CloudResult<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(CloudError::Io(e)),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().into_owned();

        if path.is_dir() {
            let child_prefix = if prefix.is_empty() {
                file_name
            } else {
                format!("{prefix}/{file_name}")
            };
            collect_keys(&path, &child_prefix, out)?;
        } else if !std::path::Path::new(&file_name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("gen"))
        {
            let key = if prefix.is_empty() {
                file_name
            } else {
                format!("{prefix}/{file_name}")
            };
            out.push(key);
        }
    }

    Ok(())
}

#[async_trait::async_trait]
impl StorageBackend for LocalBackend {
    async fn put(&self, key: &str, data: &[u8], precondition: Option<u64>) -> CloudResult<u64> {
        let current_gen = self.read_generation(key)?;

        if let Some(expected) = precondition
            && current_gen != expected
        {
            return Err(CloudError::PreconditionFailed(format!(
                "key '{key}': expected generation {expected}, found {current_gen}"
            )));
        }

        let file_path = self.key_path(key);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&file_path, data)?;

        let next = current_gen + 1;
        self.write_generation(key, next)?;

        Ok(next)
    }

    async fn get(&self, key: &str) -> CloudResult<(Vec<u8>, u64)> {
        let file_path = self.key_path(key);
        let data = std::fs::read(&file_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                CloudError::NotFound(format!("key '{key}' not found"))
            } else {
                CloudError::Io(e)
            }
        })?;
        let current = self.read_generation(key)?;
        Ok((data, current))
    }

    async fn exists(&self, key: &str) -> CloudResult<Option<u64>> {
        let file_path = self.key_path(key);
        if file_path.exists() {
            let current = self.read_generation(key)?;
            Ok(Some(current))
        } else {
            Ok(None)
        }
    }

    async fn delete(&self, key: &str) -> CloudResult<()> {
        let file_path = self.key_path(key);
        match std::fs::remove_file(&file_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(CloudError::NotFound(format!("key '{key}' not found")));
            }
            Err(e) => return Err(CloudError::Io(e)),
        }

        // Remove sidecar too.
        let gen_path = self.gen_path(key);
        let _ = std::fs::remove_file(&gen_path);

        Ok(())
    }

    async fn list(&self, prefix: &str) -> CloudResult<Vec<String>> {
        let search_dir = if prefix.is_empty() {
            self.base_path.clone()
        } else {
            self.base_path.join(prefix)
        };

        let mut keys = Vec::new();

        if search_dir.is_dir() {
            collect_keys(&search_dir, prefix, &mut keys)?;
        } else {
            // The prefix might match files directly (not a directory).
            // List the parent directory and filter by prefix.
            let parent = search_dir.parent().unwrap_or(&self.base_path);
            let parent_prefix = Path::new(prefix)
                .parent()
                .map_or(String::new(), |p| p.to_string_lossy().into_owned());

            if parent.is_dir() {
                let mut all_keys = Vec::new();
                collect_keys(parent, &parent_prefix, &mut all_keys)?;
                keys = all_keys
                    .into_iter()
                    .filter(|k| k.starts_with(prefix))
                    .collect();
            }
        }

        keys.sort();
        Ok(keys)
    }

    fn name(&self) -> &'static str {
        "local"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_get_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(dir.path().join("store")).unwrap();

        let version = backend
            .put("test/key1", b"hello world", None)
            .await
            .unwrap();
        assert_eq!(version, 1);

        let (data, version2) = backend.get("test/key1").await.unwrap();
        assert_eq!(data, b"hello world");
        assert_eq!(version2, 1);
    }

    #[tokio::test]
    async fn exists_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(dir.path().join("store")).unwrap();

        assert!(backend.exists("missing").await.unwrap().is_none());

        backend.put("present", b"data", None).await.unwrap();
        let generation = backend.exists("present").await.unwrap();
        assert_eq!(generation, Some(1));

        backend.delete("present").await.unwrap();
        assert!(backend.exists("present").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_keys() {
        let dir = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(dir.path().join("store")).unwrap();

        backend.put("chunks/aaa", b"1", None).await.unwrap();
        backend.put("chunks/bbb", b"2", None).await.unwrap();
        backend
            .put("repos/manifest.json", b"3", None)
            .await
            .unwrap();

        let chunk_keys = backend.list("chunks").await.unwrap();
        assert_eq!(chunk_keys.len(), 2);
        assert!(chunk_keys.contains(&"chunks/aaa".to_owned()));
        assert!(chunk_keys.contains(&"chunks/bbb".to_owned()));

        let repo_keys = backend.list("repos").await.unwrap();
        assert_eq!(repo_keys.len(), 1);
    }

    #[tokio::test]
    async fn precondition_success() {
        let dir = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(dir.path().join("store")).unwrap();

        let gen1 = backend.put("key", b"v1", Some(0)).await.unwrap();
        assert_eq!(gen1, 1);

        let gen2 = backend.put("key", b"v2", Some(1)).await.unwrap();
        assert_eq!(gen2, 2);

        let (data, _) = backend.get("key").await.unwrap();
        assert_eq!(data, b"v2");
    }

    #[tokio::test]
    async fn precondition_failure() {
        let dir = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(dir.path().join("store")).unwrap();

        backend.put("key", b"v1", None).await.unwrap();

        let result = backend.put("key", b"v2", Some(999)).await;
        assert!(matches!(result, Err(CloudError::PreconditionFailed(_))));
    }

    #[tokio::test]
    async fn get_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(dir.path().join("store")).unwrap();

        let result = backend.get("nonexistent").await;
        assert!(matches!(result, Err(CloudError::NotFound(_))));
    }

    #[tokio::test]
    async fn delete_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(dir.path().join("store")).unwrap();

        let result = backend.delete("nonexistent").await;
        assert!(matches!(result, Err(CloudError::NotFound(_))));
    }
}
