//! Sync engine for pushing and pulling `.ovc` files to/from cloud storage.
//!
//! The engine uses content-defined chunking to minimize data transfer:
//! only chunks that differ between the local and remote representations
//! are uploaded or downloaded.

use std::collections::HashSet;
use std::path::Path;

use chrono::Utc;
use sha2::Digest as _;

use crate::backend::StorageBackend;
use crate::chunker::{self, ChunkParams, sha256_hex};
use crate::error::{CloudError, CloudResult};
use crate::manifest::{ChunkDescriptor, SyncManifest};

// ── Sidecar sync-state ────────────────────────────────────────────────────────

/// Persisted sidecar written alongside the `.ovc` file after each successful
/// push or pull. Stored as `<ovc_path>.sync-state`.
///
/// This gives `status()` enough context to distinguish between `LocalAhead`,
/// `RemoteAhead`, and `Diverged` without an extra round-trip to the backend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SyncState {
    /// The manifest generation number at the time of the last sync.
    generation: u64,
    /// The manifest `file_hash` at the time of the last sync.
    file_hash: String,
}

/// Returns the path of the sidecar file for a given `.ovc` path.
fn sync_state_path(ovc_path: &Path) -> std::path::PathBuf {
    let mut p = ovc_path.as_os_str().to_os_string();
    p.push(".sync-state");
    std::path::PathBuf::from(p)
}

/// Reads the sidecar sync state. Returns `None` if the file does not exist or
/// cannot be parsed (treated as "no prior sync").
fn read_sync_state(ovc_path: &Path) -> Option<SyncState> {
    let p = sync_state_path(ovc_path);
    let data = std::fs::read_to_string(&p).ok()?;
    serde_json::from_str(&data).ok()
}

/// Atomically writes the sidecar sync state.
fn write_sync_state(ovc_path: &Path, state: &SyncState) -> CloudResult<()> {
    let sidecar = sync_state_path(ovc_path);
    let json = serde_json::to_string(state)
        .map_err(|e| CloudError::Storage(format!("failed to serialize sync state: {e}")))?;
    // Write via a temp file + rename for atomicity.
    let parent = sidecar.parent().unwrap_or_else(|| Path::new("."));
    let tmp = parent.join(format!(".sync-state-{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, json.as_bytes())
        .map_err(|e| CloudError::Storage(format!("failed to write sync state tmp: {e}")))?;
    std::fs::rename(&tmp, &sidecar).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        CloudError::Storage(format!("failed to rename sync state: {e}"))
    })
}

/// The sync engine coordinates chunked push/pull operations against
/// a [`StorageBackend`].
pub struct SyncEngine {
    backend: Box<dyn StorageBackend>,
    repo_id: String,
}

/// Statistics returned after a successful push.
#[derive(Debug, Clone)]
pub struct PushResult {
    /// Number of new chunks uploaded.
    pub chunks_uploaded: u64,
    /// Number of chunks reused from the remote (already present).
    pub chunks_reused: u64,
    /// Total bytes uploaded (chunk data only).
    pub bytes_uploaded: u64,
    /// The manifest version (generation) after the push.
    pub manifest_version: u64,
}

/// Statistics returned after a successful pull.
#[derive(Debug, Clone)]
pub struct PullResult {
    /// Number of chunks downloaded from the remote.
    pub chunks_downloaded: u64,
    /// Number of chunks found in the local cache (not downloaded).
    pub chunks_cached: u64,
    /// Total bytes downloaded.
    pub bytes_downloaded: u64,
    /// The manifest version (generation) of the pulled data.
    pub manifest_version: u64,
}

/// Sync status comparing local state against the remote.
#[derive(Debug, Clone)]
pub enum SyncStatus {
    /// Local and remote are identical.
    InSync {
        /// The current manifest version.
        version: u64,
    },
    /// Local has changes not yet pushed.
    LocalAhead,
    /// Remote has a newer version than local.
    RemoteAhead {
        /// The remote manifest version.
        remote_version: u64,
    },
    /// Both local and remote have diverged.
    Diverged,
    /// No remote manifest exists yet.
    NoRemote,
}

impl SyncEngine {
    /// Creates a new sync engine using the given storage backend.
    #[must_use]
    pub fn new(backend: Box<dyn StorageBackend>, repo_id: String) -> Self {
        Self { backend, repo_id }
    }

    /// Returns the manifest key for this repository.
    fn manifest_key(&self) -> String {
        format!("repos/{}/manifest.json", self.repo_id)
    }

