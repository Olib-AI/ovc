//! Three-way merge for content and tree objects.
//!
//! Implements line-level three-way merging using the diff algorithm from [`crate::diff`].
//! Non-overlapping changes from both sides are applied cleanly; overlapping
//! changes produce conflict markers.

use std::collections::BTreeMap;

use crate::diff::{self, DiffOp};
use crate::error::{CoreError, CoreResult};
use crate::id::ObjectId;
use crate::object::{Object, TreeEntry};
use crate::store::ObjectStore;

/// The result of a content merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeResult {
    /// The merge completed without conflicts.
    Clean(Vec<u8>),
    /// The merge has conflicts that need manual resolution.
    Conflict(MergeConflict),
}

/// Details of a conflicted merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeConflict {
    /// The merged content with conflict markers embedded.
    pub content: Vec<u8>,
    /// The number of conflict regions.
    pub conflict_count: usize,
}

/// The result of a tree-level merge.
#[derive(Debug, Clone)]
pub struct TreeMergeResult {
    /// The merged tree entries.
    pub entries: Vec<TreeEntry>,
    /// Paths that had conflicts.
    pub conflicts: Vec<TreeConflict>,
}

/// A conflict at the tree level.
#[derive(Debug, Clone)]
pub struct TreeConflict {
    /// The path of the conflicting entry.
    pub path: String,
    /// Description of the conflict.
    pub kind: TreeConflictKind,
}

/// The kind of tree-level conflict.
#[derive(Debug, Clone)]
pub enum TreeConflictKind {
    /// Both sides modified the same file differently.
    ModifyModify {
        /// The merged content with conflict markers.
        content: Vec<u8>,
    },
    /// One side modified while the other deleted.
    ModifyDelete {
        /// Which side modified (true=ours, false=theirs).
        modified_by_ours: bool,
    },
    /// Both sides added a file at the same path with different content.
    AddAdd,
}

