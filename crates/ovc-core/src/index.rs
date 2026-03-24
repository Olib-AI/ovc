//! Staging area (index) for tracking files to be committed.
//!
//! The [`Index`] maintains a sorted list of [`IndexEntry`] values representing
//! files staged for the next commit. It supports building tree objects from
//! the staged entries and populating the index from an existing tree.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::id::ObjectId;
use crate::object::{FileMode, Object, Tree, TreeEntry};
use crate::store::ObjectStore;

/// Flags on an index entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntryFlags {
    /// Whether this entry is assumed unchanged (skip stat check).
    pub assume_unchanged: bool,
    /// Whether this entry has an intent-to-add marker.
    pub intent_to_add: bool,
}

/// A single entry in the staging index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    /// The file path relative to the repository root (forward-slash separated).
    pub path: String,
    /// The object id of the blob.
    pub oid: ObjectId,
    /// The file mode.
    pub mode: FileMode,
    /// File size in bytes.
    pub file_size: u64,
    /// Last modification time (seconds since epoch).
    pub mtime_secs: i64,
    /// Last modification time (nanosecond component).
    pub mtime_nanos: u32,
    /// Merge stage: 0=normal, 1=base, 2=ours, 3=theirs.
    pub stage: u8,
    /// Additional flags.
    pub flags: IndexEntryFlags,
}

/// The staging area, mapping file paths to index entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    /// Entries sorted by path.
    entries: Vec<IndexEntry>,
}

impl Index {
    /// Creates a new empty index.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Stages a file by hashing its content, storing the blob, and adding an entry.
    ///
    /// Returns the `ObjectId` of the stored blob.
    pub fn stage_file(
        &mut self,
        path: &str,
        content: &[u8],
        mode: FileMode,
        store: &mut ObjectStore,
    ) -> CoreResult<ObjectId> {
        let blob = Object::Blob(content.to_vec());
        let oid = store.insert(&blob)?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();

        let entry = IndexEntry {
            path: path.to_owned(),
            oid,
            mode,
            file_size: u64::try_from(content.len()).unwrap_or(u64::MAX),
            mtime_secs: i64::try_from(now.as_secs()).unwrap_or(i64::MAX),
            mtime_nanos: now.subsec_nanos(),
            stage: 0,
            flags: IndexEntryFlags::default(),
        };

        // Replace existing entry at same path or insert in sorted order.
        if let Some(pos) = self.entries.iter().position(|e| e.path == path) {
            self.entries[pos] = entry;
        } else {
            let pos = self
                .entries
                .binary_search_by(|e| e.path.as_str().cmp(path))
                .unwrap_or_else(|p| p);
            self.entries.insert(pos, entry);
        }

        Ok(oid)
    }

    /// Removes a file from the staging area.
    pub fn unstage_file(&mut self, path: &str) {
        self.entries.retain(|e| e.path != path);
    }

    /// Looks up an entry by path.
    #[must_use]
    pub fn get_entry(&self, path: &str) -> Option<&IndexEntry> {
        self.entries.iter().find(|e| e.path == path)
    }

    /// Returns all entries sorted by path.
    #[must_use]
    pub fn entries(&self) -> &[IndexEntry] {
        &self.entries
    }

