//! `.ovc` binary file format structures.
//!
//! An `.ovc` file has the following layout:
//!
//! ```text
//! ┌──────────────────────────────────────────┐
//! │ FileHeader (64 bytes)                    │
//! ├──────────────────────────────────────────┤
//! │ Encrypted Segment 0                      │
//! │ Encrypted Segment 1                      │
//! │ ...                                      │
//! ├──────────────────────────────────────────┤
//! │ Encrypted Superblock                     │
//! ├──────────────────────────────────────────┤
//! │ FileTrailer (32 bytes)                   │
//! └──────────────────────────────────────────┘
//! ```
//!
//! The header is unencrypted and contains the KDF parameters needed to
//! derive the master key. The trailer points back to the superblock.
//! The superblock (encrypted) contains the segment index, refs, and config.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::access::AccessControl;
use crate::compression::CompressionAlgorithm;
use crate::crypto::{CipherAlgorithm, KdfAlgorithm};
use crate::error::{CoreError, CoreResult};
use crate::id::ObjectId;
use crate::index::Index;
use crate::keys::SealedKey;
use crate::object::ObjectType;
use crate::pulls::PullRequestStore;
use crate::refs::RefStore;
use crate::stash::StashStore;
use crate::store::StoredObjectEntry;
use crate::submodule::SubmoduleConfig;

/// Magic bytes identifying a `.ovc` file: `OVC\x00`.
pub const MAGIC: [u8; 4] = [b'O', b'V', b'C', 0x00];

/// Current format version.
pub const FORMAT_VERSION: u16 = 1;

/// Minimum reader version that can open files written by this version.
pub const MIN_READER_VERSION: u16 = 1;

/// Size of the file header in bytes.
pub const HEADER_SIZE: usize = 64;

/// Size of the file trailer in bytes.
pub const TRAILER_SIZE: usize = 32;

/// The unencrypted header at the start of every `.ovc` file.
///
/// Fixed size: 64 bytes. Contains everything needed to derive the master
/// key before any decryption can occur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHeader {
    /// Format version of this file.
    pub format_version: u16,
    /// Minimum reader version required.
    pub min_reader_version: u16,
    /// KDF algorithm used.
    pub kdf_algorithm: KdfAlgorithm,
    /// Cipher algorithm used.
    pub cipher_algorithm: CipherAlgorithm,
    /// Compression algorithm used.
    pub compression_algorithm: CompressionAlgorithm,
    /// Argon2 time cost (iterations).
    pub argon2_time_cost: u32,
    /// Argon2 memory cost in KiB.
    pub argon2_memory_cost_kib: u32,
    /// Argon2 parallelism.
    pub argon2_parallelism: u8,
    /// KDF salt (32 bytes).
    pub kdf_salt: [u8; 32],
}

