//! Myers diff algorithm for line-oriented content comparison.
//!
//! Implements the classic O(ND) shortest-edit-script algorithm described in
//! "An O(ND) Difference Algorithm and Its Variations" by Eugene W. Myers (1986).
//! Optimizations include trimming common prefix and suffix before running the
//! core algorithm.

use std::fmt::Write;

/// A single diff operation describing how to transform old into new.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffOp {
    /// Lines are identical in both sequences.
    Equal {
        /// Starting line index in the old sequence.
        old_start: usize,
        /// Starting line index in the new sequence.
        new_start: usize,
        /// Number of equal lines.
        count: usize,
    },
    /// Lines present only in the new sequence.
    Insert {
        /// Starting line index in the new sequence.
        new_start: usize,
        /// Number of inserted lines.
        count: usize,
    },
    /// Lines present only in the old sequence.
    Delete {
        /// Starting line index in the old sequence.
        old_start: usize,
        /// Number of deleted lines.
        count: usize,
    },
}

/// A contiguous group of changes with surrounding context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// Starting line in the old file (1-based).
    pub old_start: usize,
    /// Number of lines from the old file.
    pub old_count: usize,
    /// Starting line in the new file (1-based).
    pub new_start: usize,
    /// Number of lines from the new file.
    pub new_count: usize,
    /// The individual lines making up this hunk.
    pub lines: Vec<HunkLine>,
}

/// A single line within a hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HunkLine {
    /// An unchanged context line.
    Context(Vec<u8>),
    /// A line added in the new version.
    Addition(Vec<u8>),
    /// A line removed from the old version.
    Deletion(Vec<u8>),
}

/// An individual line action for hunk building.
struct LineAction {
    /// 0=equal, 1=insert, 2=delete.
    kind: u8,
    /// Line index in the old sequence.
    old_idx: usize,
    /// Line index in the new sequence.
    new_idx: usize,
}

/// Splits byte content into lines, preserving line endings.
fn split_lines(data: &[u8]) -> Vec<&[u8]> {
    if data.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0;
    for (idx, &byte) in data.iter().enumerate() {
        if byte == b'\n' {
            lines.push(&data[start..=idx]);
            start = idx + 1;
        }
    }
    if start < data.len() {
        lines.push(&data[start..]);
    }
    lines
}

/// Maximum number of lines allowed per input before the diff engine falls back
/// to a single replace operation. This prevents O((N+M)^2) memory consumption
/// from the Myers trace matrix on very large files.
const MAX_DIFF_LINES: usize = 50_000;

/// Computes the shortest edit script between old and new byte content using Myers' algorithm.
///
/// The content is split into lines before comparison. Returns a sequence of
/// `DiffOp` values that, when applied in order, transform old into new.
///
/// If either input exceeds [`MAX_DIFF_LINES`], a single `Delete` + `Insert`
/// pair is returned instead of computing the full diff.
#[must_use]
pub fn diff_lines(old: &[u8], new: &[u8]) -> Vec<DiffOp> {
    let old_lines = split_lines(old);
    let new_lines = split_lines(new);

    if old_lines.len() > MAX_DIFF_LINES || new_lines.len() > MAX_DIFF_LINES {
        let mut ops = Vec::new();
        if !old_lines.is_empty() {
            ops.push(DiffOp::Delete {
                old_start: 0,
                count: old_lines.len(),
            });
        }
        if !new_lines.is_empty() {
            ops.push(DiffOp::Insert {
                new_start: 0,
                count: new_lines.len(),
            });
        }
        return ops;
    }

    myers_diff(&old_lines, &new_lines)
}