/// Performs a three-way merge of byte content.
///
/// Diffs `base` against both `ours` and `theirs`, then walks both diffs
/// simultaneously. Non-overlapping changes are applied cleanly; overlapping
/// changes produce conflict markers.
#[must_use]
pub fn three_way_merge(base: &[u8], ours: &[u8], theirs: &[u8]) -> MergeResult {
    // Fast paths.
    if ours == theirs {
        return MergeResult::Clean(ours.to_vec());
    }
    if base == ours {
        return MergeResult::Clean(theirs.to_vec());
    }
    if base == theirs {
        return MergeResult::Clean(ours.to_vec());
    }

    let base_lines = split_lines(base);
    let ours_lines = split_lines(ours);
    let theirs_lines = split_lines(theirs);

    let ours_ops = diff::diff_lines(base, ours);
    let theirs_ops = diff::diff_lines(base, theirs);

    // Build change maps: base_line_index -> what happened.
    let ours_changes = build_change_map(&ours_ops, &ours_lines);
    let theirs_changes = build_change_map(&theirs_ops, &theirs_lines);

    let mut result: Vec<u8> = Vec::new();
    let mut conflict_count = 0usize;
    let mut base_idx = 0usize;

    while base_idx < base_lines.len() {
        let our_change = ours_changes.get(&base_idx);
        let their_change = theirs_changes.get(&base_idx);

        match (our_change, their_change) {
            (None, None) => {
                // No changes on either side, keep base line.
                result.extend_from_slice(base_lines[base_idx]);
                base_idx += 1;
            }
            (Some(change), None) => {
                // Only our side changed.
                apply_change(change, &mut result);
                if change.base_count == 0 {
                    // Pure insertion: emit the base line at this position too,
                    // since no base lines were consumed by the change.
                    result.extend_from_slice(base_lines[base_idx]);
                    base_idx += 1;
                } else {
                    base_idx += change.base_count;
                }
            }
            (None, Some(change)) => {
                // Only their side changed.
                apply_change(change, &mut result);
                if change.base_count == 0 {
                    result.extend_from_slice(base_lines[base_idx]);
                    base_idx += 1;
                } else {
                    base_idx += change.base_count;
                }
            }
            (Some(ours_c), Some(theirs_c)) => {
                // Both sides changed the same region.
                if ours_c.new_lines == theirs_c.new_lines {
                    // Both made identical changes - apply once.
                    apply_change(ours_c, &mut result);
                } else {
                    // Conflict.
                    conflict_count += 1;
                    result.extend_from_slice(b"<<<<<<< ours\n");
                    for line in &ours_c.new_lines {
                        result.extend_from_slice(line);
                    }
                    result.extend_from_slice(b"=======\n");
                    for line in &theirs_c.new_lines {
                        result.extend_from_slice(line);
                    }
                    result.extend_from_slice(b">>>>>>> theirs\n");
                }
                let consumed = ours_c.base_count.max(theirs_c.base_count);
                if consumed == 0 {
                    result.extend_from_slice(base_lines[base_idx]);
                    base_idx += 1;
                } else {
                    base_idx += consumed;
                }
            }
        }
    }

    // Handle insertions past the end of base.
    let ours_tail = ours_changes.get(&base_lines.len());
    let theirs_tail = theirs_changes.get(&base_lines.len());
    match (ours_tail, theirs_tail) {
        (Some(ours_c), Some(theirs_c)) => {
            if ours_c.new_lines == theirs_c.new_lines {
                // Both sides inserted the same content; emit once.
                apply_change(ours_c, &mut result);
            } else {
                // Different trailing insertions; emit conflict markers.
                conflict_count += 1;
                result.extend_from_slice(b"<<<<<<< ours\n");
                for line in &ours_c.new_lines {
                    result.extend_from_slice(line);
                }
                result.extend_from_slice(b"=======\n");
                for line in &theirs_c.new_lines {
                    result.extend_from_slice(line);
                }
                result.extend_from_slice(b">>>>>>> theirs\n");
            }
        }
        (Some(change), None) | (None, Some(change)) => apply_change(change, &mut result),
        (None, None) => {}
    }

    if conflict_count == 0 {
        MergeResult::Clean(result)
    } else {
        MergeResult::Conflict(MergeConflict {
            content: result,
            conflict_count,
        })
    }
}

/// A region of change relative to the base.
#[derive(Debug, Clone)]
struct ChangeRegion {
    /// Number of base lines consumed by this change.
    base_count: usize,
    /// The replacement lines (may be empty for deletions).
    new_lines: Vec<Vec<u8>>,
}

/// Builds a map from base line index to change regions.
fn build_change_map(ops: &[DiffOp], new_lines: &[&[u8]]) -> BTreeMap<usize, ChangeRegion> {
    let mut changes = BTreeMap::new();

    for op in ops {
        match *op {
            DiffOp::Equal { .. } => {
                // No change.
            }
            DiffOp::Delete { old_start, count } => {
                changes.insert(
                    old_start,
                    ChangeRegion {
                        base_count: count,
                        new_lines: Vec::new(),
                    },
                );
            }
            DiffOp::Insert { new_start, count } => {
                let lines: Vec<Vec<u8>> = (new_start..new_start + count)
                    .filter_map(|i| new_lines.get(i).map(|l| l.to_vec()))
                    .collect();
                // Insertions happen "before" the corresponding base position.
                // Use new_start as a proxy; for insertions at the start, this is 0.
                let base_pos = find_base_position_for_insert(ops, new_start);
                let entry = changes.entry(base_pos).or_insert_with(|| ChangeRegion {
                    base_count: 0,
                    new_lines: Vec::new(),
                });
                entry.new_lines.extend(lines);
            }
        }
    }

    changes
}