impl FileHeader {
    /// Serializes the header into a fixed 64-byte array.
    ///
    /// Layout (byte offsets):
    /// - `[0..4]`   magic `OVC\x00`
    /// - `[4..6]`   `format_version` (LE)
    /// - `[6..8]`   `min_reader_version` (LE)
    /// - `[8]`      `kdf_algorithm`
    /// - `[9]`      `cipher_algorithm`
    /// - `[10]`     `compression_algorithm`
    /// - `[11..15]` `argon2_time_cost` (LE)
    /// - `[15..19]` `argon2_memory_cost_kib` (LE)
    /// - `[19]`     `argon2_parallelism`
    /// - `[20..52]` `kdf_salt`
    /// - `[52..64]` reserved (zeros)
    #[must_use]
    pub fn serialize(&self) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];
        buf[0..4].copy_from_slice(&MAGIC);
        buf[4..6].copy_from_slice(&self.format_version.to_le_bytes());
        buf[6..8].copy_from_slice(&self.min_reader_version.to_le_bytes());
        buf[8] = self.kdf_algorithm as u8;
        buf[9] = self.cipher_algorithm as u8;
        buf[10] = self.compression_algorithm as u8;
        buf[11..15].copy_from_slice(&self.argon2_time_cost.to_le_bytes());
        buf[15..19].copy_from_slice(&self.argon2_memory_cost_kib.to_le_bytes());
        buf[19] = self.argon2_parallelism;
        buf[20..52].copy_from_slice(&self.kdf_salt);
        // [52..64] reserved
        buf
    }

    /// Deserializes a header from a 64-byte array.
    pub fn deserialize(bytes: &[u8; HEADER_SIZE]) -> CoreResult<Self> {
        if bytes[0..4] != MAGIC {
            return Err(CoreError::FormatError {
                reason: "invalid magic bytes — not an .ovc file".into(),
            });
        }

        let format_version = u16::from_le_bytes([bytes[4], bytes[5]]);
        let min_reader_version = u16::from_le_bytes([bytes[6], bytes[7]]);

        if format_version < MIN_READER_VERSION {
            return Err(CoreError::FormatError {
                reason: format!(
                    "file format version {format_version} is too old (minimum: {MIN_READER_VERSION})"
                ),
            });
        }

        let kdf_algorithm =
            KdfAlgorithm::from_u8(bytes[8]).ok_or_else(|| CoreError::FormatError {
                reason: format!("unknown KDF algorithm: {}", bytes[8]),
            })?;

        let cipher_algorithm =
            CipherAlgorithm::from_u8(bytes[9]).ok_or_else(|| CoreError::FormatError {
                reason: format!("unknown cipher algorithm: {}", bytes[9]),
            })?;

        let compression_algorithm =
            CompressionAlgorithm::from_u8(bytes[10]).ok_or_else(|| CoreError::FormatError {
                reason: format!("unknown compression algorithm: {}", bytes[10]),
            })?;

        let argon2_time_cost = u32::from_le_bytes([bytes[11], bytes[12], bytes[13], bytes[14]]);
        let argon2_memory_cost_kib =
            u32::from_le_bytes([bytes[15], bytes[16], bytes[17], bytes[18]]);
        let argon2_parallelism = bytes[19];

        // Validate Argon2 parallelism here so every code path that calls
        // `FileHeader::deserialize` (both `Repository::open` and
        // `Repository::open_with_key`) benefits from the check. A crafted
        // superblock with p_cost = 0 would cause `argon2::Params::new` to
        // return an error; p_cost > 16 is unreasonably high for any legitimate
        // use case and could cause excessive resource consumption.
        if !(1..=16).contains(&argon2_parallelism) {
            return Err(CoreError::FormatError {
                reason: format!(
                    "argon2 parallelism {argon2_parallelism} is out of allowed range [1, 16]"
                ),
            });
        }

        let mut kdf_salt = [0u8; 32];
        kdf_salt.copy_from_slice(&bytes[20..52]);

        Ok(Self {
            format_version,
            min_reader_version,
            kdf_algorithm,
            cipher_algorithm,
            compression_algorithm,
            argon2_time_cost,
            argon2_memory_cost_kib,
            argon2_parallelism,
            kdf_salt,
        })
    }
}

/// The unencrypted trailer at the end of every `.ovc` file.
///
/// Fixed size: 32 bytes. Points back to the encrypted superblock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTrailer {
    /// Byte offset of the encrypted superblock from the start of the file.
    pub superblock_offset: u64,
    /// Length of the encrypted superblock in bytes.
    pub superblock_length: u64,
    /// Monotonically increasing sequence number for file writes.
    pub file_sequence: u64,
    /// First 8 bytes of an HMAC over the trailer fields (truncated for space).
    pub trailer_hmac_truncated: [u8; 8],
}