/// Core Myers O(ND) diff algorithm with common prefix/suffix trimming.
fn myers_diff(old: &[&[u8]], new: &[&[u8]]) -> Vec<DiffOp> {
    let old_len = old.len();
    let new_len = new.len();

    // Trim common prefix.
    let mut prefix_len = 0;
    while prefix_len < old_len && prefix_len < new_len && old[prefix_len] == new[prefix_len] {
        prefix_len += 1;
    }

    // Trim common suffix.
    let mut suffix_len = 0;
    while suffix_len < (old_len - prefix_len)
        && suffix_len < (new_len - prefix_len)
        && old[old_len - 1 - suffix_len] == new[new_len - 1 - suffix_len]
    {
        suffix_len += 1;
    }

    let old_trimmed = &old[prefix_len..old_len - suffix_len];
    let new_trimmed = &new[prefix_len..new_len - suffix_len];

    let inner_ops = if old_trimmed.is_empty() && new_trimmed.is_empty() {
        Vec::new()
    } else if old_trimmed.is_empty() {
        vec![DiffOp::Insert {
            new_start: prefix_len,
            count: new_trimmed.len(),
        }]
    } else if new_trimmed.is_empty() {
        vec![DiffOp::Delete {
            old_start: prefix_len,
            count: old_trimmed.len(),
        }]
    } else {
        myers_core(old_trimmed, new_trimmed, prefix_len)
    };

    let mut ops = Vec::new();

    if prefix_len > 0 {
        ops.push(DiffOp::Equal {
            old_start: 0,
            new_start: 0,
            count: prefix_len,
        });
    }

    ops.extend(inner_ops);

    if suffix_len > 0 {
        ops.push(DiffOp::Equal {
            old_start: old_len - suffix_len,
            new_start: new_len - suffix_len,
            count: suffix_len,
        });
    }

    ops
}

/// The core Myers algorithm on trimmed sequences.
///
/// `offset` is the number of prefix lines already trimmed, used to compute
/// correct line indices in the returned `DiffOp` values.
#[allow(clippy::many_single_char_names)]
fn myers_core(old: &[&[u8]], new: &[&[u8]], offset: usize) -> Vec<DiffOp> {
    let old_len = old.len();
    let new_len = new.len();
    let max_edit = old_len + new_len;

    // V array indexed by diagonal + max_edit (to handle negative diagonal values).
    let size = 2 * max_edit + 1;
    let mut frontier = vec![0usize; size];
    let mut trace: Vec<Vec<usize>> = Vec::new();

    let diag_idx = |diag: isize| -> usize {
        usize::try_from(diag + isize::try_from(max_edit).unwrap_or(0)).unwrap_or(0)
    };

    'outer: for edit_dist in 0..=max_edit {
        trace.push(frontier.clone());

        let edit_i = isize::try_from(edit_dist).unwrap_or(0);
        let mut diag = -edit_i;
        while diag <= edit_i {
            let mut col = if diag == -edit_i
                || (diag != edit_i && frontier[diag_idx(diag - 1)] < frontier[diag_idx(diag + 1)])
            {
                frontier[diag_idx(diag + 1)]
            } else {
                frontier[diag_idx(diag - 1)] + 1
            };

            let mut row = usize::try_from(isize::try_from(col).unwrap_or(0) - diag).unwrap_or(0);

            // Follow diagonal (equal lines): compare old[col] with new[row].
            #[allow(clippy::suspicious_operation_groupings)]
            while col < old_len && row < new_len && old[col] == new[row] {
                col += 1;
                row += 1;
            }

            frontier[diag_idx(diag)] = col;

            if col >= old_len && row >= new_len {
                break 'outer;
            }

            diag += 2;
        }
    }

    // Backtrack to find the actual edit script.
    backtrack(&trace, old_len, new_len, max_edit, offset)
}

