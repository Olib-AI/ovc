//! Line-by-line authorship tracking (blame).
//!
//! Walks commit history following a specific file path and attributes each
//! line in the current version to the commit that last changed it.

use crate::diff::{DiffOp, diff_lines};
use crate::error::{CoreError, CoreResult};
use crate::id::ObjectId;
use crate::index::Index;
use crate::object::Object;
use crate::store::ObjectStore;

/// A single line in a blame result, attributed to a specific commit.
#[derive(Debug, Clone)]
pub struct BlameLine {
    /// The commit that last modified this line.
    pub commit_id: ObjectId,
    /// Author name from the commit.
    pub author: String,
    /// Unix timestamp of the commit.
    pub timestamp: i64,
    /// 1-based line number in the current file.
    pub line_number: usize,
    /// The line content.
    pub content: String,
}

/// Computes blame annotations for every line of a file at the given HEAD commit.
///
/// Walks parent commits, diffing adjacent versions to determine which commit
/// introduced each line. Lines unchanged between a commit and its parent are
/// recursively attributed to the parent.
#[allow(clippy::too_many_lines)]
pub fn blame(
    file_path: &str,
    head_oid: ObjectId,
    store: &ObjectStore,
) -> CoreResult<Vec<BlameLine>> {
    // Resolve the head commit and get the file content.
    let commit = get_commit(store, &head_oid)?;
    let current_content =
        resolve_file_in_tree(file_path, &commit.tree, store)?.ok_or_else(|| {
            CoreError::FormatError {
                reason: format!("file not found in HEAD tree: {file_path}"),
            }
        })?;

    let lines = split_into_lines(&current_content);
    if lines.is_empty() {
        return Ok(Vec::new());
    }

    // Track which line indices still need attribution.
    // Each element maps a "current line index" to the line index in the
    // version we are currently comparing.
    let line_count = lines.len();
    let mut attributions: Vec<Option<(ObjectId, String, i64)>> = vec![None; line_count];
    // pending[i] = index into the "working version" for this original line
    let mut pending: Vec<(usize, usize)> = (0..line_count).map(|i| (i, i)).collect();

    let mut current_oid = head_oid;
    let mut current_blob = current_content;

    // Limit traversal depth to prevent runaway on malformed history.
    let max_depth = 10_000;

    for _ in 0..max_depth {
        if pending.is_empty() {
            break;
        }

        let commit = get_commit(store, &current_oid)?;

        if commit.parents.is_empty() {
            // Initial commit: all remaining lines are attributed here.
            for (orig_idx, _) in &pending {
                attributions[*orig_idx] = Some((
                    current_oid,
                    commit.author.name.clone(),
                    commit.author.timestamp,
                ));
            }
            break;
        }

        // For merge commits (more than one parent) use a best-parent
        // heuristic: pick the parent whose file content requires the fewest
        // diff operations to produce the current version.  This gives
        // substantially better attribution than blindly following the first
        // parent because it follows the side of the merge that actually
        // introduced the majority of the file's lines.
        //
        // Limitation: this heuristic does not perform a full recursive
        // multi-parent blame (i.e. it does not separately attribute lines
        // that were unique to each parent).  A fully correct implementation
        // would require tracking per-line parent provenance across all
        // parents simultaneously, which is significantly more complex.
        // The heuristic is correct for the common case where a merge commit
        // does not itself modify file content.
        let parent_oid = if commit.parents.len() > 1 {
            pick_best_parent(&commit.parents, file_path, &current_blob, store)
        } else {
            commit.parents[0]
        };

        let parent_commit = get_commit(store, &parent_oid)?;
        let parent_blob = resolve_file_in_tree(file_path, &parent_commit.tree, store)?;

        let parent_content = parent_blob.unwrap_or_default();

        if parent_content == current_blob {
            // File unchanged in this commit, pass all pending to parent.
            current_oid = parent_oid;
            current_blob = parent_content;
            continue;
        }

        // Diff parent vs current to find which lines changed.
        let ops = diff_lines(&parent_content, &current_blob);

        // Build a mapping: new_line_index -> Option<old_line_index>
        let current_line_count = if current_blob.is_empty() {
            0
        } else {
            split_into_lines(&current_blob).len()
        };
        let mut new_to_old: Vec<Option<usize>> = vec![None; current_line_count];

        for op in &ops {
            match *op {
                DiffOp::Equal {
                    old_start,
                    new_start,
                    count,
                } => {
                    for i in 0..count {
                        if new_start + i < new_to_old.len() {
                            new_to_old[new_start + i] = Some(old_start + i);
                        }
                    }
                }
                DiffOp::Insert { .. } | DiffOp::Delete { .. } => {}
            }
        }

        let mut next_pending = Vec::new();
        for (orig_idx, current_line_idx) in &pending {
            if *current_line_idx < new_to_old.len() {
                if let Some(old_idx) = new_to_old[*current_line_idx] {
                    // Line existed in parent, keep tracking.
                    next_pending.push((*orig_idx, old_idx));
                } else {
                    // Line was introduced in this commit.
                    attributions[*orig_idx] = Some((
                        current_oid,
                        commit.author.name.clone(),
                        commit.author.timestamp,
                    ));
                }
            } else {
                // Out of range, attribute to current.
                attributions[*orig_idx] = Some((
                    current_oid,
                    commit.author.name.clone(),
                    commit.author.timestamp,
                ));
            }
        }

        pending = next_pending;
        current_oid = parent_oid;
        current_blob = parent_content;
    }

    // Any still-unattributed lines get attributed to the deepest commit reached.
    if !pending.is_empty()
        && let Ok(commit) = get_commit(store, &current_oid)
    {
        for (orig_idx, _) in &pending {
            if attributions[*orig_idx].is_none() {
                attributions[*orig_idx] = Some((
                    current_oid,
                    commit.author.name.clone(),
                    commit.author.timestamp,
                ));
            }
        }
    }

    let result = lines
        .into_iter()
        .enumerate()
        .map(|(i, content)| {
            let (commit_id, author, timestamp) =
                attributions[i]
                    .clone()
                    .unwrap_or((head_oid, String::new(), 0));
            BlameLine {
                commit_id,
                author,
                timestamp,
                line_number: i + 1,
                content,
            }
        })
        .collect();

    Ok(result)
}