    /// Builds tree objects from the current index and stores them.
    ///
    /// Returns the root tree `ObjectId`. Directory structure is inferred
    /// from forward-slash separators in entry paths.
    pub fn write_tree(&self, store: &mut ObjectStore) -> CoreResult<ObjectId> {
        // Build a nested map: directory path -> Vec<(name, oid, mode, is_dir)>
        let mut dir_entries: BTreeMap<String, Vec<(String, ObjectId, FileMode)>> = BTreeMap::new();

        // Collect all unique directory paths and their file children.
        for entry in &self.entries {
            let (dir, file_name) = match entry.path.rsplit_once('/') {
                Some((d, f)) => (d.to_owned(), f.to_owned()),
                None => (String::new(), entry.path.clone()),
            };

            dir_entries
                .entry(dir.clone())
                .or_default()
                .push((file_name, entry.oid, entry.mode));

            // Ensure all ancestor directories exist in the map, even if they
            // contain no direct file children (only subdirectories).
            let mut ancestor = dir.as_str();
            while let Some((parent, _)) = ancestor.rsplit_once('/') {
                // If the parent already exists we can stop — its ancestors
                // will have been registered when the parent was first seen.
                if dir_entries.contains_key(parent) {
                    break;
                }
                dir_entries.entry(parent.to_owned()).or_default();
                ancestor = parent;
            }
            // Ensure the root directory exists.
            dir_entries.entry(String::new()).or_default();
        }

        // If the index is empty, create an empty tree.
        if dir_entries.is_empty() {
            let empty_tree = Object::Tree(Tree {
                entries: Vec::new(),
            });
            return store.insert(&empty_tree);
        }

        // Process directories bottom-up: sort by depth descending so deeper dirs
        // are processed first and their tree OIDs can be used by parent dirs.
        let mut dir_keys: Vec<String> = dir_entries.keys().cloned().collect();
        dir_keys.sort_by(|a, b| {
            let depth_a = if a.is_empty() {
                0
            } else {
                a.matches('/').count() + 1
            };
            let depth_b = if b.is_empty() {
                0
            } else {
                b.matches('/').count() + 1
            };
            depth_b.cmp(&depth_a).then_with(|| a.cmp(b))
        });

        let mut dir_oids: BTreeMap<String, ObjectId> = BTreeMap::new();

        for dir_path in &dir_keys {
            let file_children = dir_entries.get(dir_path).cloned().unwrap_or_default();

            let mut tree_entries: Vec<TreeEntry> = file_children
                .into_iter()
                .map(|(name, oid, mode)| TreeEntry {
                    mode,
                    name: name.into_bytes(),
                    oid,
                })
                .collect();

            // Add subdirectory entries (children of this directory that are themselves directories).
            for (sub_dir, sub_oid) in &dir_oids {
                let parent = match sub_dir.rsplit_once('/') {
                    Some((p, _)) => p.to_owned(),
                    None => String::new(),
                };
                if parent == *dir_path {
                    let sub_name = match sub_dir.rsplit_once('/') {
                        Some((_, n)) => n.to_owned(),
                        None => sub_dir.clone(),
                    };
                    tree_entries.push(TreeEntry {
                        mode: FileMode::Directory,
                        name: sub_name.into_bytes(),
                        oid: *sub_oid,
                    });
                }
            }

            tree_entries.sort_by(|a, b| a.name.cmp(&b.name));

            let tree = Object::Tree(Tree {
                entries: tree_entries,
            });
            let oid = store.insert(&tree)?;
            dir_oids.insert(dir_path.clone(), oid);
        }

        // The root tree is at the empty-string key.
        dir_oids
            .get("")
            .copied()
            .ok_or_else(|| CoreError::FormatError {
                reason: "failed to build root tree from index".into(),
            })
    }

    /// Populates this index from a tree object, recursively walking subdirectories.
    pub fn read_tree(&mut self, oid: &ObjectId, store: &ObjectStore) -> CoreResult<()> {
        self.entries.clear();
        self.read_tree_recursive(oid, "", store)?;
        self.entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(())
    }

    /// Restores an index entry to match the blob at `path` in the given HEAD tree.
    ///
    /// If the file exists in the HEAD tree, the index entry is replaced with the
    /// HEAD version. If the file does not exist in the HEAD tree (i.e. it was
    /// newly added), the entry is removed from the index entirely.
    pub fn restore_to_head(
        &mut self,
        path: &str,
        head_tree: &ObjectId,
        store: &ObjectStore,
    ) -> CoreResult<()> {
        // Build a temporary index from the HEAD tree to find the entry.
        let mut head_index = Self::new();
        head_index.read_tree(head_tree, store)?;

        if let Some(head_entry) = head_index.get_entry(path) {
            // File exists in HEAD: restore the index entry to match.
            let restored = head_entry.clone();
            if let Some(pos) = self.entries.iter().position(|e| e.path == path) {
                self.entries[pos] = restored;
            } else {
                let pos = self
                    .entries
                    .binary_search_by(|e| e.path.as_str().cmp(path))
                    .unwrap_or_else(|p| p);
                self.entries.insert(pos, restored);
            }
        } else {
            // File does not exist in HEAD: remove from index (correct for "added" files).
            self.entries.retain(|e| e.path != path);
        }

        Ok(())
    }

    /// Clears all entries from the index.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Returns true if the index has no entries.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    // ── Internal helpers ──────────────────────────────────────────────