/// Backtrack through the trace to reconstruct the edit operations.
#[allow(clippy::many_single_char_names)]
fn backtrack(
    trace: &[Vec<usize>],
    old_len: usize,
    new_len: usize,
    max_edit: usize,
    offset: usize,
) -> Vec<DiffOp> {
    let diag_idx = |diag: isize| -> usize {
        usize::try_from(diag + isize::try_from(max_edit).unwrap_or(0)).unwrap_or(0)
    };

    let mut col = old_len;
    let mut row = new_len;
    let mut ops: Vec<DiffOp> = Vec::new();

    for depth in (0..trace.len()).rev() {
        let frontier = &trace[depth];
        let depth_i = isize::try_from(depth).unwrap_or(0);
        let diag = isize::try_from(col).unwrap_or(0) - isize::try_from(row).unwrap_or(0);

        let prev_diag = if diag == -depth_i
            || (diag != depth_i && frontier[diag_idx(diag - 1)] < frontier[diag_idx(diag + 1)])
        {
            diag + 1
        } else {
            diag - 1
        };

        let prev_col = frontier[diag_idx(prev_diag)];
        let prev_row =
            usize::try_from(isize::try_from(prev_col).unwrap_or(0) - prev_diag).unwrap_or(0);

        // Diagonal moves (equal lines).
        while col > prev_col && row > prev_row {
            col -= 1;
            row -= 1;
            ops.push(DiffOp::Equal {
                old_start: col + offset,
                new_start: row + offset,
                count: 1,
            });
        }

        if depth > 0 {
            if col == prev_col {
                // Insertion.
                row -= 1;
                ops.push(DiffOp::Insert {
                    new_start: row + offset,
                    count: 1,
                });
            } else {
                // Deletion.
                col -= 1;
                ops.push(DiffOp::Delete {
                    old_start: col + offset,
                    count: 1,
                });
            }
        }
    }

    ops.reverse();
    merge_ops(ops)
}

/// Merges consecutive operations of the same type into single operations.
fn merge_ops(ops: Vec<DiffOp>) -> Vec<DiffOp> {
    let mut merged: Vec<DiffOp> = Vec::new();

    for op in ops {
        let should_merge = match (&op, merged.last()) {
            (
                DiffOp::Equal {
                    old_start: os,
                    new_start: ns,
                    ..
                },
                Some(DiffOp::Equal {
                    old_start: pos,
                    new_start: pns,
                    count: pc,
                }),
            ) => *os == pos + pc && *ns == pns + pc,
            (
                DiffOp::Insert { new_start: ns, .. },
                Some(DiffOp::Insert {
                    new_start: pns,
                    count: pc,
                }),
            ) => *ns == pns + pc,
            (
                DiffOp::Delete { old_start: os, .. },
                Some(DiffOp::Delete {
                    old_start: pos,
                    count: pc,
                }),
            ) => *os == pos + pc,
            _ => false,
        };

        if should_merge {
            if let Some(last) = merged.last_mut() {
                match (&op, last) {
                    (DiffOp::Equal { count, .. }, DiffOp::Equal { count: pc, .. })
                    | (DiffOp::Insert { count, .. }, DiffOp::Insert { count: pc, .. })
                    | (DiffOp::Delete { count, .. }, DiffOp::Delete { count: pc, .. }) => {
                        *pc += count;
                    }
                    _ => {}
                }
            }
        } else {
            merged.push(op);
        }
    }

    merged
}

/// Produces unified diff hunks from old and new content with the given context lines.
///
/// If either input exceeds [`MAX_DIFF_LINES`], an empty hunk list is returned
/// to avoid excessive memory use from the Myers trace matrix.
#[must_use]
pub fn diff_to_hunks(old: &[u8], new: &[u8], context_lines: usize) -> Vec<Hunk> {
    let old_lines = split_lines(old);
    let new_lines = split_lines(new);

    if old_lines.len() > MAX_DIFF_LINES || new_lines.len() > MAX_DIFF_LINES {
        return Vec::new();
    }

    let ops = myers_diff(&old_lines, &new_lines);

    if ops.is_empty() {
        return Vec::new();
    }

    let actions = expand_ops_to_actions(&ops);
    let change_indices = collect_change_indices(&actions);

    if change_indices.is_empty() {
        return Vec::new();
    }

    let groups = group_changes(&change_indices, context_lines);
    build_hunks_from_groups(&groups, &actions, &old_lines, &new_lines, context_lines)
}

