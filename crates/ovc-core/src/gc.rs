//! Garbage collection for unreachable objects.
//!
//! [`garbage_collect`] performs a mark-and-sweep pass over the object store,
//! removing any objects not reachable from known references (branches, tags,
//! HEAD, and stash entries).

use std::collections::{HashSet, VecDeque};

use crate::error::CoreResult;
use crate::id::ObjectId;
use crate::object::Object;
use crate::refs::RefStore;
use crate::stash::StashStore;
use crate::store::ObjectStore;

/// Statistics from a garbage collection run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcResult {
    /// Number of objects before GC.
    pub objects_before: usize,
    /// Number of objects after GC.
    pub objects_after: usize,
    /// Total compressed bytes before GC.
    pub bytes_before: u64,
    /// Total compressed bytes after GC.
    pub bytes_after: u64,
}

/// Performs garbage collection on the object store.
///
/// Marks all objects reachable from branches, tags, HEAD, and stash entries,
/// then removes any objects that are not reachable.
pub fn garbage_collect(
    store: &mut ObjectStore,
    ref_store: &RefStore,
    stash: &StashStore,
) -> CoreResult<GcResult> {
    let objects_before = store.count();
    let bytes_before = store.total_compressed_bytes();

    // Collect all root object ids from references.
    let mut roots = Vec::new();

    // HEAD.
    if let Ok(head_oid) = ref_store.resolve_head() {
        roots.push(head_oid);
    }

    // All branches.
    for (_name, oid) in ref_store.list_branches() {
        roots.push(*oid);
    }

    // All tags.
    for (_name, oid, _msg) in ref_store.list_tags() {
        roots.push(*oid);
    }

    // Stash entries.
    for entry in stash.list() {
        roots.push(entry.commit_id);
        roots.push(entry.index_commit_id);
        roots.push(entry.base_commit_id);
    }

    // Reflog entries — protect objects referenced by recent history so that
    // force-pushed or amended commits remain recoverable via `ovc reflog`.
    for (_name, entries) in ref_store.all_reflog_entries() {
        for entry in entries {
            roots.push(entry.new_value);
            if let Some(old) = entry.old_value {
                roots.push(old);
            }
        }
    }

    // Mark phase: BFS from all roots.
    let reachable = mark_reachable(&roots, store)?;

    // Sweep phase: remove unreachable objects.
    store.retain(|oid, _| reachable.contains(oid));

    let objects_after = store.count();
    let bytes_after = store.total_compressed_bytes();

    Ok(GcResult {
        objects_before,
        objects_after,
        bytes_before,
        bytes_after,
    })
}

/// Performs a BFS from the given root object ids, following commit->tree->blob
/// references to build the complete set of reachable object ids.
fn mark_reachable(roots: &[ObjectId], store: &ObjectStore) -> CoreResult<HashSet<ObjectId>> {
    let mut reachable = HashSet::new();
    let mut queue = VecDeque::new();

    for root in roots {
        if !root.is_zero() && reachable.insert(*root) {
            queue.push_back(*root);
        }
    }

    while let Some(oid) = queue.pop_front() {
        let Some(obj) = store.get(&oid)? else {
            continue;
        };

        match obj {
            Object::Commit(commit) => {
                // Tree.
                if reachable.insert(commit.tree) {
                    queue.push_back(commit.tree);
                }
                // Parents.
                for parent in &commit.parents {
                    if reachable.insert(*parent) {
                        queue.push_back(*parent);
                    }
                }
            }
            Object::Tree(tree) => {
                for entry in &tree.entries {
                    if reachable.insert(entry.oid) {
                        queue.push_back(entry.oid);
                    }
                }
            }
            Object::Tag(tag) => {
                if reachable.insert(tag.target) {
                    queue.push_back(tag.target);
                }
            }
            Object::Blob(_) => {
                // Blobs are leaf nodes; nothing further to traverse.
            }
        }
    }

    Ok(reachable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Commit, FileMode, Identity, Tree, TreeEntry};

    fn test_identity() -> Identity {
        Identity {
            name: "Test".into(),
            email: "test@test.com".into(),
            timestamp: 1_700_000_000,
            tz_offset_minutes: 0,
        }
    }

    #[test]
    fn gc_removes_orphaned_objects() {
        let mut store = ObjectStore::default();
        let ref_store = RefStore::default();
        let stash = StashStore::new();

        // Create an orphaned blob (not referenced by any tree or commit).
        let orphan = Object::Blob(b"orphaned data".to_vec());
        let orphan_oid = store.insert(&orphan).unwrap();

        // Create a referenced blob + tree + commit.
        let blob = Object::Blob(b"referenced data".to_vec());
        let blob_oid = store.insert(&blob).unwrap();
        let tree = Object::Tree(Tree {
            entries: vec![TreeEntry {
                mode: FileMode::Regular,
                name: b"file.txt".to_vec(),
                oid: blob_oid,
            }],
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
        let commit_oid = store.insert(&Object::Commit(commit)).unwrap();

        // Set up refs so the commit is reachable.
        let mut ref_store = ref_store;
        ref_store
            .set_branch("main", commit_oid, &test_identity(), "init")
            .unwrap();

        assert_eq!(store.count(), 4); // orphan + blob + tree + commit

        let result = garbage_collect(&mut store, &ref_store, &stash).unwrap();

        assert_eq!(result.objects_before, 4);
        assert_eq!(result.objects_after, 3); // orphan removed
        assert!(!store.contains(&orphan_oid));
        assert!(store.contains(&blob_oid));
        assert!(store.contains(&tree_oid));
        assert!(store.contains(&commit_oid));
    }

    #[test]
    fn gc_preserves_stash_objects() {
        let mut store = ObjectStore::default();
        let ref_store = RefStore::default();

        // Create a commit referenced only by stash.
        let tree = Object::Tree(Tree {
            entries: Vec::new(),
        });
        let tree_oid = store.insert(&tree).unwrap();
        let commit = Commit {
            tree: tree_oid,
            parents: Vec::new(),
            author: test_identity(),
            committer: test_identity(),
            message: "stash".into(),
            signature: None,
            sequence: 0,
        };
        let commit_oid = store.insert(&Object::Commit(commit)).unwrap();

        let mut stash = StashStore::new();
        stash.entries_mut().push(crate::stash::StashEntry {
            message: "test stash".into(),
            commit_id: commit_oid,
            index_commit_id: commit_oid,
            base_commit_id: commit_oid,
            timestamp: 0,
        });

        let result = garbage_collect(&mut store, &ref_store, &stash).unwrap();
        assert_eq!(result.objects_after, 2); // tree + commit preserved
    }

    #[test]
    fn gc_empty_store() {
        let mut store = ObjectStore::default();
        let ref_store = RefStore::default();
        let stash = StashStore::new();

        let result = garbage_collect(&mut store, &ref_store, &stash).unwrap();
        assert_eq!(result.objects_before, 0);
        assert_eq!(result.objects_after, 0);
    }
}