impl FileTrailer {
    /// Serializes the trailer into a fixed 32-byte array.
    ///
    /// Layout (byte offsets):
    /// - `[0..8]`   `superblock_offset` (LE)
    /// - `[8..16]`  `superblock_length` (LE)
    /// - `[16..24]` `file_sequence` (LE)
    /// - `[24..32]` `trailer_hmac_truncated`
    #[must_use]
    pub fn serialize(&self) -> [u8; TRAILER_SIZE] {
        let mut buf = [0u8; TRAILER_SIZE];
        buf[0..8].copy_from_slice(&self.superblock_offset.to_le_bytes());
        buf[8..16].copy_from_slice(&self.superblock_length.to_le_bytes());
        buf[16..24].copy_from_slice(&self.file_sequence.to_le_bytes());
        buf[24..32].copy_from_slice(&self.trailer_hmac_truncated);
        buf
    }

    /// Deserializes a trailer from a 32-byte array.
    pub fn deserialize(bytes: &[u8; TRAILER_SIZE]) -> CoreResult<Self> {
        let superblock_offset = u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        let superblock_length = u64::from_le_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        let file_sequence = u64::from_le_bytes([
            bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22], bytes[23],
        ]);
        let mut trailer_hmac_truncated = [0u8; 8];
        trailer_hmac_truncated.copy_from_slice(&bytes[24..32]);

        Ok(Self {
            superblock_offset,
            superblock_length,
            file_sequence,
            trailer_hmac_truncated,
        })
    }
}

/// The encrypted superblock containing repository metadata, keys, and index.
///
/// Sensitive fields (encryption keys, HMAC keys) are not exposed via `Debug`.
#[derive(Clone, Serialize, Deserialize)]
pub struct Superblock {
    /// The encryption key used for data segments (encrypted by the master key).
    #[serde(with = "serde_byte_array_32")]
    pub segment_encryption_key: [u8; 32],

    /// Byte offset of the segment index within the plaintext superblock.
    pub index_offset: u64,

    /// Length of the segment index in bytes.
    pub index_length: u64,

    /// Nonce used to encrypt the segment index.
    #[serde(with = "serde_byte_array_24")]
    pub index_nonce: [u8; 24],

    /// The current HEAD reference (branch name, e.g., `"refs/heads/main"`).
    pub head_ref: String,

    /// Unix timestamp when the repository was created.
    pub created_at: i64,

    /// Named references (branches, tags) -- legacy field kept for format compat.
    pub refs: BTreeMap<String, ObjectId>,

    /// Repository configuration.
    pub config: crate::config::RepositoryConfig,

    /// HMAC key for trailer integrity verification.
    #[serde(with = "serde_byte_array_32")]
    pub hmac_key: [u8; 32],

    /// Persisted object store: maps `ObjectId` to (type, compressed data).
    #[serde(default)]
    pub stored_objects: BTreeMap<ObjectId, StoredObjectEntry>,

    /// Persisted reference store.
    #[serde(default)]
    pub ref_store: RefStore,

    /// Persisted staging index.
    #[serde(default)]
    pub staging_index: Index,

    /// Persisted stash store.
    #[serde(default)]
    pub stash_store: StashStore,

    /// Key slots: sealed copies of the segment encryption key for each
    /// authorized SSH key pair. Empty for password-only repositories.
    #[serde(default)]
    pub key_slots: Vec<SealedKey>,

    /// Commit annotations (notes), keyed by commit `ObjectId`.
    #[serde(default)]
    pub notes: BTreeMap<ObjectId, String>,

    /// Submodule configurations, keyed by submodule name.
    #[serde(default)]
    pub submodules: BTreeMap<String, SubmoduleConfig>,

    /// Per-user access control list. Empty means legacy mode (no enforcement).
    #[serde(default)]
    pub access_control: AccessControl,

    /// Pull request store (encrypted inside the superblock).
    #[serde(default)]
    pub pull_request_store: PullRequestStore,
}

