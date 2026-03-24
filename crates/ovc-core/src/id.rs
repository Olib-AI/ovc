//! Content-addressable object identifiers based on BLAKE3.
//!
//! [`ObjectId`] is a 32-byte BLAKE3 hash used to uniquely identify every
//! object in the OVC store. Domain separation is achieved via BLAKE3's
//! `derive_key` facility, ensuring that a blob and a tree with identical
//! bytes will never collide.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A 32-byte BLAKE3 content-address used to identify objects.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId([u8; 32]);

impl ObjectId {
    /// The all-zero ID, used as a sentinel for "no object."
    pub const ZERO: Self = Self([0u8; 32]);

    /// Creates an `ObjectId` from raw bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the raw bytes of this identifier.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns `true` if this is the all-zero sentinel.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        *self == Self::ZERO
    }
}

// ── Domain-separated hashing ────────────────────────────────────────────

const DOMAIN_BLOB: &str = "ovc 2024-01 blob";
const DOMAIN_TREE: &str = "ovc 2024-01 tree";
const DOMAIN_COMMIT: &str = "ovc 2024-01 commit";
const DOMAIN_TAG: &str = "ovc 2024-01 tag";

/// Hashes raw blob data with domain separation.
#[must_use]
pub fn hash_blob(data: &[u8]) -> ObjectId {
    let hash = blake3::derive_key(DOMAIN_BLOB, data);
    ObjectId(hash)
}

/// Hashes serialized tree data with domain separation.
#[must_use]
pub fn hash_tree(data: &[u8]) -> ObjectId {
    let hash = blake3::derive_key(DOMAIN_TREE, data);
    ObjectId(hash)
}

/// Hashes serialized commit data with domain separation.
#[must_use]
pub fn hash_commit(data: &[u8]) -> ObjectId {
    let hash = blake3::derive_key(DOMAIN_COMMIT, data);
    ObjectId(hash)
}

/// Hashes serialized tag data with domain separation.
#[must_use]
pub fn hash_tag(data: &[u8]) -> ObjectId {
    let hash = blake3::derive_key(DOMAIN_TAG, data);
    ObjectId(hash)
}

// ── Display / Debug / FromStr ───────────────────────────────────────────

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Short hex: first 8 bytes (16 hex chars)
        write!(f, "ObjectId(")?;
        for byte in &self.0[..8] {
            write!(f, "{byte:02x}")?;
        }
        write!(f, "..)")
    }
}

/// Error returned when parsing a hex string into an [`ObjectId`] fails.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ParseObjectIdError {
    /// The hex string has the wrong length.
    #[error("expected 64 hex characters, got {len}")]
    WrongLength {
        /// Actual length of the input.
        len: usize,
    },
    /// The hex string contains invalid characters.
    #[error("invalid hex character at position {position}")]
    InvalidHex {
        /// Position of the first bad character.
        position: usize,
    },
}

impl FromStr for ObjectId {
    type Err = ParseObjectIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 64 {
            return Err(ParseObjectIdError::WrongLength { len: s.len() });
        }
        let mut bytes = [0u8; 32];
        for (i, chunk) in s.as_bytes().chunks_exact(2).enumerate() {
            let hi =
                hex_nibble(chunk[0]).ok_or(ParseObjectIdError::InvalidHex { position: i * 2 })?;
            let lo = hex_nibble(chunk[1]).ok_or(ParseObjectIdError::InvalidHex {
                position: i * 2 + 1,
            })?;
            bytes[i] = (hi << 4) | lo;
        }
        Ok(Self(bytes))
    }
}

/// Converts an ASCII hex character to its 4-bit value.
const fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ── Serde as hex strings ────────────────────────────────────────────────

impl Serialize for ObjectId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.to_string())
        } else {
            serializer.serialize_bytes(&self.0)
        }
    }
}

impl<'de> Deserialize<'de> for ObjectId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let s = String::deserialize(deserializer)?;
            s.parse().map_err(serde::de::Error::custom)
        } else {
            let bytes = <Vec<u8>>::deserialize(deserializer)?;
            if bytes.len() != 32 {
                return Err(serde::de::Error::custom(format!(
                    "expected 32 bytes for ObjectId, got {}",
                    bytes.len()
                )));
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            Ok(Self(arr))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_zero() {
        assert!(ObjectId::ZERO.is_zero());
    }

    #[test]
    fn hash_blob_deterministic() {
        let a = hash_blob(b"hello");
        let b = hash_blob(b"hello");
        assert_eq!(a, b);
    }

    #[test]
    fn domain_separation() {
        let data = b"same data";
        assert_ne!(hash_blob(data), hash_tree(data));
        assert_ne!(hash_blob(data), hash_commit(data));
        assert_ne!(hash_tree(data), hash_tag(data));
    }

    #[test]
    fn display_and_parse_round_trip() {
        let id = hash_blob(b"test");
        let hex = id.to_string();
        assert_eq!(hex.len(), 64);
        let parsed: ObjectId = hex.parse().unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn serde_json_round_trip() {
        let id = hash_blob(b"serde test");
        let json = serde_json::to_string(&id).unwrap();
        let back: ObjectId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn parse_wrong_length() {
        assert!("abc".parse::<ObjectId>().is_err());
    }

    #[test]
    fn parse_invalid_hex() {
        let bad = "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz";
        assert!(bad.parse::<ObjectId>().is_err());
    }
}
