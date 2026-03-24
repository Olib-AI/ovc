//! Stash store for saving and restoring index state.
//!
//! [`StashStore`] maintains a stack of [`StashEntry`] values, each representing
//! a snapshot of the staging index at a given point in time. Entries can be
//! pushed, popped, applied (without removal), or dropped.

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::id::ObjectId;
use crate::index::Index;
use crate::object::{Commit, Identity, Object};
use crate::store::ObjectStore;

/// A single stash entry recording the index and HEAD state at stash time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StashEntry {
    /// Human-readable description of this stash.
    pub message: String,
    /// The commit object id representing the stashed working state tree.
    pub commit_id: ObjectId,
    /// The commit object id representing the index state at stash time.
    pub index_commit_id: ObjectId,
    /// The HEAD commit at the time the stash was created.
    pub base_commit_id: ObjectId,
    /// Unix timestamp when the stash was created.
    pub timestamp: i64,
}

/// A stack-based store for stash entries, persisted in the superblock.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StashStore {
    entries: Vec<StashEntry>,
}

impl StashStore {
    /// Creates a new empty stash store.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Saves the current index state as a new stash entry.
    ///
    /// Creates a tree from the index, wraps it in a commit object, and pushes
    /// the entry onto the stash stack. Returns the zero-based index of the
    /// newly created entry.
    pub fn push(
        &mut self,
        message: &str,
        store: &mut ObjectStore,
        index: &Index,
        head_oid: ObjectId,
        author: &Identity,
    ) -> CoreResult<usize> {
        let tree_oid = index.write_tree(store)?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));

        let stash_commit = Commit {
            tree: tree_oid,
            parents: vec![head_oid],
            author: author.clone(),
            committer: author.clone(),
            message: format!("stash: {message}"),
            signature: None,
            sequence: 0,
        };
        let commit_id = store.insert(&Object::Commit(stash_commit))?;

        // The index commit records the same tree but is kept as a separate
        // object so that pop/apply can distinguish the two if needed in the
        // future (e.g., when staged vs unstaged separation is implemented).
        let index_commit = Commit {
            tree: tree_oid,
            parents: vec![head_oid],
            author: author.clone(),
            committer: author.clone(),
            message: format!("index on stash: {message}"),
            signature: None,
            sequence: 0,
        };
        let index_commit_id = store.insert(&Object::Commit(index_commit))?;

        let entry = StashEntry {
            message: message.to_owned(),
            commit_id,
            index_commit_id,
            base_commit_id: head_oid,
            timestamp: now,
        };

        // Push to front (index 0 = most recent).
        self.entries.insert(0, entry);
        Ok(0)
    }

    /// Restores a stash entry to the index and removes it from the stack.
    ///
    /// The entry at position `idx` is applied to the provided index, then
    /// removed from the stash.
    pub fn pop(
        &mut self,
        idx: usize,
        store: &ObjectStore,
        index: &mut Index,
    ) -> CoreResult<StashEntry> {
        if idx >= self.entries.len() {
            return Err(CoreError::FormatError {
                reason: format!(
                    "stash index {idx} out of range (have {} entries)",
                    self.entries.len()
                ),
            });
        }

        self.apply(idx, store, index)?;
        Ok(self.entries.remove(idx))
    }

    /// Restores a stash entry to the index without removing it from the stack.
    pub fn apply(&self, idx: usize, store: &ObjectStore, index: &mut Index) -> CoreResult<()> {
        let entry = self
            .entries
            .get(idx)
            .ok_or_else(|| CoreError::FormatError {
                reason: format!(
                    "stash index {idx} out of range (have {} entries)",
                    self.entries.len()
                ),
            })?;

        let commit = store
            .get(&entry.commit_id)?
            .ok_or(CoreError::ObjectNotFound(entry.commit_id))?;

        let Object::Commit(commit) = commit else {
            return Err(CoreError::CorruptObject {
                reason: format!("stash commit {} is not a commit object", entry.commit_id),
            });
        };

        index.read_tree(&commit.tree, store)?;
        Ok(())
    }

    /// Removes a stash entry without restoring it.
    pub fn drop_entry(&mut self, idx: usize) -> CoreResult<StashEntry> {
        if idx >= self.entries.len() {
            return Err(CoreError::FormatError {
                reason: format!(
                    "stash index {idx} out of range (have {} entries)",
                    self.entries.len()
                ),
            });
        }
        Ok(self.entries.remove(idx))
    }

    /// Returns a slice of all stash entries (index 0 = most recent).
    #[must_use]
    pub fn list(&self) -> &[StashEntry] {
        &self.entries
    }

    /// Returns a mutable reference to the entries vector.
    ///
    /// Primarily used for serialization and testing; prefer the structured
    /// push/pop/apply/drop methods for normal usage.
    pub const fn entries_mut(&mut self) -> &mut Vec<StashEntry> {
        &mut self.entries
    }

    /// Removes all stash entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for StashStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::FileMode;

    fn test_identity() -> Identity {
        Identity {
            name: "Test".into(),
            email: "test@test.com".into(),
            timestamp: 1_700_000_000,
            tz_offset_minutes: 0,
        }
    }

    fn make_head_commit(store: &mut ObjectStore) -> ObjectId {
        let tree = Object::Tree(crate::object::Tree {
            entries: Vec::new(),
        });
        let tree_oid = store.insert(&tree).unwrap();
        let commit = Commit {
            tree: tree_oid,
            parents: Vec::new(),
            author: test_identity(),
            committer: test_identity(),
            message: "initial".into(),
            signature: None,
            sequence: 1,
        };
        store.insert(&Object::Commit(commit)).unwrap()
    }

    #[test]
    fn push_pop_round_trip() {
        let mut store = ObjectStore::default();
        let mut index = Index::new();
        let head = make_head_commit(&mut store);

        index
            .stage_file("a.txt", b"hello", FileMode::Regular, &mut store)
            .unwrap();

        let mut stash = StashStore::new();
        let idx = stash
            .push("wip", &mut store, &index, head, &test_identity())
            .unwrap();
        assert_eq!(idx, 0);
        assert_eq!(stash.list().len(), 1);

        // Clear index, then pop.
        index.clear();
        assert!(index.is_empty());

        let entry = stash.pop(0, &store, &mut index).unwrap();
        assert_eq!(entry.message, "wip");
        assert!(!index.is_empty());
        assert!(stash.list().is_empty());
    }

    #[test]
    fn apply_does_not_remove() {
        let mut store = ObjectStore::default();
        let mut index = Index::new();
        let head = make_head_commit(&mut store);

        index
            .stage_file("b.txt", b"data", FileMode::Regular, &mut store)
            .unwrap();

        let mut stash = StashStore::new();
        stash
            .push("test", &mut store, &index, head, &test_identity())
            .unwrap();

        index.clear();
        stash.apply(0, &store, &mut index).unwrap();
        assert!(!index.is_empty());
        assert_eq!(stash.list().len(), 1);
    }

    #[test]
    fn drop_removes_without_apply() {
        let mut store = ObjectStore::default();
        let index = Index::new();
        let head = make_head_commit(&mut store);

        let mut stash = StashStore::new();
        stash
            .push("drop me", &mut store, &index, head, &test_identity())
            .unwrap();

        let entry = stash.drop_entry(0).unwrap();
        assert_eq!(entry.message, "drop me");
        assert!(stash.list().is_empty());
    }

    #[test]
    fn clear_removes_all() {
        let mut store = ObjectStore::default();
        let index = Index::new();
        let head = make_head_commit(&mut store);

        let mut stash = StashStore::new();
        stash
            .push("a", &mut store, &index, head, &test_identity())
            .unwrap();
        stash
            .push("b", &mut store, &index, head, &test_identity())
            .unwrap();
        assert_eq!(stash.list().len(), 2);

        stash.clear();
        assert!(stash.list().is_empty());
    }

    #[test]
    fn list_ordering() {
        let mut store = ObjectStore::default();
        let index = Index::new();
        let head = make_head_commit(&mut store);

        let mut stash = StashStore::new();
        stash
            .push("first", &mut store, &index, head, &test_identity())
            .unwrap();
        stash
            .push("second", &mut store, &index, head, &test_identity())
            .unwrap();

        let list = stash.list();
        assert_eq!(list[0].message, "second");
        assert_eq!(list[1].message, "first");
    }

    #[test]
    fn out_of_range_errors() {
        let mut stash = StashStore::new();
        let store = ObjectStore::default();
        let mut index = Index::new();

        assert!(stash.pop(0, &store, &mut index).is_err());
        assert!(stash.apply(0, &store, &mut index).is_err());
        assert!(stash.drop_entry(0).is_err());
    }
}