/// Expands `DiffOp` into per-line `LineAction` entries.
fn expand_ops_to_actions(ops: &[DiffOp]) -> Vec<LineAction> {
    let mut actions = Vec::new();
    for op in ops {
        match *op {
            DiffOp::Equal {
                old_start,
                new_start,
                count,
            } => {
                for idx in 0..count {
                    actions.push(LineAction {
                        kind: 0,
                        old_idx: old_start + idx,
                        new_idx: new_start + idx,
                    });
                }
            }
            DiffOp::Delete { old_start, count } => {
                for idx in 0..count {
                    actions.push(LineAction {
                        kind: 2,
                        old_idx: old_start + idx,
                        new_idx: 0,
                    });
                }
            }
            DiffOp::Insert { new_start, count } => {
                for idx in 0..count {
                    actions.push(LineAction {
                        kind: 1,
                        old_idx: 0,
                        new_idx: new_start + idx,
                    });
                }
            }
        }
    }
    actions
}

/// Collects indices of non-equal actions.
fn collect_change_indices(actions: &[LineAction]) -> Vec<usize> {
    actions
        .iter()
        .enumerate()
        .filter(|(_, a)| a.kind != 0)
        .map(|(i, _)| i)
        .collect()
}

/// Groups nearby changes so hunks can include shared context.
fn group_changes(change_indices: &[usize], context_lines: usize) -> Vec<(usize, usize)> {
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let mut group_start = change_indices[0];
    let mut group_end = change_indices[0];

    for &ci in &change_indices[1..] {
        if ci > group_end + 2 * context_lines + 1 {
            groups.push((group_start, group_end));
            group_start = ci;
        }
        group_end = ci;
    }
    groups.push((group_start, group_end));
    groups
}

/// Builds `Hunk` values from grouped change ranges.
fn build_hunks_from_groups(
    groups: &[(usize, usize)],
    actions: &[LineAction],
    old_lines: &[&[u8]],
    new_lines: &[&[u8]],
    context_lines: usize,
) -> Vec<Hunk> {
    let mut hunks = Vec::new();

    for &(gs, ge) in groups {
        let ctx_start = gs.saturating_sub(context_lines);
        let ctx_end = (ge + context_lines + 1).min(actions.len());

        let mut hunk_lines = Vec::new();
        let mut old_start = usize::MAX;
        let mut new_start = usize::MAX;
        let mut old_count = 0usize;
        let mut new_count = 0usize;

        for action in &actions[ctx_start..ctx_end] {
            match action.kind {
                0 => {
                    if old_start == usize::MAX {
                        old_start = action.old_idx;
                        new_start = action.new_idx;
                    }
                    hunk_lines.push(HunkLine::Context(old_lines[action.old_idx].to_vec()));
                    old_count += 1;
                    new_count += 1;
                }
                1 => {
                    if new_start == usize::MAX {
                        new_start = action.new_idx;
                    }
                    if old_start == usize::MAX {
                        old_start = action.new_idx.min(old_lines.len());
                    }
                    hunk_lines.push(HunkLine::Addition(new_lines[action.new_idx].to_vec()));
                    new_count += 1;
                }
                2 => {
                    if old_start == usize::MAX {
                        old_start = action.old_idx;
                    }
                    if new_start == usize::MAX {
                        new_start = action.old_idx.min(new_lines.len());
                    }
                    hunk_lines.push(HunkLine::Deletion(old_lines[action.old_idx].to_vec()));
                    old_count += 1;
                }
                _ => {}
            }
        }

        hunks.push(Hunk {
            old_start: old_start + 1, // 1-based
            old_count,
            new_start: new_start + 1, // 1-based
            new_count,
            lines: hunk_lines,
        });
    }

    hunks
}

/// Formats hunks as a unified diff string.
#[must_use]
pub fn format_unified_diff(hunks: &[Hunk], old_name: &str, new_name: &str) -> String {
    let mut out = String::new();

    if hunks.is_empty() {
        return out;
    }

    let _ = writeln!(out, "--- {old_name}");
    let _ = writeln!(out, "+++ {new_name}");

    for hunk in hunks {
        let _ = writeln!(
            out,
            "@@ -{},{} +{},{} @@",
            hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
        );

        for line in &hunk.lines {
            let (prefix, data) = match line {
                HunkLine::Context(data) => (" ", data.as_slice()),
                HunkLine::Addition(data) => ("+", data.as_slice()),
                HunkLine::Deletion(data) => ("-", data.as_slice()),
            };
            let _ = write!(out, "{prefix}");
            let _ = out.write_str(&String::from_utf8_lossy(data));
            if !data.ends_with(b"\n") {
                let _ = writeln!(out);
            }
        }
    }

    out
}