/// Finds the base line index where an insertion at `new_idx` should be anchored.
fn find_base_position_for_insert(ops: &[DiffOp], new_idx: usize) -> usize {
    // Walk ops to find the base position corresponding to this new index.
    let mut base_pos = 0;
    let mut new_pos = 0;

    for op in ops {
        match *op {
            DiffOp::Equal {
                old_start,
                new_start,
                count,
            } => {
                if new_idx >= new_start && new_idx < new_start + count {
                    return old_start + (new_idx - new_start);
                }
                base_pos = old_start + count;
                new_pos = new_start + count;
            }
            DiffOp::Delete { old_start, count } => {
                base_pos = old_start + count;
            }
            DiffOp::Insert { new_start, count } => {
                if new_idx >= new_start && new_idx < new_start + count {
                    return base_pos;
                }
                new_pos = new_start + count;
            }
        }
    }

    let _ = new_pos;
    base_pos
}

/// Applies a change region to the output buffer.
fn apply_change(change: &ChangeRegion, result: &mut Vec<u8>) {
    for line in &change.new_lines {
        result.extend_from_slice(line);
    }
}

/// Splits byte content into lines, preserving line endings.
fn split_lines(data: &[u8]) -> Vec<&[u8]> {
    if data.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, &b) in data.iter().enumerate() {
        if b == b'\n' {
            lines.push(&data[start..=i]);
            start = i + 1;
        }
    }
    if start < data.len() {
        lines.push(&data[start..]);
    }
    lines
}

