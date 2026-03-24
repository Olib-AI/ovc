//! Canonical binary serialization of OVC objects.
//!
//! Objects are serialized using [postcard](https://docs.rs/postcard) for
//! compact, deterministic encoding. The on-disk format prepends a single
//! type byte ([`ObjectType`] discriminant) followed by the postcard payload.
//!
//! **Important:** When serializing a [`Commit`] for hashing, the `signature`
//! field is excluded so that a detached signature does not alter the commit's
//! content address.

use crate::error::{CoreError, CoreResult};
use crate::object::{Commit, Object, ObjectType};

/// Serializes an [`Object`] into its canonical byte representation.
///
/// The returned bytes consist of a one-byte type tag followed by the
/// postcard-encoded payload. For blobs, the payload is the raw data
/// (no postcard framing).
pub fn serialize_object(obj: &Object) -> CoreResult<Vec<u8>> {
    let type_byte = obj.object_type() as u8;
    let payload = match obj {
        Object::Blob(data) => {
            let mut buf = Vec::with_capacity(1 + data.len());
            buf.push(type_byte);
            buf.extend_from_slice(data);
            return Ok(buf);
        }
        Object::Tree(tree) => postcard::to_allocvec(tree),
        Object::Commit(commit) => {
            // Serialize without signature so the hash is signature-independent.
            let hashable = CommitForHashing {
                tree: &commit.tree,
                parents: &commit.parents,
                author: &commit.author,
                committer: &commit.committer,
                message: &commit.message,
                sequence: commit.sequence,
            };
            postcard::to_allocvec(&hashable)
        }
        Object::Tag(tag) => postcard::to_allocvec(tag),
    }
    .map_err(|e| CoreError::Serialization {
        reason: e.to_string(),
    })?;

    let mut buf = Vec::with_capacity(1 + payload.len());
    buf.push(type_byte);
    buf.extend_from_slice(&payload);
    Ok(buf)
}

/// Serializes a [`Commit`] including its signature field (for storage, not hashing).
pub fn serialize_commit_full(commit: &Commit) -> CoreResult<Vec<u8>> {
    let payload = postcard::to_allocvec(commit).map_err(|e| CoreError::Serialization {
        reason: e.to_string(),
    })?;
    let mut buf = Vec::with_capacity(1 + payload.len());
    buf.push(ObjectType::Commit as u8);
    buf.extend_from_slice(&payload);
    Ok(buf)
}

/// Deserializes an [`Object`] from its type byte and payload.
///
/// The `type_byte` is the first byte produced by [`serialize_object`], and
/// `data` is the remaining bytes (the payload without the type prefix).
pub fn deserialize_object(type_byte: u8, data: &[u8]) -> CoreResult<Object> {
    let obj_type = ObjectType::from_u8(type_byte).ok_or_else(|| CoreError::CorruptObject {
        reason: format!("unknown object type byte: {type_byte}"),
    })?;

    match obj_type {
        ObjectType::Blob => Ok(Object::Blob(data.to_vec())),
        ObjectType::Tree => {
            let tree = postcard::from_bytes(data).map_err(|e| CoreError::Serialization {
                reason: format!("failed to deserialize tree: {e}"),
            })?;
            Ok(Object::Tree(tree))
        }
        ObjectType::Commit => {
            let commit = postcard::from_bytes(data).map_err(|e| CoreError::Serialization {
                reason: format!("failed to deserialize commit: {e}"),
            })?;
            Ok(Object::Commit(commit))
        }
        ObjectType::Tag => {
            let tag = postcard::from_bytes(data).map_err(|e| CoreError::Serialization {
                reason: format!("failed to deserialize tag: {e}"),
            })?;
            Ok(Object::Tag(tag))
        }
    }
}

/// Internal borrow-only struct used to hash commits without the signature.
#[derive(serde::Serialize)]
struct CommitForHashing<'a> {
    tree: &'a crate::id::ObjectId,
    parents: &'a [crate::id::ObjectId],
    author: &'a crate::object::Identity,
    committer: &'a crate::object::Identity,
    message: &'a str,
    sequence: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id;
    use crate::object::*;

    fn sample_commit() -> Commit {
        Commit {
            tree: id::hash_tree(b"root"),
            parents: vec![],
            author: Identity {
                name: "Alice".into(),
                email: "alice@example.com".into(),
                timestamp: 1_700_000_000,
                tz_offset_minutes: -480,
            },
            committer: Identity {
                name: "Alice".into(),
                email: "alice@example.com".into(),
                timestamp: 1_700_000_000,
                tz_offset_minutes: -480,
            },
            message: "initial commit".into(),
            signature: None,
            sequence: 1,
        }
    }

    #[test]
    fn blob_round_trip() {
        let obj = Object::Blob(b"hello world".to_vec());
        let bytes = serialize_object(&obj).unwrap();
        assert_eq!(bytes[0], ObjectType::Blob as u8);
        let back = deserialize_object(bytes[0], &bytes[1..]).unwrap();
        assert_eq!(obj, back);
    }

    #[test]
    fn tree_round_trip() {
        let obj = Object::Tree(Tree {
            entries: vec![TreeEntry {
                mode: FileMode::Regular,
                name: b"README.md".to_vec(),
                oid: id::hash_blob(b"readme content"),
            }],
        });
        let bytes = serialize_object(&obj).unwrap();
        let back = deserialize_object(bytes[0], &bytes[1..]).unwrap();
        assert_eq!(obj, back);
    }

    #[test]
    fn commit_full_round_trip() {
        let obj = Object::Commit(sample_commit());
        // Full serialization (with signature field) round-trips correctly.
        let bytes_full = serialize_commit_full(&sample_commit()).unwrap();
        let back_full = deserialize_object(bytes_full[0], &bytes_full[1..]).unwrap();
        assert_eq!(obj, back_full);
    }

    #[test]
    fn commit_hash_serialization_is_deterministic() {
        // The hash-only serialization (via serialize_object) excludes the
        // signature, so it is NOT round-trippable via deserialize_object.
        // But it IS deterministic for hashing purposes.
        let commit = sample_commit();
        let bytes1 = serialize_object(&Object::Commit(commit.clone())).unwrap();
        let bytes2 = serialize_object(&Object::Commit(commit)).unwrap();
        assert_eq!(bytes1, bytes2);
    }

    #[test]
    fn unknown_type_byte() {
        assert!(deserialize_object(255, b"data").is_err());
    }
}
