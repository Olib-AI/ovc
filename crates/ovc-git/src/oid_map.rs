//! Bidirectional mapping between git SHA1 hex strings and OVC `ObjectId`s.

use std::collections::BTreeMap;

use ovc_core::id::ObjectId;
use serde::{Deserialize, Serialize};

/// A bidirectional map between git SHA1 hex strings and OVC object identifiers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OidMap {
    git_to_ovc: BTreeMap<String, ObjectId>,
    ovc_to_git: BTreeMap<ObjectId, String>,
}

impl OidMap {
    /// Creates an empty mapping.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a bidirectional association.
    pub fn insert(&mut self, git_sha1: &str, ovc_id: ObjectId) {
        self.git_to_ovc.insert(git_sha1.to_owned(), ovc_id);
        self.ovc_to_git.insert(ovc_id, git_sha1.to_owned());
    }

    /// Looks up the OVC `ObjectId` for a given git SHA1 hex string.
    #[must_use]
    pub fn get_ovc(&self, git_sha1: &str) -> Option<&ObjectId> {
        self.git_to_ovc.get(git_sha1)
    }

    /// Looks up the git SHA1 hex string for a given OVC `ObjectId`.
    #[must_use]
    pub fn get_git(&self, ovc_id: &ObjectId) -> Option<&str> {
        self.ovc_to_git.get(ovc_id).map(String::as_str)
    }

    /// Returns the number of mappings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.git_to_ovc.len()
    }

    /// Returns `true` if the map contains no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.git_to_ovc.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovc_core::id;

    #[test]
    fn insert_and_lookup() {
        let mut map = OidMap::new();
        let ovc_id = id::hash_blob(b"hello");
        let git_sha1 = "ce013625030ba8dba906f756967f9e9ca394464a";

        map.insert(git_sha1, ovc_id);
        assert_eq!(map.get_ovc(git_sha1), Some(&ovc_id));
        assert_eq!(map.get_git(&ovc_id), Some(git_sha1));
    }

    #[test]
    fn missing_lookup() {
        let map = OidMap::new();
        assert!(map.get_ovc("abc").is_none());
        assert!(map.get_git(&ObjectId::ZERO).is_none());
    }

    #[test]
    fn serde_roundtrip() {
        let mut map = OidMap::new();
        let ovc_id = id::hash_blob(b"test");
        map.insert("abcd1234abcd1234abcd1234abcd1234abcd1234", ovc_id);

        let json = serde_json::to_string(&map).unwrap();
        let back: OidMap = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(
            back.get_ovc("abcd1234abcd1234abcd1234abcd1234abcd1234"),
            Some(&ovc_id)
        );
    }
}