/// Merges two tree objects with a common base tree.
///
/// Walks all three trees, applies non-conflicting changes, and reports conflicts.
#[allow(clippy::too_many_lines)]
pub fn merge_trees(
    base_tree: &ObjectId,
    our_tree: &ObjectId,
    their_tree: &ObjectId,
    store: &mut ObjectStore,
) -> CoreResult<TreeMergeResult> {
    let base_entries = load_tree_entries(base_tree, store)?;
    let our_entries = load_tree_entries(our_tree, store)?;
    let their_entries = load_tree_entries(their_tree, store)?;

    let mut result_entries = Vec::new();
    let mut conflicts = Vec::new();

    // Collect all unique paths across all three trees. Using a BTreeSet
    // eliminates O(n^2) deduplication and produces sorted output directly.
    let all_paths: Vec<String> = base_entries
        .keys()
        .chain(our_entries.keys())
        .chain(their_entries.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .cloned()
        .collect();

    for path in &all_paths {
        let base = base_entries.get(path);
        let ours = our_entries.get(path);
        let theirs = their_entries.get(path);

        match (base, ours, theirs) {
            // No change on either side.
            (Some(b), Some(o), Some(t)) if b.oid == o.oid && b.oid == t.oid => {
                result_entries.push(b.clone());
            }
            // Only ours changed.
            (Some(b), Some(o), Some(t)) if b.oid == t.oid => {
                result_entries.push(o.clone());
            }
            // Only theirs changed.
            (Some(b), Some(o), Some(t)) if b.oid == o.oid => {
                result_entries.push(t.clone());
            }
            // Both changed to the same thing.
            (Some(_), Some(o), Some(t)) if o.oid == t.oid => {
                result_entries.push(o.clone());
            }
            // Both changed differently - modify/modify conflict or recursive tree merge.
            (Some(b), Some(o), Some(t)) => {
                // If all three are directory entries, recursively merge subtrees.
                if b.mode == crate::object::FileMode::Directory
                    && o.mode == crate::object::FileMode::Directory
                    && t.mode == crate::object::FileMode::Directory
                {
                    let sub_result = merge_trees(&b.oid, &o.oid, &t.oid, store)?;
                    let merged_subtree = Object::Tree(crate::object::Tree {
                        entries: sub_result.entries,
                    });
                    let merged_oid = store.insert(&merged_subtree)?;
                    let mut merged_entry = o.clone();
                    merged_entry.oid = merged_oid;
                    result_entries.push(merged_entry);
                    // Propagate any sub-conflicts with prefixed paths.
                    for mut sc in sub_result.conflicts {
                        sc.path = format!("{path}/{}", sc.path);
                        conflicts.push(sc);
                    }
                    continue;
                }

                // Attempt content-level three-way merge if all three are blobs.
                let base_blob = store.get(&b.oid)?.and_then(|obj| match obj {
                    Object::Blob(data) => Some(data),
                    _ => None,
                });
                let ours_blob = store.get(&o.oid)?.and_then(|obj| match obj {
                    Object::Blob(data) => Some(data),
                    _ => None,
                });
                let theirs_blob = store.get(&t.oid)?.and_then(|obj| match obj {
                    Object::Blob(data) => Some(data),
                    _ => None,
                });

                if let (Some(base_data), Some(ours_data), Some(theirs_data)) =
                    (base_blob, ours_blob, theirs_blob)
                {
                    // Binary files must not be passed through the line-based
                    // three-way merge: conflict markers embedded as raw bytes
                    // corrupt the file irreversibly. Instead:
                    //   • identical result on both sides → no conflict (use either)
                    //   • only one side changed from base → take that side cleanly
                    //   • both sides changed differently → conflict, keep ours
                    if diff::is_binary(&base_data)
                        || diff::is_binary(&ours_data)
                        || diff::is_binary(&theirs_data)
                    {
                        if ours_data == theirs_data {
                            // Both sides agree; store the (identical) result.
                            let merged_oid = store.insert(&Object::Blob(ours_data))?;
                            let mut merged_entry = o.clone();
                            merged_entry.oid = merged_oid;
                            result_entries.push(merged_entry);
                        } else if base_data == ours_data {
                            // Only theirs changed; take theirs.
                            result_entries.push(t.clone());
                        } else if base_data == theirs_data {
                            // Only ours changed; take ours.
                            result_entries.push(o.clone());
                        } else {
                            // Both sides changed differently; report conflict,
                            // keep ours (do NOT attempt line merge on binary).
                            result_entries.push(o.clone());
                            conflicts.push(TreeConflict {
                                path: path.clone(),
                                kind: TreeConflictKind::ModifyModify {
                                    content: Vec::new(),
                                },
                            });
                        }
                    } else {
                        match three_way_merge(&base_data, &ours_data, &theirs_data) {
                            MergeResult::Clean(merged) => {
                                let merged_oid = store.insert(&Object::Blob(merged))?;
                                let mut merged_entry = o.clone();
                                merged_entry.oid = merged_oid;
                                result_entries.push(merged_entry);
                            }
                            MergeResult::Conflict(conflict) => {
                                result_entries.push(o.clone());
                                conflicts.push(TreeConflict {
                                    path: path.clone(),
                                    kind: TreeConflictKind::ModifyModify {
                                        content: conflict.content,
                                    },
                                });
                            }
                        }
                    }
                } else {
                    // Cannot merge non-blob objects; report conflict with empty content.
                    result_entries.push(o.clone());
                    conflicts.push(TreeConflict {
                        path: path.clone(),
                        kind: TreeConflictKind::ModifyModify {
                            content: Vec::new(),
                        },
                    });
                }
            }
            // Deleted by theirs, still in ours.
            (Some(b), Some(o), None) => {
                if b.oid == o.oid {
                    // Ours didn't change, theirs deleted. Accept deletion.
                } else {
                    // Ours modified, theirs deleted. Conflict.
                    result_entries.push(o.clone());
                    conflicts.push(TreeConflict {
                        path: path.clone(),
                        kind: TreeConflictKind::ModifyDelete {
                            modified_by_ours: true,
                        },
                    });
                }
            }
            // Deleted by ours, still in theirs.
            (Some(b), None, Some(t)) => {
                if b.oid == t.oid {
                    // Theirs didn't change, ours deleted. Accept deletion.
                } else {
                    // Theirs modified, ours deleted. Conflict.
                    result_entries.push(t.clone());
                    conflicts.push(TreeConflict {
                        path: path.clone(),
                        kind: TreeConflictKind::ModifyDelete {
                            modified_by_ours: false,
                        },
                    });
                }
            }
            // Both deleted, or not in any tree.
            (Some(_) | None, None, None) => {
                // Both agree to delete, or path not in any tree.
            }
            // Added by ours only.
            (None, Some(o), None) => {
                result_entries.push(o.clone());
            }
            // Added by theirs only.
            (None, None, Some(t)) => {
                result_entries.push(t.clone());
            }
            // Added by both with same content.
            (None, Some(o), Some(t)) if o.oid == t.oid => {
                result_entries.push(o.clone());
            }
            // Added by both with different content.
            (None, Some(o), Some(_)) => {
                result_entries.push(o.clone());
                conflicts.push(TreeConflict {
                    path: path.clone(),
                    kind: TreeConflictKind::AddAdd,
                });
            }
        }
    }

    Ok(TreeMergeResult {
        entries: result_entries,
        conflicts,
    })
}

/// Loads tree entries as a name-indexed map.
fn load_tree_entries(
    oid: &ObjectId,
    store: &ObjectStore,
) -> CoreResult<BTreeMap<String, TreeEntry>> {
    let obj = store.get(oid)?.ok_or(CoreError::ObjectNotFound(*oid))?;
    let Object::Tree(tree) = obj else {
        return Err(CoreError::CorruptObject {
            reason: format!("expected tree, got {:?}", obj.object_type()),
        });
    };

    let mut map = BTreeMap::new();
    for entry in tree.entries {
        let name = String::from_utf8_lossy(&entry.name).into_owned();
        map.insert(name, entry);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{FileMode, Tree};

    #[test]
    fn clean_merge_non_overlapping() {
        let base = b"line1\nline2\nline3\nline4\nline5\n";
        let ours = b"line1\nours2\nline3\nline4\nline5\n";
        let theirs = b"line1\nline2\nline3\ntheirs4\nline5\n";

        match three_way_merge(base, ours, theirs) {
            MergeResult::Clean(content) => {
                let s = String::from_utf8_lossy(&content);
                assert!(s.contains("ours2"));
                assert!(s.contains("theirs4"));
                assert!(!s.contains("<<<<<<<"));
            }
            MergeResult::Conflict(_) => panic!("expected clean merge"),
        }
    }

    #[test]
    fn conflict_on_same_line() {
        let base = b"line1\nline2\nline3\n";
        let ours = b"line1\nours\nline3\n";
        let theirs = b"line1\ntheirs\nline3\n";

        match three_way_merge(base, ours, theirs) {
            MergeResult::Conflict(conflict) => {
                assert!(conflict.conflict_count > 0);
                let s = String::from_utf8_lossy(&conflict.content);
                assert!(s.contains("<<<<<<< ours"));
                assert!(s.contains("======="));
                assert!(s.contains(">>>>>>> theirs"));
            }
            MergeResult::Clean(_) => panic!("expected conflict"),
        }
    }

    #[test]
    fn identical_changes_merge_cleanly() {
        let base = b"line1\nline2\nline3\n";
        let ours = b"line1\nchanged\nline3\n";
        let theirs = b"line1\nchanged\nline3\n";

        match three_way_merge(base, ours, theirs) {
            MergeResult::Clean(content) => {
                assert_eq!(content, ours);
            }
            MergeResult::Conflict(_) => panic!("expected clean merge for identical changes"),
        }
    }

    #[test]
    fn one_side_unchanged() {
        let base = b"original\n";
        let ours = b"modified\n";
        let theirs = b"original\n";

        match three_way_merge(base, ours, theirs) {
            MergeResult::Clean(content) => {
                assert_eq!(content, ours);
            }
            MergeResult::Conflict(_) => panic!("expected clean merge"),
        }
    }

    #[test]
    fn both_sides_identical_to_each_other() {
        let base = b"base\n";
        let both = b"same change\n";

        match three_way_merge(base, both, both) {
            MergeResult::Clean(content) => {
                assert_eq!(content, both);
            }
            MergeResult::Conflict(_) => panic!("expected clean merge"),
        }
    }

    #[test]
    fn tree_merge_basic() {
        let mut store = ObjectStore::default();

        // Use multi-line content where both sides modify the same line to produce
        // a genuine modify/modify conflict.
        let blob_a = store
            .insert(&Object::Blob(b"line1\nshared\nline3\n".to_vec()))
            .unwrap();
        let blob_b = store
            .insert(&Object::Blob(b"line1\nours\nline3\n".to_vec()))
            .unwrap();
        let blob_c = store
            .insert(&Object::Blob(b"line1\ntheirs\nline3\n".to_vec()))
            .unwrap();

        let base = Object::Tree(Tree {
            entries: vec![TreeEntry {
                mode: FileMode::Regular,
                name: b"file.txt".to_vec(),
                oid: blob_a,
            }],
        });
        let base_oid = store.insert(&base).unwrap();

        let ours = Object::Tree(Tree {
            entries: vec![TreeEntry {
                mode: FileMode::Regular,
                name: b"file.txt".to_vec(),
                oid: blob_b,
            }],
        });
        let ours_oid = store.insert(&ours).unwrap();

        let theirs = Object::Tree(Tree {
            entries: vec![TreeEntry {
                mode: FileMode::Regular,
                name: b"file.txt".to_vec(),
                oid: blob_c,
            }],
        });
        let theirs_oid = store.insert(&theirs).unwrap();

        let result = merge_trees(&base_oid, &ours_oid, &theirs_oid, &mut store).unwrap();
        assert!(
            !result.conflicts.is_empty(),
            "expected modify/modify conflict"
        );

        // Verify the conflict contains conflict markers.
        match &result.conflicts[0].kind {
            TreeConflictKind::ModifyModify { content } => {
                let s = String::from_utf8_lossy(content);
                assert!(s.contains("<<<<<<< ours"), "expected conflict markers");
            }
            other => panic!("expected ModifyModify, got {other:?}"),
        }
    }

    #[test]
    fn tree_merge_clean_content() {
        let mut store = ObjectStore::default();

        // Non-overlapping changes should merge cleanly at the content level.
        let blob_a = store
            .insert(&Object::Blob(b"line1\nline2\nline3\nline4\n".to_vec()))
            .unwrap();
        let blob_b = store
            .insert(&Object::Blob(b"ours1\nline2\nline3\nline4\n".to_vec()))
            .unwrap();
        let blob_c = store
            .insert(&Object::Blob(b"line1\nline2\nline3\ntheirs4\n".to_vec()))
            .unwrap();

        let base = Object::Tree(Tree {
            entries: vec![TreeEntry {
                mode: FileMode::Regular,
                name: b"file.txt".to_vec(),
                oid: blob_a,
            }],
        });
        let base_oid = store.insert(&base).unwrap();

        let ours = Object::Tree(Tree {
            entries: vec![TreeEntry {
                mode: FileMode::Regular,
                name: b"file.txt".to_vec(),
                oid: blob_b,
            }],
        });
        let ours_oid = store.insert(&ours).unwrap();

        let theirs = Object::Tree(Tree {
            entries: vec![TreeEntry {
                mode: FileMode::Regular,
                name: b"file.txt".to_vec(),
                oid: blob_c,
            }],
        });
        let theirs_oid = store.insert(&theirs).unwrap();

        let result = merge_trees(&base_oid, &ours_oid, &theirs_oid, &mut store).unwrap();
        assert!(result.conflicts.is_empty(), "expected clean content merge");

        // Verify the merged entry has the correct content.
        let merged_oid = result.entries[0].oid;
        let merged = store.get(&merged_oid).unwrap().unwrap();
        if let Object::Blob(data) = merged {
            let s = String::from_utf8_lossy(&data);
            assert!(s.contains("ours1"), "merged content should contain ours");
            assert!(
                s.contains("theirs4"),
                "merged content should contain theirs"
            );
        } else {
            panic!("expected blob");
        }
    }
}
