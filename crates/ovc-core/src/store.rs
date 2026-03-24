//! In-memory content-addressable object store.
//!
//! [`ObjectStore`] holds compressed, serialized objects keyed by their
//! [`ObjectId`]. Insertion is deduplicated: storing the same content twice
//! returns the same id without allocating additional storage.

use std::collections::BTreeMap;

use crate::compression;
use crate::error::{CoreError, CoreResult};
use crate::id::{self, ObjectId};
use crate::object::{Object, ObjectType};
use crate::serialize;

/// A compressed, typed blob stored in the object store.
#[derive(Debug, Clone)]
pub struct StoredObject {
    /// The compressed serialized bytes (without the type prefix).
    pub compressed_data: Vec<u8>,
    /// The object type.
    pub object_type: ObjectType,
}

/// In-memory content-addressable store for OVC objects.
#[derive(Debug, Clone)]
pub struct ObjectStore {
    objects: BTreeMap<ObjectId, StoredObject>,
    compression_level: i32,
}

impl ObjectStore {
    /// Creates a new empty object store.
    #[must_use]
    pub const fn new(compression_level: i32) -> Self {
        Self {
            objects: BTreeMap::new(),
            compression_level,
        }
    }

    /// Inserts an object into the store, returning its content address.
    ///
    /// If an object with the same content already exists, this is a no-op
    /// and the existing id is returned (content-addressable deduplication).
    pub fn insert(&mut self, obj: &Object) -> CoreResult<ObjectId> {
        // Compute the hash from the canonical (signature-stripped) serialization.
        let hash_serialized = serialize::serialize_object(obj)?;
        let hash_payload = &hash_serialized[1..];

        let oid = match obj {
            Object::Blob(data) => id::hash_blob(data),
            Object::Tree(_) => id::hash_tree(hash_payload),
            Object::Commit(_) => id::hash_commit(hash_payload),
            Object::Tag(_) => id::hash_tag(hash_payload),
        };

        // Deduplicate: skip compression + allocation if already present.
        if self.objects.contains_key(&oid) {
            return Ok(oid);
        }

        // For storage, use full serialization (including signature for commits).
        let storage_serialized = match obj {
            Object::Commit(commit) => serialize::serialize_commit_full(commit)?,
            _ => hash_serialized,
        };
        let type_byte = storage_serialized[0];
        let storage_payload = &storage_serialized[1..];

        let compressed = compression::compress(storage_payload, self.compression_level)?;

        let obj_type = ObjectType::from_u8(type_byte).ok_or_else(|| CoreError::CorruptObject {
            reason: format!("unknown type byte during insert: {type_byte}"),
        })?;

        self.objects.insert(
            oid,
            StoredObject {
                compressed_data: compressed,
                object_type: obj_type,
            },
        );

        Ok(oid)
    }

    /// Retrieves an object by its id, decompressing and deserializing it.
    ///
    /// Returns `Ok(None)` if the id is not present in the store.
    pub fn get(&self, oid: &ObjectId) -> CoreResult<Option<Object>> {
        let Some(stored) = self.objects.get(oid) else {
            return Ok(None);
        };

        let payload = compression::decompress(&stored.compressed_data)?;
        let obj = serialize::deserialize_object(stored.object_type as u8, &payload)?;
        Ok(Some(obj))
    }

    /// Returns `true` if the store contains an object with the given id.
    #[must_use]
    pub fn contains(&self, oid: &ObjectId) -> bool {
        self.objects.contains_key(oid)
    }

    /// Returns the number of objects in the store.
    #[must_use]
    pub fn count(&self) -> usize {
        self.objects.len()
    }

    /// Returns an iterator over all object ids in the store.
    pub fn ids(&self) -> impl Iterator<Item = &ObjectId> {
        self.objects.keys()
    }

    /// Resolve a hex prefix to a unique `ObjectId`.
    ///
    /// Returns `Ok(oid)` if exactly one object matches. Returns an error if
    /// the prefix is ambiguous (multiple matches) or not found.
    pub fn resolve_prefix(&self, hex_prefix: &str) -> CoreResult<ObjectId> {
        let lower = hex_prefix.to_ascii_lowercase();
        let mut matches = Vec::new();
        for oid in self.objects.keys() {
            if oid.to_string().starts_with(&lower) {
                matches.push(*oid);
                if matches.len() > 1 {
                    return Err(CoreError::Config {
                        reason: format!("ambiguous object prefix: {hex_prefix}"),
                    });
                }
            }
        }
        matches
            .into_iter()
            .next()
            .ok_or(CoreError::ObjectNotFound(ObjectId::ZERO))
    }