impl std::fmt::Debug for Superblock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Superblock")
            .field("segment_encryption_key", &"[REDACTED]")
            .field("index_offset", &self.index_offset)
            .field("index_length", &self.index_length)
            .field("index_nonce", &"[REDACTED]")
            .field("head_ref", &self.head_ref)
            .field("created_at", &self.created_at)
            .field("refs", &self.refs)
            .field("config", &self.config)
            .field("hmac_key", &"[REDACTED]")
            .field("stored_objects_count", &self.stored_objects.len())
            .field("ref_store", &self.ref_store)
            .field("staging_index_entries", &self.staging_index.entries().len())
            .field("stash_entries", &self.stash_store.list().len())
            .field("key_slots", &self.key_slots.len())
            .field("notes_count", &self.notes.len())
            .field("submodules_count", &self.submodules.len())
            .field("access_control_users", &self.access_control.users.len())
            .field(
                "pull_requests",
                &self.pull_request_store.pull_requests.len(),
            )
            .finish()
    }
}

/// Describes a single encrypted segment on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentDescriptor {
    /// Byte offset from the start of the file.
    pub file_offset: u64,
    /// Length of the segment on disk (including nonce + ciphertext + tag).
    pub disk_length: u64,
}

/// Location of a single object within the decoded segment data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectLocation {
    /// Index into the `SegmentIndex::segments` vec.
    pub segment_index: u32,
    /// Byte offset within the decrypted segment data.
    pub offset_in_segment: u64,
    /// The type of this object.
    pub object_type: ObjectType,
    /// Size of the serialized object data in bytes (excluding type byte).
    pub object_size: u64,
}

/// Index mapping object ids to their storage locations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentIndex {
    /// Descriptors for each encrypted segment in the file.
    pub segments: Vec<SegmentDescriptor>,
    /// Map from object id to its location.
    pub objects: BTreeMap<ObjectId, ObjectLocation>,
}

impl SegmentIndex {
    /// Creates an empty segment index.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            segments: Vec::new(),
            objects: BTreeMap::new(),
        }
    }
}

impl Default for SegmentIndex {
    fn default() -> Self {
        Self::new()
    }
}

// ── Serde helpers for fixed-size byte arrays ────────────────────────────

mod serde_byte_array_32 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u8; 32], D::Error> {
        let v = <Vec<u8>>::deserialize(deserializer)?;
        <[u8; 32]>::try_from(v.as_slice())
            .map_err(|_| serde::de::Error::custom(format!("expected 32 bytes, got {}", v.len())))
    }
}

mod serde_byte_array_24 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8; 24], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u8; 24], D::Error> {
        let v = <Vec<u8>>::deserialize(deserializer)?;
        <[u8; 24]>::try_from(v.as_slice())
            .map_err(|_| serde::de::Error::custom(format!("expected 24 bytes, got {}", v.len())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_round_trip() {
        let header = FileHeader {
            format_version: FORMAT_VERSION,
            min_reader_version: MIN_READER_VERSION,
            kdf_algorithm: KdfAlgorithm::Argon2id,
            cipher_algorithm: CipherAlgorithm::XChaCha20Poly1305,
            compression_algorithm: CompressionAlgorithm::Zstd,
            argon2_time_cost: 3,
            argon2_memory_cost_kib: 65536,
            argon2_parallelism: 4,
            kdf_salt: [0xAB; 32],
        };

        let bytes = header.serialize();
        assert_eq!(bytes.len(), HEADER_SIZE);
        assert_eq!(&bytes[0..4], &MAGIC);

        let parsed = FileHeader::deserialize(&bytes).unwrap();
        assert_eq!(parsed, header);
    }

    #[test]
    fn header_bad_magic() {
        let mut bytes = [0u8; HEADER_SIZE];
        bytes[0..4].copy_from_slice(b"NOPE");
        assert!(FileHeader::deserialize(&bytes).is_err());
    }

    #[test]
    fn trailer_round_trip() {
        let trailer = FileTrailer {
            superblock_offset: 12345,
            superblock_length: 6789,
            file_sequence: 42,
            trailer_hmac_truncated: [1, 2, 3, 4, 5, 6, 7, 8],
        };

        let bytes = trailer.serialize();
        assert_eq!(bytes.len(), TRAILER_SIZE);

        let parsed = FileTrailer::deserialize(&bytes).unwrap();
        assert_eq!(parsed, trailer);
    }
}