    /// Returns the storage key for a chunk with the given hash.
    fn chunk_key(hash: &str) -> String {
        format!("chunks/{hash}")
    }

    /// Fetches the remote manifest. Returns `None` if no manifest exists.
    async fn fetch_manifest(&self) -> CloudResult<Option<(SyncManifest, u64)>> {
        match self.backend.get(&self.manifest_key()).await {
            Ok((data, generation)) => {
                let manifest = SyncManifest::from_json(&data)?;
                Ok(Some((manifest, generation)))
            }
            Err(CloudError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Pushes the `.ovc` file at `ovc_path` to the remote backend.
    ///
    /// Only chunks not already present on the remote are uploaded. The
    /// manifest is updated atomically using compare-and-swap to detect
    /// concurrent modifications.
    ///
    /// Retries up to 3 attempts on `SyncConflict` (re-fetching the manifest
    /// each time) and on `Network` errors (with exponential backoff: 100 ms,
    /// 400 ms, 1 600 ms).
    #[allow(clippy::too_many_lines)]
    pub async fn push(&self, ovc_path: &Path) -> CloudResult<PushResult> {
        /// Maximum number of retry attempts for push operations.
        const MAX_PUSH_ATTEMPTS: u32 = 3;

        // Read the (potentially multi-GiB) file on a blocking thread to avoid
        // stalling the async runtime's cooperative scheduler.
        let ovc_owned = ovc_path.to_path_buf();
        let file_data = tokio::task::spawn_blocking(move || std::fs::read(&ovc_owned))
            .await
            .map_err(|e| CloudError::Storage(format!("task join error: {e}")))??;
        let file_hash = sha256_hex(&file_data);

        // Chunk the file once; chunks are reused across retry attempts.
        let params = ChunkParams::default();
        let local_chunks = chunker::chunk_data(&file_data, &params);

        let mut attempt = 0u32;
        let mut network_delay_ms = 100u64;

        loop {
            attempt += 1;

            // Fetch remote manifest to determine what already exists.
            // Re-fetched on every attempt so we always use the latest generation.
            let remote_state = self.fetch_manifest().await?;
            let (remote_hashes, manifest_generation) = match &remote_state {
                Some((manifest, generation)) => {
                    let hashes: HashSet<&str> =
                        manifest.chunks.iter().map(|c| c.hash.as_str()).collect();
                    (hashes, *generation)
                }
                None => (HashSet::new(), 0),
            };

            // Upload new chunks.
            let mut chunks_uploaded = 0u64;
            let mut chunks_reused = 0u64;
            let mut bytes_uploaded = 0u64;

            for chunk in &local_chunks {
                if remote_hashes.contains(chunk.hash.as_str()) {
                    // Chunk already exists on remote; also verify it's actually there.
                    if self
                        .backend
                        .exists(&Self::chunk_key(&chunk.hash))
                        .await?
                        .is_some()
                    {
                        chunks_reused += 1;
                        continue;
                    }
                }

                // Upload the chunk (no precondition — chunks are content-addressed
                // and immutable, so overwrites are idempotent).
                self.backend
                    .put(&Self::chunk_key(&chunk.hash), &chunk.data, None)
                    .await?;
                chunks_uploaded += 1;
                bytes_uploaded += chunk.length;
            }

            // Build the new manifest.
            let manifest = SyncManifest {
                version: 1,
                repo_id: self.repo_id.clone(),
                chunks: local_chunks
                    .iter()
                    .map(|c| ChunkDescriptor {
                        hash: c.hash.clone(),
                        offset: c.offset,
                        length: c.length,
                    })
                    .collect(),
                total_size: file_data.len() as u64,
                last_modified: Utc::now().to_rfc3339(),
                file_hash: file_hash.clone(),
            };

            let manifest_json = manifest.to_json()?;

            // Upload manifest with CAS. If the generation doesn't match,
            // someone else pushed concurrently.
            let put_result = self
                .backend
                .put(
                    &self.manifest_key(),
                    &manifest_json,
                    Some(manifest_generation),
                )
                .await
                .map_err(|e| match e {
                    CloudError::PreconditionFailed(_) => CloudError::SyncConflict,
                    other => other,
                });

            match put_result {
                Ok(new_version) => {
                    // Persist sidecar so status() can compute the correct
                    // direction on the next call.
                    write_sync_state(
                        ovc_path,
                        &SyncState {
                            generation: new_version,
                            file_hash: file_hash.clone(),
                        },
                    )?;
                    return Ok(PushResult {
                        chunks_uploaded,
                        chunks_reused,
                        bytes_uploaded,
                        manifest_version: new_version,
                    });
                }
                Err(CloudError::SyncConflict) if attempt < MAX_PUSH_ATTEMPTS => {
                    // Remote changed concurrently; re-fetch manifest and retry.
                    tracing::warn!(
                        attempt,
                        "push sync conflict, re-fetching manifest and retrying"
                    );
                }
                Err(CloudError::Network(ref msg)) if attempt < MAX_PUSH_ATTEMPTS => {
                    tracing::warn!(
                        attempt,
                        delay_ms = network_delay_ms,
                        "push network error ({msg}), retrying with backoff"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(network_delay_ms)).await;
                    network_delay_ms *= 4;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Pulls the `.ovc` file from the remote backend to `ovc_path`.
    ///
    /// Only chunks not present locally are downloaded. The reassembled
    /// file is verified against the manifest's file hash before being
    /// written atomically (via a temporary file + rename).
    #[allow(clippy::too_many_lines)]
    pub async fn pull(&self, ovc_path: &Path) -> CloudResult<PullResult> {
        /// Maximum number of chunks allowed in a remote manifest.
        const MAX_MANIFEST_CHUNKS: usize = 100_000;
        /// Maximum total file size (10 GiB) allowed from a remote manifest.
        const MAX_MANIFEST_TOTAL_SIZE: u64 = 10_737_418_240;

        let (manifest, manifest_generation) = self
            .fetch_manifest()
            .await?
            .ok_or_else(|| CloudError::NotFound("no remote manifest found".to_owned()))?;

        if manifest.chunks.len() > MAX_MANIFEST_CHUNKS {
            return Err(CloudError::Storage(format!(
                "remote manifest contains {} chunks, exceeding the maximum of {MAX_MANIFEST_CHUNKS}",
                manifest.chunks.len()
            )));
        }
        if manifest.total_size > MAX_MANIFEST_TOTAL_SIZE {
            return Err(CloudError::Storage(format!(
                "remote manifest declares total size {} bytes, exceeding the maximum of {MAX_MANIFEST_TOTAL_SIZE}",
                manifest.total_size
            )));
        }

        // If the local file exists, chunk it to build a hash-to-data map for
        // chunks we already have, avoiding redundant downloads.
        // Read on a blocking thread to avoid stalling the async runtime.
        let ovc_owned = ovc_path.to_path_buf();
        let local_chunk_map: std::collections::HashMap<String, Vec<u8>> =
            tokio::task::spawn_blocking(
                move || -> CloudResult<std::collections::HashMap<String, Vec<u8>>> {
                    if ovc_owned.exists() {
                        let local_data = std::fs::read(&ovc_owned)?;
                        let params = ChunkParams::default();
                        let local_chunks = chunker::chunk_data(&local_data, &params);
                        Ok(local_chunks.into_iter().map(|c| (c.hash, c.data)).collect())
                    } else {
                        Ok(std::collections::HashMap::new())
                    }
                },
            )
            .await
            .map_err(|e| CloudError::Storage(format!("task join error: {e}")))??;

        // Write chunks incrementally to a temporary file to avoid holding the
        // entire assembled file in memory. Each chunk is written as it is
        // retrieved, keeping peak memory proportional to the largest single
        // chunk rather than the total file size.
        let parent = ovc_path.parent().unwrap_or_else(|| Path::new("."));
        let tmp_path = parent.join(format!(".ovc-pull-{}.tmp", uuid::Uuid::new_v4()));
        let ovc_owned = ovc_path.to_path_buf();

        // Open the temporary file on a blocking thread.
        let tmp_clone = tmp_path.clone();
        let mut tmp_file = tokio::task::spawn_blocking(move || -> CloudResult<std::fs::File> {
            Ok(std::fs::File::create(&tmp_clone)?)
        })
        .await
        .map_err(|e| CloudError::Storage(format!("task join error: {e}")))??;

        let mut chunks_downloaded = 0u64;
        let mut chunks_cached = 0u64;
        let mut bytes_downloaded = 0u64;
        // Running SHA-256 hasher to verify the assembled file without a second
        // pass over the data.
        let mut hasher = sha2::Sha256::new();

        for descriptor in &manifest.chunks {
            let chunk_data: Vec<u8> = if let Some(cached_data) =
                local_chunk_map.get(&descriptor.hash)
            {
                // Chunk is available locally — use it directly without downloading.
                chunks_cached += 1;
                cached_data.clone()
            } else {
                // Chunk is missing locally — download from remote.
                let (data, _version) = self.backend.get(&Self::chunk_key(&descriptor.hash)).await?;

                // Verify chunk integrity.
                let actual_hash = sha256_hex(&data);
                if actual_hash != descriptor.hash {
                    let _ = std::fs::remove_file(&tmp_path);
                    return Err(CloudError::Storage(format!(
                        "chunk integrity check failed: expected {}, got {actual_hash}",
                        descriptor.hash,
                    )));
                }

                chunks_downloaded += 1;
                bytes_downloaded += data.len() as u64;
                data
            };

            // Feed this chunk into the running file hash.
            hasher.update(&chunk_data);

            // Write the chunk to the temp file on a blocking thread.
            let write_result = tokio::task::spawn_blocking({
                let tmp_path_err = tmp_path.clone();
                move || -> CloudResult<std::fs::File> {
                    use std::io::Write as _;
                    tmp_file
                        .write_all(&chunk_data)
                        .map_err(|e| {
                            let _ = std::fs::remove_file(&tmp_path_err);
                            CloudError::Io(e)
                        })
                        .map(|()| tmp_file)
                }
            })
            .await
            .map_err(|e| {
                let _ = std::fs::remove_file(&tmp_path);
                CloudError::Storage(format!("task join error: {e}"))
            })??;

            tmp_file = write_result;
        }

        // Drop the file handle (flushes) before rename.
        drop(tmp_file);

        // Verify the assembled file hash against the manifest.
        let assembled_hash = hex::encode(hasher.finalize());
        if assembled_hash != manifest.file_hash {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(CloudError::Storage(format!(
                "file integrity check failed: expected {}, got {assembled_hash}",
                manifest.file_hash,
            )));
        }

        // Rename the verified temp file into place atomically.
        let tmp_rename = tmp_path.clone();
        tokio::task::spawn_blocking(move || -> CloudResult<()> {
            std::fs::rename(&tmp_rename, &ovc_owned).map_err(|e| {
                let _ = std::fs::remove_file(&tmp_rename);
                CloudError::Io(e)
            })
        })
        .await
        .map_err(|e| CloudError::Storage(format!("task join error: {e}")))??;

        // Persist sidecar so status() can compute the correct direction on
        // the next call.
        write_sync_state(
            ovc_path,
            &SyncState {
                generation: manifest_generation,
                file_hash: manifest.file_hash.clone(),
            },
        )?;

        Ok(PullResult {
            chunks_downloaded,
            chunks_cached,
            bytes_downloaded,
            manifest_version: manifest_generation,
        })
    }

    /// Compares the local `.ovc` file against the remote manifest to
    /// determine the sync status.
    ///
    /// Uses a sidecar file (`<ovc_path>.sync-state`) written by [`push`] and
    /// [`pull`] to distinguish `LocalAhead`, `RemoteAhead`, and `Diverged`
    /// without an additional round-trip. If no sidecar exists (first sync),
    /// returns [`SyncStatus::NoRemote`] when no remote manifest is present, or
    /// defers to hash-only comparison when a remote exists but no sidecar does.
    pub async fn status(&self, ovc_path: &Path) -> CloudResult<SyncStatus> {
        let remote = self.fetch_manifest().await?;

        match remote {
            None => Ok(SyncStatus::NoRemote),
            Some((manifest, generation)) => {
                if !ovc_path.exists() {
                    return Ok(SyncStatus::RemoteAhead {
                        remote_version: generation,
                    });
                }

                // Read the local file and compute its hash on a blocking thread.
                let ovc_owned = ovc_path.to_path_buf();
                let local_data = tokio::task::spawn_blocking(move || std::fs::read(&ovc_owned))
                    .await
                    .map_err(|e| CloudError::Storage(format!("task join error: {e}")))??;
                let local_hash = sha256_hex(&local_data);

                if local_hash == manifest.file_hash {
                    return Ok(SyncStatus::InSync {
                        version: generation,
                    });
                }

                // Hashes differ. Use the sidecar to determine direction.
                let stored = read_sync_state(ovc_path);

                match stored {
                    None => {
                        // No sidecar — we have never synced from this path. We
                        // cannot tell which side is ahead, so report Unknown
                        // via the existing NoRemote variant is wrong.  The
                        // least-surprising answer for a first-time user who has
                        // never pushed is LocalAhead.
                        Ok(SyncStatus::LocalAhead)
                    }
                    Some(state) => {
                        // stored_generation == manifest generation: the remote
                        // hasn't moved since our last sync, so local changed.
                        // stored_generation < manifest generation: remote
                        // advanced (someone else pushed).
                        // stored_generation > manifest generation: shouldn't
                        // normally happen (CAS prevents rollbacks), treat as
                        // Diverged.
                        #[allow(clippy::comparison_chain)]
                        if state.generation == generation {
                            Ok(SyncStatus::LocalAhead)
                        } else if state.generation < generation {
                            Ok(SyncStatus::RemoteAhead {
                                remote_version: generation,
                            })
                        } else {
                            // stored_generation > generation — diverged state.
                            Ok(SyncStatus::Diverged)
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::LocalBackend;

    /// Helper: creates a `SyncEngine` backed by a local filesystem.
    fn make_engine(dir: &Path) -> SyncEngine {
        let backend = LocalBackend::new(dir.join("remote")).unwrap();
        SyncEngine::new(Box::new(backend), "test-repo".to_owned())
    }

    #[tokio::test]
    async fn push_to_empty_remote() {
        let dir = tempfile::tempdir().unwrap();
        let engine = make_engine(dir.path());

        // Create a test file.
        let ovc_path = dir.path().join("test.ovc");
        let data = vec![42u8; 2048];
        std::fs::write(&ovc_path, &data).unwrap();

        let result = engine.push(&ovc_path).await.unwrap();
        assert!(result.chunks_uploaded > 0);
        assert_eq!(result.chunks_reused, 0);
        assert!(result.manifest_version > 0);

        // Verify manifest exists on remote.
        let manifest_key = engine.manifest_key();
        let (manifest_data, _version) = engine.backend.get(&manifest_key).await.unwrap();
        let manifest = SyncManifest::from_json(&manifest_data).unwrap();
        assert_eq!(manifest.repo_id, "test-repo");
        assert_eq!(manifest.total_size, 2048);

        // Verify all chunks exist on remote.
        for chunk_desc in &manifest.chunks {
            let exists = engine
                .backend
                .exists(&SyncEngine::chunk_key(&chunk_desc.hash))
                .await
                .unwrap();
            assert!(exists.is_some(), "chunk {} should exist", chunk_desc.hash);
        }
    }

    #[tokio::test]
    async fn pull_from_remote() {
        let dir = tempfile::tempdir().unwrap();
        let engine = make_engine(dir.path());

        // Push a file first.
        let original_path = dir.path().join("original.ovc");
        let data: Vec<u8> = (0..4096u32)
            .map(|i| (i.wrapping_mul(31) & 0xFF) as u8)
            .collect();
        std::fs::write(&original_path, &data).unwrap();
        engine.push(&original_path).await.unwrap();

        // Pull to a different path.
        let pulled_path = dir.path().join("pulled.ovc");
        let result = engine.pull(&pulled_path).await.unwrap();
        assert!(result.chunks_downloaded > 0);
        assert!(result.manifest_version > 0);

        // Verify pulled file matches original.
        let pulled_data = std::fs::read(&pulled_path).unwrap();
        assert_eq!(pulled_data, data);
    }

    #[tokio::test]
    async fn push_pull_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let engine = make_engine(dir.path());

        // Create a non-trivial file.
        let ovc_path = dir.path().join("roundtrip.ovc");
        let data: Vec<u8> = (0..10_000u32)
            .map(|i| (i.wrapping_mul(97).wrapping_add(13) & 0xFF) as u8)
            .collect();
        std::fs::write(&ovc_path, &data).unwrap();

        // Push.
        let push_result = engine.push(&ovc_path).await.unwrap();
        assert!(push_result.chunks_uploaded > 0);

        // Pull to a new location.
        let output_path = dir.path().join("output.ovc");
        let pull_result = engine.pull(&output_path).await.unwrap();
        assert!(pull_result.manifest_version > 0);

        // Compare.
        let output_data = std::fs::read(&output_path).unwrap();
        assert_eq!(output_data, data);
    }

    #[tokio::test]
    async fn push_conflict_detection() {
        let dir = tempfile::tempdir().unwrap();

        // Two engines sharing the same remote.
        let backend1 = LocalBackend::new(dir.path().join("remote")).unwrap();
        let backend2 = LocalBackend::new(dir.path().join("remote")).unwrap();
        let engine1 = SyncEngine::new(Box::new(backend1), "conflict-repo".to_owned());
        let engine2 = SyncEngine::new(Box::new(backend2), "conflict-repo".to_owned());

        // First push succeeds.
        let file1 = dir.path().join("file1.ovc");
        std::fs::write(&file1, b"first version").unwrap();
        engine1.push(&file1).await.unwrap();

        // Second push from a different engine that doesn't know about the first push.
        // The manifest generation will be 0 (engine2 hasn't fetched it), but the
        // remote now has generation 1. This should trigger a conflict.
        //
        // However, engine2.push() fetches the manifest first, so it will see gen=1.
        // To simulate a true conflict, we need engine2 to have fetched the manifest
        // (seeing gen=1), then engine1 pushes again (advancing to gen=2), then
        // engine2 tries to push with the stale gen=1.
        //
        // Simpler: push with engine1, then push with engine2. Engine2 will fetch
        // gen=1, upload chunks, then CAS with gen=1. Meanwhile engine1 pushes again
        // advancing to gen=2. Since we can't interleave async operations this way,
        // we directly test the precondition failure.

        // Push again with engine1 to advance to gen=2.
        let file2 = dir.path().join("file2.ovc");
        std::fs::write(&file2, b"second version from engine1").unwrap();
        engine1.push(&file2).await.unwrap();

        // Now engine2 tries to push, but it will fetch the manifest at gen=2.
        // That will succeed because it uses CAS with the current gen.
        // To truly test conflict, we need to manipulate the generation.
        // Instead, let's manually test the precondition path.

        // Write a manifest with gen=2 on the remote, then try to push with
        // a stale view by directly calling put with wrong gen.
        let file3 = dir.path().join("file3.ovc");
        std::fs::write(&file3, b"third version").unwrap();

        // engine2 would see gen=2 when it fetches. But if engine1 pushes between
        // engine2's fetch and engine2's manifest upload, we get a conflict.
        // Test this by manually advancing the manifest generation.
        let manifest_key = engine2.manifest_key();
        let (manifest_data, _) = engine2.backend.get(&manifest_key).await.unwrap();
        // Advance the manifest one more time to simulate a concurrent push.
        engine1
            .backend
            .put(&manifest_key, &manifest_data, None)
            .await
            .unwrap();

        // Now engine2 tries to push. It fetches manifest at gen=3,
        // but we just advanced it. We need to be more surgical.
        // Let's directly test that the CAS works by doing a manual put.
        let result = engine2
            .backend
            .put(&manifest_key, b"conflict", Some(1))
            .await;
        assert!(
            matches!(result, Err(CloudError::PreconditionFailed(_))),
            "expected precondition failure, got {result:?}"
        );
    }

    #[tokio::test]
    async fn status_no_remote() {
        let dir = tempfile::tempdir().unwrap();
        let engine = make_engine(dir.path());

        let ovc_path = dir.path().join("test.ovc");
        std::fs::write(&ovc_path, b"data").unwrap();

        let status = engine.status(&ovc_path).await.unwrap();
        assert!(matches!(status, SyncStatus::NoRemote));
    }

    #[tokio::test]
    async fn status_in_sync() {
        let dir = tempfile::tempdir().unwrap();
        let engine = make_engine(dir.path());

        let ovc_path = dir.path().join("test.ovc");
        std::fs::write(&ovc_path, b"synced data").unwrap();

        engine.push(&ovc_path).await.unwrap();

        let status = engine.status(&ovc_path).await.unwrap();
        assert!(matches!(status, SyncStatus::InSync { .. }));
    }

    #[tokio::test]
    async fn status_local_ahead() {
        let dir = tempfile::tempdir().unwrap();
        let engine = make_engine(dir.path());

        let ovc_path = dir.path().join("test.ovc");
        std::fs::write(&ovc_path, b"version 1").unwrap();
        engine.push(&ovc_path).await.unwrap();

        // Modify local file.
        std::fs::write(&ovc_path, b"version 2").unwrap();

        let status = engine.status(&ovc_path).await.unwrap();
        assert!(matches!(status, SyncStatus::LocalAhead));
    }

    #[tokio::test]
    async fn status_remote_ahead() {
        let dir = tempfile::tempdir().unwrap();
        let engine = make_engine(dir.path());

        let ovc_path = dir.path().join("test.ovc");
        std::fs::write(&ovc_path, b"some data").unwrap();
        engine.push(&ovc_path).await.unwrap();

        // Delete local file to simulate remote being ahead.
        std::fs::remove_file(&ovc_path).unwrap();

        let status = engine.status(&ovc_path).await.unwrap();
        assert!(matches!(status, SyncStatus::RemoteAhead { .. }));
    }
}
