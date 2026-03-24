//! Rebase operations: replay commits onto a new base.
//!
//! Provides [`rebase`] to replay a chain of commits from one branch tip onto
//! another target commit, and [`find_merge_base`] to locate the nearest common
//! ancestor of two commits via BFS.

use std::collections::{HashSet, VecDeque};

use crate::error::{CoreError, CoreResult};
use crate::id::ObjectId;
use crate::merge;
use crate::object::{Commit, Identity, Object, Tree};
use crate::store::ObjectStore;

/// Successful rebase result containing the new tip and commit mappings.
#[derive(Debug, Clone)]
pub struct RebaseResult {
    /// The new branch tip commit after all replays.
    pub new_tip: ObjectId,
    /// Mapping of (original commit, replayed commit) for each replayed commit.
    pub replayed: Vec<(ObjectId, ObjectId)>,
}

/// Error type for rebase operations that may encounter conflicts.
#[derive(Debug)]
pub enum RebaseError {
    /// A merge conflict occurred while replaying a commit.
    Conflict {
        /// The commit that caused the conflict.
        commit: ObjectId,
        /// Paths that have conflicts.
        conflicts: Vec<String>,
        /// Commits that were successfully replayed before the conflict.
        completed: Vec<(ObjectId, ObjectId)>,
    },
    /// No common ancestor could be found between the two branches.
    NoCommonAncestor,
    /// The branch being rebased contains merge commits (more than one parent).
    ///
    /// OVC's linear rebase cannot replay merge commits correctly; use
    /// `merge` instead to integrate such branches.
    MergeCommitInChain {
        /// The merge commit that stopped the rebase.
        commit: ObjectId,
    },
    /// The base commit was not found in the first-parent chain of the tip.
    ///
    /// This can happen when `branch_tip` and `base` are on unrelated branches
    /// even though a merge base was found.
    BaseNotReachable,
    /// A core library error occurred.
    Core(CoreError),
}

impl From<CoreError> for RebaseError {
    fn from(e: CoreError) -> Self {
        Self::Core(e)
    }
}