    /// Removes an object from the store. Returns `true` if it was present.
    pub fn remove(&mut self, oid: &ObjectId) -> bool {
        self.objects.remove(oid).is_some()
    }

    /// Retains only the objects for which the predicate returns `true`.
    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&ObjectId, &mut StoredObject) -> bool,
    {
        self.objects.retain(f);
    }

    /// Returns the total compressed bytes stored across all objects.
    #[must_use]
    pub fn total_compressed_bytes(&self) -> u64 {
        self.objects
            .values()
            .map(|s| u64::try_from(s.compressed_data.len()).unwrap_or(0))
            .sum()
    }

    /// Returns a reference to the internal map (for serialization during save).
    #[must_use]
    pub const fn raw_objects(&self) -> &BTreeMap<ObjectId, StoredObject> {
        &self.objects
    }

    /// Looks up a blob OID by following tree entries along `path` components.
    ///
    /// This is O(depth) instead of O(total entries) because it only decompresses
    /// and examines tree objects along the target path, not the entire tree.
    ///
    /// Returns `Ok(None)` if the path does not exist in the tree.
    pub fn lookup_path_in_tree(
        &self,
        tree_oid: &ObjectId,
        path: &str,
    ) -> CoreResult<Option<ObjectId>> {
        let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if components.is_empty() {
            return Ok(None);
        }

        let mut current_tree_oid = *tree_oid;

        for (i, component) in components.iter().enumerate() {
            let is_last = i == components.len() - 1;
            let obj = self.get(&current_tree_oid)?;
            let Some(Object::Tree(tree)) = obj else {
                return Ok(None);
            };

            let entry = tree.entries.iter().find(|e| e.name == component.as_bytes());
            let Some(entry) = entry else {
                return Ok(None);
            };

            if is_last {
                return Ok(Some(entry.oid));
            }
            // Intermediate: must be a directory — follow into it.
            current_tree_oid = entry.oid;
        }

        Ok(None)
    }
}

/// A serializable representation of a stored object for persistence.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredObjectEntry {
    /// The object type discriminant.
    pub object_type: ObjectType,
    /// The compressed payload bytes.
    pub compressed_data: Vec<u8>,
}

impl ObjectStore {
    /// Exports all objects as a serializable map for persistence in the superblock.
    #[must_use]
    pub fn export(&self) -> BTreeMap<ObjectId, StoredObjectEntry> {
        self.objects
            .iter()
            .map(|(oid, stored)| {
                (
                    *oid,
                    StoredObjectEntry {
                        object_type: stored.object_type,
                        compressed_data: stored.compressed_data.clone(),
                    },
                )
            })
            .collect()
    }

    /// Imports objects from a serialized map, replacing current contents.
    pub fn import(&mut self, entries: BTreeMap<ObjectId, StoredObjectEntry>) {
        self.objects = entries
            .into_iter()
            .map(|(oid, entry)| {
                (
                    oid,
                    StoredObject {
                        object_type: entry.object_type,
                        compressed_data: entry.compressed_data,
                    },
                )
            })
            .collect();
    }
}

impl Default for ObjectStore {
    fn default() -> Self {
        Self::new(compression::DEFAULT_COMPRESSION_LEVEL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::*;

    #[test]
    fn insert_and_get_blob() {
        let mut store = ObjectStore::default();
        let obj = Object::Blob(b"hello world".to_vec());
        let oid = store.insert(&obj).unwrap();
        assert!(!oid.is_zero());

        let retrieved = store.get(&oid).unwrap().unwrap();
        assert_eq!(retrieved, obj);
    }

    #[test]
    fn deduplication() {
        let mut store = ObjectStore::default();
        let obj = Object::Blob(b"duplicate".to_vec());
        let oid1 = store.insert(&obj).unwrap();
        let oid2 = store.insert(&obj).unwrap();
        assert_eq!(oid1, oid2);
        assert_eq!(store.count(), 1);
    }

    #[test]
    fn missing_object() {
        let store = ObjectStore::default();
        let oid = id::hash_blob(b"nonexistent");
        assert!(!store.contains(&oid));
        assert!(store.get(&oid).unwrap().is_none());
    }

    #[test]
    fn insert_tree() {
        let mut store = ObjectStore::default();
        let tree = Object::Tree(Tree {
            entries: vec![TreeEntry {
                mode: FileMode::Regular,
                name: b"file.txt".to_vec(),
                oid: id::hash_blob(b"content"),
            }],
        });
        let oid = store.insert(&tree).unwrap();
        let back = store.get(&oid).unwrap().unwrap();
        assert_eq!(back, tree);
    }
}
