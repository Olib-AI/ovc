//! Cherry-pick: apply a single commit's changes onto another commit.
//!
//! Computes the diff introduced by a commit (relative to its first parent)
//! and replays that diff onto the current HEAD via three-way merge.

use crate::error::{CoreError, CoreResult};
use crate::id::ObjectId;
use crate::merge;
use crate::object::{Commit, Identity, Object, Tree};
use crate::store::ObjectStore;

/// Applies the changes introduced by `commit_oid` onto `head_oid`.
///
/// The base for the three-way merge is the commit's first parent tree.
/// The "ours" side is the `head_oid` tree and the "theirs" side is the
/// commit's tree. A new commit is created with the original author
/// preserved and the provided `committer` identity.
///
/// Returns the new commit's `ObjectId`, or an error if the merge produces
/// conflicts.
pub fn cherry_pick(
    commit_oid: ObjectId,
    head_oid: ObjectId,
    store: &mut ObjectStore,
    committer: &Identity,
) -> CoreResult<ObjectId> {
    // Load the commit to cherry-pick.
    let pick_obj = store
        .get(&commit_oid)?
        .ok_or(CoreError::ObjectNotFound(commit_oid))?;
    let Object::Commit(pick_commit) = pick_obj else {
        return Err(CoreError::CorruptObject {
            reason: format!("expected commit at {commit_oid}"),
        });
    };

    // Determine the base tree (first parent's tree, or empty tree for root commits).
    let base_tree = if let Some(parent_oid) = pick_commit.parents.first() {
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

    // Three-way merge: base=parent_tree, ours=head_tree, theirs=pick_tree.
    let merge_result = merge::merge_trees(&base_tree, &head_commit.tree, &pick_commit.tree, store)?;

    if !merge_result.conflicts.is_empty() {
        let paths: Vec<String> = merge_result
            .conflicts
            .iter()
            .map(|c| c.path.clone())
            .collect();
        return Err(CoreError::FormatError {
            reason: format!("cherry-pick conflicts in: {}", paths.join(", ")),
        });
    }

    // Build the merged tree.
    let merged_tree = Object::Tree(Tree {
        entries: merge_result.entries,
    });
    let merged_tree_oid = store.insert(&merged_tree)?;

    // Create a new commit preserving the original author.
    let new_commit = Commit {
        tree: merged_tree_oid,
        parents: vec![head_oid],
        author: pick_commit.author.clone(),
        committer: committer.clone(),
        message: pick_commit.message.clone(),
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
    fn cherry_pick_single_commit() {
        let mut store = ObjectStore::default();
        let identity = test_identity();

        // Base: one file.
        let base_tree = make_tree_with_file(&mut store, "readme.txt", b"hello\n");
        let base = make_commit(&mut store, base_tree, vec![], "initial", 1);

        // Branch A: add a new file.
        let a_blob1 = store.insert(&Object::Blob(b"hello\n".to_vec())).unwrap();
        let a_blob2 = store.insert(&Object::Blob(b"new file\n".to_vec())).unwrap();
        let a_tree = Object::Tree(Tree {
            entries: vec![
                TreeEntry {
                    mode: FileMode::Regular,
                    name: b"new.txt".to_vec(),
                    oid: a_blob2,
                },
                TreeEntry {
                    mode: FileMode::Regular,
                    name: b"readme.txt".to_vec(),
                    oid: a_blob1,
                },
            ],
        });
        let a_tree_oid = store.insert(&a_tree).unwrap();
        let a_commit = make_commit(&mut store, a_tree_oid, vec![base], "add new.txt", 2);

        // Branch B (HEAD): modify existing file.
        let b_tree = make_tree_with_file(&mut store, "readme.txt", b"hello world\n");
        let b_commit = make_commit(&mut store, b_tree, vec![base], "update readme", 2);

        // Cherry-pick a_commit onto b_commit.
        let result = cherry_pick(a_commit, b_commit, &mut store, &identity).unwrap();

        // Verify the new commit exists and has both changes.
        let obj = store.get(&result).unwrap().unwrap();
        let Object::Commit(new_commit) = obj else {
            panic!("expected commit");
        };

        let tree_obj = store.get(&new_commit.tree).unwrap().unwrap();
        let Object::Tree(tree) = tree_obj else {
            panic!("expected tree");
        };

        let names: Vec<String> = tree
            .entries
            .iter()
            .map(|e| String::from_utf8_lossy(&e.name).into_owned())
            .collect();
        assert!(names.contains(&"readme.txt".to_owned()));
        assert!(names.contains(&"new.txt".to_owned()));

        // Verify the commit message is preserved from the cherry-picked commit.
        assert_eq!(new_commit.message, "add new.txt");
        assert_eq!(new_commit.parents, vec![b_commit]);
    }
}