/// Selects the parent whose file content is closest to `current_blob`.
///
/// "Closest" is measured as the number of [`DiffOp`] operations produced by
/// diffing that parent's blob against `current_blob`.  Fewer operations means
/// fewer changes, so that parent is the most likely origin of the current
/// content.  Falls back to `parents[0]` if the store is missing objects or
/// the file does not exist in any parent.
fn pick_best_parent(
    parents: &[ObjectId],
    file_path: &str,
    current_blob: &[u8],
    store: &ObjectStore,
) -> ObjectId {
    let mut best_oid = parents[0];
    // Sentinel: lower is better.  u32::MAX means "no valid candidate yet".
    let mut best_ops = u32::MAX;

    for &parent_oid in parents {
        let parent_content = get_commit(store, &parent_oid)
            .ok()
            .and_then(|c| resolve_file_in_tree(file_path, &c.tree, store).ok())
            .flatten()
            .unwrap_or_default();

        let ops = diff_lines(&parent_content, current_blob);
        // Count only non-Equal operations as the distance metric.
        let distance = ops
            .iter()
            .filter(|op| !matches!(op, DiffOp::Equal { .. }))
            .count();
        #[allow(clippy::cast_possible_truncation)]
        let distance = distance.min(u32::MAX as usize) as u32;

        if distance < best_ops {
            best_ops = distance;
            best_oid = parent_oid;
        }
    }

    best_oid
}

/// Extracts a commit object from the store.
fn get_commit(store: &ObjectStore, oid: &ObjectId) -> CoreResult<crate::object::Commit> {
    let obj = store.get(oid)?.ok_or(CoreError::ObjectNotFound(*oid))?;
    match obj {
        Object::Commit(c) => Ok(c),
        _ => Err(CoreError::CorruptObject {
            reason: format!("expected commit object at {oid}"),
        }),
    }
}

/// Resolves a file path in a tree, returning the blob content if found.
fn resolve_file_in_tree(
    file_path: &str,
    tree_oid: &ObjectId,
    store: &ObjectStore,
) -> CoreResult<Option<Vec<u8>>> {
    let mut index = Index::new();
    index.read_tree(tree_oid, store)?;
    let entry = index.get_entry(file_path);
    match entry {
        Some(e) => {
            let obj = store.get(&e.oid)?;
            match obj {
                Some(Object::Blob(data)) => Ok(Some(data)),
                _ => Ok(None),
            }
        }
        None => Ok(None),
    }
}

/// Splits content into lines, stripping trailing newlines.
fn split_into_lines(content: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(content);
    text.lines().map(String::from).collect()
}
