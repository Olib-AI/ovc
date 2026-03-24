//! Sync manifest describing the chunked representation of a `.ovc` file.
//!
//! The manifest is stored in the remote backend as
//! `repos/{repo_id}/manifest.json`. It records the ordered list of
//! chunk descriptors that compose the full file, enabling the sync
//! engine to diff local vs. remote and transfer only changed chunks.

use serde::{Deserialize, Serialize};

use crate::error::{CloudError, CloudResult};

/// Describes the full chunked representation of a `.ovc` file at a
/// point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncManifest {
    /// Manifest schema version (currently 1).
    pub version: u64,
    /// Unique identifier for the repository.
    pub repo_id: String,
    /// Ordered list of chunks composing the file.
    pub chunks: Vec<ChunkDescriptor>,
    /// Total size of the original file in bytes.
    pub total_size: u64,
    /// ISO 8601 timestamp of the last modification.
    pub last_modified: String,
    /// SHA-256 hex digest of the complete `.ovc` file.
    pub file_hash: String,
}

/// Describes a single chunk within a [`SyncManifest`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChunkDescriptor {
    /// SHA-256 hex digest of the chunk data.
    pub hash: String,
    /// Byte offset of the chunk in the original file.
    pub offset: u64,
    /// Length of the chunk in bytes.
    pub length: u64,
}

impl SyncManifest {
    /// Serializes the manifest to a JSON byte vector.
    pub fn to_json(&self) -> CloudResult<Vec<u8>> {
        serde_json::to_vec_pretty(self)
            .map_err(|e| CloudError::Serialization(format!("failed to serialize manifest: {e}")))
    }

    /// Deserializes a manifest from JSON bytes.
    pub fn from_json(data: &[u8]) -> CloudResult<Self> {
        serde_json::from_slice(data)
            .map_err(|e| CloudError::Serialization(format!("failed to deserialize manifest: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_deserialize_round_trip() {
        let manifest = SyncManifest {
            version: 1,
            repo_id: "test-repo-123".to_owned(),
            chunks: vec![
                ChunkDescriptor {
                    hash: "abcdef1234567890".to_owned(),
                    offset: 0,
                    length: 1024,
                },
                ChunkDescriptor {
                    hash: "1234567890abcdef".to_owned(),
                    offset: 1024,
                    length: 2048,
                },
            ],
            total_size: 3072,
            last_modified: "2026-03-21T12:00:00Z".to_owned(),
            file_hash: "deadbeef".to_owned(),
        };

        let json = manifest.to_json().unwrap();
        let deserialized = SyncManifest::from_json(&json).unwrap();

        assert_eq!(deserialized.version, manifest.version);
        assert_eq!(deserialized.repo_id, manifest.repo_id);
        assert_eq!(deserialized.chunks.len(), 2);
        assert_eq!(deserialized.chunks[0].hash, "abcdef1234567890");
        assert_eq!(deserialized.chunks[1].offset, 1024);
        assert_eq!(deserialized.total_size, 3072);
        assert_eq!(deserialized.file_hash, "deadbeef");
        assert_eq!(deserialized.last_modified, "2026-03-21T12:00:00Z");
    }
}
