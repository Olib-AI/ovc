//! Revert: create a new commit that undoes the changes introduced by a given commit.
//!
//! Computes the diff introduced by a commit (relative to its first parent)
//! and applies the reverse of that diff onto the current HEAD via three-way merge.

use crate::error::{CoreError, CoreResult};
use crate::id::ObjectId;
use crate::merge;
use crate::object::{Commit, Identity, Object, Tree};
use crate::store::ObjectStore;

/// Reverts the changes introduced by `commit_oid` on top of `head_oid`.
///
/// The three-way merge uses the commit's tree as the base, the `head_oid`
/// tree as "ours", and the commit's first parent tree as "theirs". This
/// effectively reverses the commit's changes.
///
/// Returns the new commit's `ObjectId`, or an error if the reverse merge
/// produces conflicts.
pub fn revert(
    commit_oid: ObjectId,
    head_oid: ObjectId,
    store: &mut ObjectStore,
    committer: &Identity,
) -> CoreResult<ObjectId> {
    // Load the commit to revert.
    let revert_obj = store
        .get(&commit_oid)?
        .ok_or(CoreError::ObjectNotFound(commit_oid))?;
    let Object::Commit(revert_commit) = revert_obj else {
        return Err(CoreError::CorruptObject {
            reason: format!("expected commit at {commit_oid}"),
        });
    };

    // Determine the parent tree (first parent, or empty tree for root commits).
    let parent_tree = if let Some(parent_oid) = revert_commit.parents.first() {
        let parent_obj = store
            .get(parent_oid)?
            .ok_or(CoreError::ObjectNotFound(*parent_oid))?;
        let Object::Commit(parent_commit) = parent_obj else {
            return Err(CoreError::CorruptObject {
                reason: format!("expected commit at {parent_oid}"),
            });
        };
        parent_commit.tree
    } else {
        let empty = Object::Tree(Tree {
            entries: Vec::new(),
        });
        store.insert(&empty)?
    };

    // Load the head commit's tree.
    let head_obj = store
        .get(&head_oid)?
        .ok_or(CoreError::ObjectNotFound(head_oid))?;
    let Object::Commit(head_commit) = head_obj else {
        return Err(CoreError::CorruptObject {
            reason: format!("expected commit at {head_oid}"),
        });
    };

    // Three-way merge to reverse the commit:
    // base = commit's tree (what was introduced)
    // ours = HEAD's tree (current state)
    // theirs = parent's tree (state before the commit)
    // This undoes the commit's changes on top of HEAD.
    let merge_result =
        merge::merge_trees(&revert_commit.tree, &head_commit.tree, &parent_tree, store)?;

    if !merge_result.conflicts.is_empty() {
        let paths: Vec<String> = merge_result
            .conflicts
            .iter()
            .map(|c| c.path.clone())
            .collect();
        return Err(CoreError::FormatError {
            reason: format!("revert conflicts in: {}", paths.join(", ")),
        });
    }

    // Build the merged tree.
    let merged_tree = Object::Tree(Tree {
        entries: merge_result.entries,
    });
    let merged_tree_oid = store.insert(&merged_tree)?;

    // Create a new commit with a revert message.
    let new_commit = Commit {
        tree: merged_tree_oid,
        parents: vec![head_oid],
        author: committer.clone(),
        committer: committer.clone(),
        message: format!("Revert: {}", revert_commit.message),
        signature: None,
        sequence: head_commit.sequence + 1,
    };

    store.insert(&Object::Commit(new_commit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{FileMode, TreeEntry};

    fn test_identity() -> Identity {
        Identity {
            name: "Test".into(),
            email: "test@test.com".into(),
            timestamp: 1_700_000_000,
            tz_offset_minutes: 0,
        }
    }

    fn make_tree_with_file(store: &mut ObjectStore, name: &str, content: &[u8]) -> ObjectId {
        let blob_oid = store.insert(&Object::Blob(content.to_vec())).unwrap();
        let tree = Object::Tree(Tree {
            entries: vec![TreeEntry {
                mode: FileMode::Regular,
                name: name.as_bytes().to_vec(),
                oid: blob_oid,
            }],
        });
        store.insert(&tree).unwrap()
    }

    fn make_commit(
        store: &mut ObjectStore,
        tree_oid: ObjectId,
        parents: Vec<ObjectId>,
        msg: &str,
        seq: u64,
    ) -> ObjectId {
        let commit = Commit {
            tree: tree_oid,
            parents,
            author: test_identity(),
            committer: test_identity(),
            message: msg.to_owned(),
            signature: None,
            sequence: seq,
        };
        store.insert(&Object::Commit(commit)).unwrap()
    }

    #[test]
    fn revert_single_commit() {
        let mut store = ObjectStore::default();
        let identity = test_identity();

        // Initial commit: readme.txt = "hello\n"
        let base_tree = make_tree_with_file(&mut store, "readme.txt", b"hello\n");
        let base = make_commit(&mut store, base_tree, vec![], "initial", 1);

        // Second commit: readme.txt = "hello world\n"
        let changed_tree = make_tree_with_file(&mut store, "readme.txt", b"hello world\n");
        let changed = make_commit(&mut store, changed_tree, vec![base], "update readme", 2);

        // Revert the second commit (should restore "hello\n").
        let result = revert(changed, changed, &mut store, &identity).unwrap();

        let obj = store.get(&result).unwrap().unwrap();
        let Object::Commit(new_commit) = obj else {
            panic!("expected commit");
        };

        assert!(new_commit.message.starts_with("Revert: "));
        assert_eq!(new_commit.parents, vec![changed]);

        // The tree should match the base tree's content.
        let tree_obj = store.get(&new_commit.tree).unwrap().unwrap();
        let Object::Tree(tree) = tree_obj else {
            panic!("expected tree");
        };

        let readme_entry = tree
            .entries
            .iter()
            .find(|e| e.name == b"readme.txt")
            .expect("readme.txt should exist");

        let blob = store.get(&readme_entry.oid).unwrap().unwrap();
        let Object::Blob(data) = blob else {
            panic!("expected blob");
        };
        assert_eq!(data, b"hello\n");
    }
}