/// Returns `true` if the content appears to be binary (contains NUL in first 8192 bytes).
#[must_use]
pub fn is_binary(content: &[u8]) -> bool {
    let check_len = content.len().min(8192);
    content[..check_len].contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_files_produce_no_changes() {
        let data = b"line1\nline2\nline3\n";
        let ops = diff_lines(data, data);
        for op in &ops {
            assert!(matches!(op, DiffOp::Equal { .. }));
        }
    }

    #[test]
    fn empty_to_content() {
        let ops = diff_lines(b"", b"hello\n");
        assert!(ops.iter().any(|op| matches!(op, DiffOp::Insert { .. })));
    }

    #[test]
    fn content_to_empty() {
        let ops = diff_lines(b"hello\n", b"");
        assert!(ops.iter().any(|op| matches!(op, DiffOp::Delete { .. })));
    }

    #[test]
    fn both_empty() {
        let ops = diff_lines(b"", b"");
        assert!(ops.is_empty());
    }

    #[test]
    fn addition_in_middle() {
        let old = b"a\nc\n";
        let new = b"a\nb\nc\n";
        let ops = diff_lines(old, new);

        let has_insert = ops.iter().any(|op| matches!(op, DiffOp::Insert { .. }));
        assert!(has_insert);

        // Verify the diff produces correct hunks.
        let hunks = diff_to_hunks(old, new, 3);
        assert!(!hunks.is_empty());
        let has_addition = hunks[0]
            .lines
            .iter()
            .any(|l| matches!(l, HunkLine::Addition(_)));
        assert!(has_addition);
    }

    #[test]
    fn deletion_in_middle() {
        let old = b"a\nb\nc\n";
        let new = b"a\nc\n";
        let ops = diff_lines(old, new);
        let has_delete = ops.iter().any(|op| matches!(op, DiffOp::Delete { .. }));
        assert!(has_delete);
    }

    #[test]
    fn modification() {
        let old = b"a\nb\nc\n";
        let new = b"a\nB\nc\n";
        let ops = diff_lines(old, new);
        let has_delete = ops.iter().any(|op| matches!(op, DiffOp::Delete { .. }));
        let has_insert = ops.iter().any(|op| matches!(op, DiffOp::Insert { .. }));
        assert!(has_delete);
        assert!(has_insert);
    }

    #[test]
    fn unified_diff_format() {
        let old = b"a\nb\nc\n";
        let new = b"a\nB\nc\n";
        let hunks = diff_to_hunks(old, new, 3);
        let unified = format_unified_diff(&hunks, "a/file.txt", "b/file.txt");
        assert!(unified.contains("--- a/file.txt"));
        assert!(unified.contains("+++ b/file.txt"));
        assert!(unified.contains("@@"));
    }

    #[test]
    fn is_binary_detects_nul() {
        assert!(is_binary(b"hello\x00world"));
        assert!(!is_binary(b"hello world"));
        assert!(!is_binary(b""));
    }

    #[test]
    fn large_diff() {
        let mut old_s = String::new();
        for i in 0..100 {
            let _ = writeln!(old_s, "line {i}");
        }
        let old = old_s.into_bytes();
        let mut new = old.clone();
        // Insert a line at position ~50.
        let insert_pos = new.windows(7).position(|w| w == b"line 50").unwrap_or(0);
        let insertion = b"INSERTED LINE\n";
        new.splice(insert_pos..insert_pos, insertion.iter().copied());

        let ops = diff_lines(&old, &new);
        let has_insert = ops.iter().any(|op| matches!(op, DiffOp::Insert { .. }));
        assert!(has_insert);
    }
}