    /// Recursively walks a tree, adding blob entries to the index.
    fn read_tree_recursive(
        &mut self,
        oid: &ObjectId,
        prefix: &str,
        store: &ObjectStore,
    ) -> CoreResult<()> {
        let obj = store.get(oid)?.ok_or(CoreError::ObjectNotFound(*oid))?;

        let Object::Tree(tree) = obj else {
            return Err(CoreError::CorruptObject {
                reason: format!("expected tree object at {oid}"),
            });
        };

        for entry in &tree.entries {
            let name = String::from_utf8_lossy(&entry.name).into_owned();
            let full_path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };

            if entry.mode == FileMode::Directory {
                self.read_tree_recursive(&entry.oid, &full_path, store)?;
            } else {
                // Get blob size if possible.
                let file_size = store.get(&entry.oid)?.map_or(0, |obj| match obj {
                    Object::Blob(data) => u64::try_from(data.len()).unwrap_or(0),
                    _ => 0,
                });

                self.entries.push(IndexEntry {
                    path: full_path,
                    oid: entry.oid,
                    mode: entry.mode,
                    file_size,
                    mtime_secs: 0,
                    mtime_nanos: 0,
                    stage: 0,
                    flags: IndexEntryFlags::default(),
                });
            }
        }

        Ok(())
    }
}

impl Default for Index {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_and_get_entry() {
        let mut store = ObjectStore::default();
        let mut index = Index::new();

        let oid = index
            .stage_file("hello.txt", b"hello world", FileMode::Regular, &mut store)
            .unwrap();

        assert!(!oid.is_zero());
        let entry = index.get_entry("hello.txt").unwrap();
        assert_eq!(entry.oid, oid);
        assert_eq!(entry.mode, FileMode::Regular);
    }

    #[test]
    fn unstage_file() {
        let mut store = ObjectStore::default();
        let mut index = Index::new();

        index
            .stage_file("a.txt", b"a", FileMode::Regular, &mut store)
            .unwrap();
        assert!(!index.is_empty());

        index.unstage_file("a.txt");
        assert!(index.is_empty());
    }

    #[test]
    fn entries_sorted_by_path() {
        let mut store = ObjectStore::default();
        let mut index = Index::new();

        index
            .stage_file("c.txt", b"c", FileMode::Regular, &mut store)
            .unwrap();
        index
            .stage_file("a.txt", b"a", FileMode::Regular, &mut store)
            .unwrap();
        index
            .stage_file("b.txt", b"b", FileMode::Regular, &mut store)
            .unwrap();

        let paths: Vec<&str> = index.entries().iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, &["a.txt", "b.txt", "c.txt"]);
    }

    #[test]
    fn write_tree_and_read_tree_round_trip() {
        let mut store = ObjectStore::default();
        let mut index = Index::new();

        index
            .stage_file("README.md", b"# Hello", FileMode::Regular, &mut store)
            .unwrap();
        index
            .stage_file(
                "src/main.rs",
                b"fn main() {}",
                FileMode::Regular,
                &mut store,
            )
            .unwrap();
        index
            .stage_file("src/lib.rs", b"pub mod foo;", FileMode::Regular, &mut store)
            .unwrap();

        let tree_oid = index.write_tree(&mut store).unwrap();

        let mut index2 = Index::new();
        index2.read_tree(&tree_oid, &store).unwrap();

        assert_eq!(index2.entries().len(), 3);
        let paths: Vec<&str> = index2.entries().iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, &["README.md", "src/lib.rs", "src/main.rs"]);

        // Verify OIDs match.
        for orig_entry in index.entries() {
            let restored = index2.get_entry(&orig_entry.path).unwrap();
            assert_eq!(orig_entry.oid, restored.oid);
        }
    }

    #[test]
    fn write_tree_empty_index() {
        let mut store = ObjectStore::default();
        let index = Index::new();
        let oid = index.write_tree(&mut store).unwrap();
        assert!(!oid.is_zero());
    }

    #[test]
    fn clear_empties_index() {
        let mut store = ObjectStore::default();
        let mut index = Index::new();
        index
            .stage_file("f.txt", b"f", FileMode::Regular, &mut store)
            .unwrap();
        index.clear();
        assert!(index.is_empty());
    }

    #[test]
    fn restage_replaces_entry() {
        let mut store = ObjectStore::default();
        let mut index = Index::new();

        let oid1 = index
            .stage_file("f.txt", b"version1", FileMode::Regular, &mut store)
            .unwrap();
        let oid2 = index
            .stage_file("f.txt", b"version2", FileMode::Regular, &mut store)
            .unwrap();

        assert_ne!(oid1, oid2);
        assert_eq!(index.entries().len(), 1);
        assert_eq!(index.get_entry("f.txt").unwrap().oid, oid2);
    }
}