impl std::fmt::Display for RebaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict {
                commit, conflicts, ..
            } => {
                write!(
                    f,
                    "conflict while replaying {commit}: {}",
                    conflicts.join(", ")
                )
            }
            Self::NoCommonAncestor => write!(f, "no common ancestor found"),
            Self::MergeCommitInChain { commit } => write!(
                f,
                "cannot rebase: commit {commit} is a merge commit; use merge instead"
            ),
            Self::BaseNotReachable => write!(
                f,
                "cannot rebase: base commit is not reachable via first-parent chain from branch tip"
            ),
            Self::Core(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RebaseError {}

/// Finds the merge base (nearest common ancestor) of two commits via BFS.
///
/// Returns `None` if no common ancestor exists (disjoint histories).
pub fn find_merge_base(
    a: ObjectId,
    b: ObjectId,
    store: &ObjectStore,
) -> CoreResult<Option<ObjectId>> {
    if a == b {
        return Ok(Some(a));
    }

    let mut visited_a = HashSet::new();
    let mut visited_b = HashSet::new();
    let mut queue_a = VecDeque::new();
    let mut queue_b = VecDeque::new();

    visited_a.insert(a);
    visited_b.insert(b);
    queue_a.push_back(a);
    queue_b.push_back(b);

    loop {
        let a_done = queue_a.is_empty();
        let b_done = queue_b.is_empty();

        if a_done && b_done {
            return Ok(None);
        }

        // Expand one level from side A.
        if !a_done {
            let size = queue_a.len();
            for _ in 0..size {
                let oid = queue_a.pop_front().expect("checked non-empty");
                if visited_b.contains(&oid) {
                    return Ok(Some(oid));
                }
                if let Some(Object::Commit(commit)) = store.get(&oid)? {
                    for parent in &commit.parents {
                        if visited_a.insert(*parent) {
                            if visited_b.contains(parent) {
                                return Ok(Some(*parent));
                            }
                            queue_a.push_back(*parent);
                        }
                    }
                }
            }
        }

        // Expand one level from side B.
        if !b_done {
            let size = queue_b.len();
            for _ in 0..size {
                let oid = queue_b.pop_front().expect("checked non-empty");
                if visited_a.contains(&oid) {
                    return Ok(Some(oid));
                }
                if let Some(Object::Commit(commit)) = store.get(&oid)? {
                    for parent in &commit.parents {
                        if visited_b.insert(*parent) {
                            if visited_a.contains(parent) {
                                return Ok(Some(*parent));
                            }
                            queue_b.push_back(*parent);
                        }
                    }
                }
            }
        }
    }
}

/// Collects the linear chain of commits from `tip` back to (but not including) `base`.
///
/// Returns commits in chronological order (oldest first).
///
/// # Errors
///
/// Returns [`CoreError`] if an object is missing or corrupt.
/// Returns [`RebaseError::MergeCommitInChain`] if any commit in the range has
/// more than one parent — OVC's linear rebase cannot reconstruct merge topology.
/// Returns [`RebaseError::BaseNotReachable`] if the first-parent chain reaches
/// a root commit without ever encountering `base`.
fn collect_commits(
    tip: ObjectId,
    base: ObjectId,
    store: &ObjectStore,
) -> Result<Vec<(ObjectId, Commit)>, RebaseError> {
    let mut commits = Vec::new();
    let mut current = tip;
    // Track visited OIDs to detect cycles in a corrupt object store.
    let mut visited = HashSet::new();

    while current != base {
        if !visited.insert(current) {
            // Cycle detected — the object graph is corrupt.
            return Err(CoreError::CorruptObject {
                reason: format!("cycle detected at commit {current} while collecting rebase chain"),
            }
            .into());
        }

        let obj = store
            .get(&current)?
            .ok_or(CoreError::ObjectNotFound(current))?;
        let Object::Commit(commit) = obj else {
            return Err(CoreError::CorruptObject {
                reason: format!("expected commit at {current}"),
            }
            .into());
        };

        // Refuse to rebase a chain that contains merge commits. Following only
        // the first parent would silently drop commits from the second parent,
        // producing a subtly wrong history with no diagnostic.
        if commit.parents.len() > 1 {
            return Err(RebaseError::MergeCommitInChain { commit: current });
        }

        let oid = current;
        if let Some(parent) = commit.parents.first().copied() {
            current = parent;
            commits.push((oid, commit));
        } else {
            // Root commit: base was not found anywhere in the first-parent chain.
            commits.push((oid, commit));
            // If base is ZERO we were asked to collect everything; otherwise
            // the caller made an invalid request.
            if base != ObjectId::ZERO {
                return Err(RebaseError::BaseNotReachable);
            }
            break;
        }
    }

    commits.reverse();
    Ok(commits)
}

/// Rebases the commits from `branch_tip` onto `onto`.
///
/// Finds the merge base between `branch_tip` and `onto`, collects all commits
/// from the merge base to `branch_tip`, then replays each commit by performing
/// a three-way merge of the commit's parent tree, the commit's tree, and the
/// current replay tip's tree.
pub fn rebase(
    branch_tip: ObjectId,
    onto: ObjectId,
    store: &mut ObjectStore,
    committer: &Identity,
) -> Result<RebaseResult, RebaseError> {
    let merge_base =
        find_merge_base(branch_tip, onto, store)?.ok_or(RebaseError::NoCommonAncestor)?;

    let commits_to_replay = collect_commits(branch_tip, merge_base, store)?;

    if commits_to_replay.is_empty() {
        return Ok(RebaseResult {
            new_tip: onto,
            replayed: Vec::new(),
        });
    }

    let mut current_tip = onto;
    let mut replayed = Vec::new();

    for (old_oid, commit) in &commits_to_replay {
        // Get the parent tree (the base for this commit's changes).
        let parent_tree = if let Some(parent_oid) = commit.parents.first() {
            let parent_obj = store
                .get(parent_oid)?
                .ok_or(CoreError::ObjectNotFound(*parent_oid))?;
            let Object::Commit(parent_commit) = parent_obj else {
                return Err(CoreError::CorruptObject {
                    reason: format!("expected commit at {parent_oid}"),
                }
                .into());
            };
            parent_commit.tree
        } else {
            // Root commit: use an empty tree as the base.
            let empty = Object::Tree(Tree {
                entries: Vec::new(),
            });
            store.insert(&empty)?
        };

        // Get the current tip's tree.
        let tip_obj = store
            .get(&current_tip)?
            .ok_or(CoreError::ObjectNotFound(current_tip))?;
        let tip_tree = match tip_obj {
            Object::Commit(c) => c.tree,
            _ => {
                return Err(CoreError::CorruptObject {
                    reason: format!("expected commit at {current_tip}"),
                }
                .into());
            }
        };

        // Three-way merge: base=parent_tree, ours=tip_tree, theirs=commit.tree
        let merge_result = merge::merge_trees(&parent_tree, &tip_tree, &commit.tree, store)?;

        if !merge_result.conflicts.is_empty() {
            let conflict_paths: Vec<String> = merge_result
                .conflicts
                .iter()
                .map(|c| c.path.clone())
                .collect();
            return Err(RebaseError::Conflict {
                commit: *old_oid,
                conflicts: conflict_paths,
                completed: replayed,
            });
        }

        // Build a new tree from the merged entries.
        let merged_tree = Object::Tree(Tree {
            entries: merge_result.entries,
        });
        let merged_tree_oid = store.insert(&merged_tree)?;

        // Determine sequence number from current tip.
        let tip_seq = store
            .get(&current_tip)?
            .and_then(|obj| match obj {
                Object::Commit(c) => Some(c.sequence),
                _ => None,
            })
            .unwrap_or(0);

        // Create a new commit preserving the original author and message.
        let new_commit = Commit {
            tree: merged_tree_oid,
            parents: vec![current_tip],
            author: commit.author.clone(),
            committer: committer.clone(),
            message: commit.message.clone(),
            signature: None,
            sequence: tip_seq + 1,
        };
        let new_oid = store.insert(&Object::Commit(new_commit))?;

        replayed.push((*old_oid, new_oid));
        current_tip = new_oid;
    }

    Ok(RebaseResult {
        new_tip: current_tip,
        replayed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::FileMode;
    use crate::object::TreeEntry;

    fn test_identity() -> Identity {
        Identity {
            name: "Test".into(),
            email: "test@test.com".into(),
            timestamp: 1_700_000_000,
            tz_offset_minutes: 0,
        }
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

    #[test]
    fn find_merge_base_same_commit() {
        let mut store = ObjectStore::default();
        let tree = make_tree_with_file(&mut store, "f.txt", b"data");
        let c = make_commit(&mut store, tree, vec![], "c", 1);

        let base = find_merge_base(c, c, &store).unwrap();
        assert_eq!(base, Some(c));
    }

    #[test]
    fn find_merge_base_linear() {
        let mut store = ObjectStore::default();
        let t1 = make_tree_with_file(&mut store, "f.txt", b"v1");
        let c1 = make_commit(&mut store, t1, vec![], "c1", 1);
        let t2 = make_tree_with_file(&mut store, "f.txt", b"v2");
        let c2 = make_commit(&mut store, t2, vec![c1], "c2", 2);
        let t3 = make_tree_with_file(&mut store, "f.txt", b"v3");
        let c3 = make_commit(&mut store, t3, vec![c2], "c3", 3);

        let base = find_merge_base(c2, c3, &store).unwrap();
        assert_eq!(base, Some(c2));
    }

    #[test]
    fn find_merge_base_branched() {
        let mut store = ObjectStore::default();
        let t1 = make_tree_with_file(&mut store, "f.txt", b"base");
        let c1 = make_commit(&mut store, t1, vec![], "root", 1);

        let t2 = make_tree_with_file(&mut store, "f.txt", b"branch-a");
        let c2 = make_commit(&mut store, t2, vec![c1], "a", 2);

        let t3 = make_tree_with_file(&mut store, "f.txt", b"branch-b");
        let c3 = make_commit(&mut store, t3, vec![c1], "b", 2);

        let base = find_merge_base(c2, c3, &store).unwrap();
        assert_eq!(base, Some(c1));
    }

    #[test]
    fn rebase_linear_two_commits() {
        let mut store = ObjectStore::default();
        let identity = test_identity();

        // Create a base commit.
        let base_tree = make_tree_with_file(&mut store, "base.txt", b"base content\n");
        let base_commit = make_commit(&mut store, base_tree, vec![], "base", 1);

        // Create "main" branch: add a file.
        let main_blob = store
            .insert(&Object::Blob(b"base content\n".to_vec()))
            .unwrap();
        let main_blob2 = store
            .insert(&Object::Blob(b"main file\n".to_vec()))
            .unwrap();
        let main_tree = Object::Tree(Tree {
            entries: vec![
                TreeEntry {
                    mode: FileMode::Regular,
                    name: b"base.txt".to_vec(),
                    oid: main_blob,
                },
                TreeEntry {
                    mode: FileMode::Regular,
                    name: b"main.txt".to_vec(),
                    oid: main_blob2,
                },
            ],
        });
        let main_tree_oid = store.insert(&main_tree).unwrap();
        let main_commit = make_commit(
            &mut store,
            main_tree_oid,
            vec![base_commit],
            "main change",
            2,
        );

        // Create "feature" branch: two commits modifying different files.
        let feat1_blob = store
            .insert(&Object::Blob(b"base content\n".to_vec()))
            .unwrap();
        let feat1_blob2 = store
            .insert(&Object::Blob(b"feature file 1\n".to_vec()))
            .unwrap();
        let feat1_tree = Object::Tree(Tree {
            entries: vec![
                TreeEntry {
                    mode: FileMode::Regular,
                    name: b"base.txt".to_vec(),
                    oid: feat1_blob,
                },
                TreeEntry {
                    mode: FileMode::Regular,
                    name: b"feat1.txt".to_vec(),
                    oid: feat1_blob2,
                },
            ],
        });
        let feat1_tree_oid = store.insert(&feat1_tree).unwrap();
        let feat1_commit = make_commit(
            &mut store,
            feat1_tree_oid,
            vec![base_commit],
            "feature 1",
            2,
        );

        let feat2_blob = store
            .insert(&Object::Blob(b"base content\n".to_vec()))
            .unwrap();
        let feat2_blob2 = store
            .insert(&Object::Blob(b"feature file 1\n".to_vec()))
            .unwrap();
        let feat2_blob3 = store
            .insert(&Object::Blob(b"feature file 2\n".to_vec()))
            .unwrap();
        let feat2_tree = Object::Tree(Tree {
            entries: vec![
                TreeEntry {
                    mode: FileMode::Regular,
                    name: b"base.txt".to_vec(),
                    oid: feat2_blob,
                },
                TreeEntry {
                    mode: FileMode::Regular,
                    name: b"feat1.txt".to_vec(),
                    oid: feat2_blob2,
                },
                TreeEntry {
                    mode: FileMode::Regular,
                    name: b"feat2.txt".to_vec(),
                    oid: feat2_blob3,
                },
            ],
        });
        let feat2_tree_oid = store.insert(&feat2_tree).unwrap();
        let feat2_commit = make_commit(
            &mut store,
            feat2_tree_oid,
            vec![feat1_commit],
            "feature 2",
            3,
        );

        // Rebase feature branch onto main.
        let result = rebase(feat2_commit, main_commit, &mut store, &identity).unwrap();

        assert_eq!(result.replayed.len(), 2);
        assert_ne!(result.new_tip, feat2_commit);

        // Verify the new tip commit has the expected tree.
        let new_tip_obj = store.get(&result.new_tip).unwrap().unwrap();
        let Object::Commit(new_tip_commit) = new_tip_obj else {
            panic!("expected commit");
        };

        // Verify the tree has files from both branches.
        let tree_obj = store.get(&new_tip_commit.tree).unwrap().unwrap();
        let Object::Tree(tree) = tree_obj else {
            panic!("expected tree");
        };

        let names: Vec<String> = tree
            .entries
            .iter()
            .map(|e| String::from_utf8_lossy(&e.name).into_owned())
            .collect();
        assert!(names.contains(&"base.txt".to_owned()));
        assert!(names.contains(&"main.txt".to_owned()));
        assert!(names.contains(&"feat2.txt".to_owned()));
    }
}
