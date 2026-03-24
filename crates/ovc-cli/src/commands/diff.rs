//! `ovc diff` — Show changes between versions.

use anyhow::{Context, Result};

use ovc_core::diff;
use ovc_core::object::Object;
use ovc_core::workdir::FileStatus;

use crate::app::DiffArgs;
use crate::context::{self, CliContext};
use crate::output;

#[allow(clippy::too_many_lines)]
pub fn execute(ctx: &CliContext, args: &DiffArgs) -> Result<()> {
    let (repo, workdir) = ctx.open_repo()?;

    // Check for `branch-a..branch-b` syntax in paths.
    if let Some(range) = args.paths.first().and_then(|p| parse_range_spec(p)) {
        return execute_range_diff(ctx, args, &repo, &range.0, &range.1);
    }

    let ignore = CliContext::load_ignore(&workdir);

    let head_tree = repo
        .ref_store()
        .resolve_head()
        .ok()
        .and_then(|oid| repo.get_object(&oid).ok().flatten())
        .and_then(|obj| match obj {
            Object::Commit(c) => Some(c.tree),
            _ => None,
        });

    let status = workdir
        .compute_status(
            repo.index(),
            head_tree.as_ref(),
            repo.object_store(),
            &ignore,
        )
        .context("failed to compute status")?;

    let path_filter: Option<std::collections::BTreeSet<&str>> = if args.paths.is_empty() {
        None
    } else {
        Some(args.paths.iter().map(String::as_str).collect())
    };

    // Collect diffs into a list for stat/name-only modes.
    let mut file_diffs: Vec<FileDiffResult> = Vec::new();

    if args.staged {
        for entry in &status {
            if !matches!(
                entry.staged,
                FileStatus::Added | FileStatus::Modified | FileStatus::Deleted
            ) {
                continue;
            }
            if let Some(ref filter) = path_filter
                && !filter.contains(entry.path.as_str())
            {
                continue;
            }

            let old_content = head_tree
                .as_ref()
                .and_then(|tree_oid| {
                    let mut head_index = ovc_core::index::Index::new();
                    head_index.read_tree(tree_oid, repo.object_store()).ok()?;
                    let head_entry = head_index.get_entry(&entry.path)?;
                    repo.get_object(&head_entry.oid)
                        .ok()
                        .flatten()
                        .and_then(|obj| match obj {
                            Object::Blob(data) => Some(data),
                            _ => None,
                        })
                })
                .unwrap_or_default();

            let new_content = repo
                .index()
                .get_entry(&entry.path)
                .and_then(|ie| {
                    repo.get_object(&ie.oid)
                        .ok()
                        .flatten()
                        .and_then(|obj| match obj {
                            Object::Blob(data) => Some(data),
                            _ => None,
                        })
                })
                .unwrap_or_default();

            file_diffs.push(FileDiffResult {
                path: entry.path.clone(),
                old: old_content,
                new: new_content,
            });
        }
    } else {
        for entry in &status {
            if entry.unstaged != FileStatus::Modified {
                continue;
            }
            if let Some(ref filter) = path_filter
                && !filter.contains(entry.path.as_str())
            {
                continue;
            }

            let index_content = repo
                .index()
                .get_entry(&entry.path)
                .and_then(|ie| {
                    repo.get_object(&ie.oid)
                        .ok()
                        .flatten()
                        .and_then(|obj| match obj {
                            Object::Blob(data) => Some(data),
                            _ => None,
                        })
                })
                .unwrap_or_default();

            let disk_content = workdir.read_file(&entry.path).unwrap_or_default();

            file_diffs.push(FileDiffResult {
                path: entry.path.clone(),
                old: index_content,
                new: disk_content,
            });
        }
    }

    print_diff_results(&file_diffs, args);

    Ok(())
}

struct FileDiffResult {
    path: String,
    old: Vec<u8>,
    new: Vec<u8>,
}

/// Parses a `ref-a..ref-b` range specification.
fn parse_range_spec(s: &str) -> Option<(String, String)> {
    // Must contain `..` but not be a file path.
    let idx = s.find("..")?;
    let left = &s[..idx];
    let right = &s[idx + 2..];
    if left.is_empty() || right.is_empty() {
        return None;
    }
    Some((left.to_owned(), right.to_owned()))
}

/// Executes a diff between two refs (branch-a..branch-b).
fn execute_range_diff(
    _ctx: &CliContext,
    args: &DiffArgs,
    repo: &ovc_core::repository::Repository,
    left_ref: &str,
    right_ref: &str,
) -> Result<()> {
    let left_oid = context::resolve_commit(left_ref, repo)?;
    let right_oid = context::resolve_commit(right_ref, repo)?;

    let left_obj = repo
        .get_object(&left_oid)?
        .ok_or_else(|| anyhow::anyhow!("commit not found: {left_oid}"))?;
    let Object::Commit(left_commit) = left_obj else {
        anyhow::bail!("not a commit: {left_oid}");
    };

    let right_obj = repo
        .get_object(&right_oid)?
        .ok_or_else(|| anyhow::anyhow!("commit not found: {right_oid}"))?;
    let Object::Commit(right_commit) = right_obj else {
        anyhow::bail!("not a commit: {right_oid}");
    };

    // Build indices from both trees.
    let mut left_index = ovc_core::index::Index::new();
    left_index
        .read_tree(&left_commit.tree, repo.object_store())
        .context("failed to read left tree")?;

    let mut right_index = ovc_core::index::Index::new();
    right_index
        .read_tree(&right_commit.tree, repo.object_store())
        .context("failed to read right tree")?;

    // Collect all unique paths from both trees.
    let left_entries: std::collections::BTreeMap<&str, &ovc_core::index::IndexEntry> = left_index
        .entries()
        .iter()
        .map(|e| (e.path.as_str(), e))
        .collect();
    let right_entries: std::collections::BTreeMap<&str, &ovc_core::index::IndexEntry> = right_index
        .entries()
        .iter()
        .map(|e| (e.path.as_str(), e))
        .collect();

    let mut all_paths: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    all_paths.extend(left_entries.keys());
    all_paths.extend(right_entries.keys());

    let mut file_diffs: Vec<FileDiffResult> = Vec::new();

    for path in all_paths {
        let old_content = left_entries
            .get(path)
            .and_then(|e| {
                repo.get_object(&e.oid)
                    .ok()
                    .flatten()
                    .and_then(|obj| match obj {
                        Object::Blob(data) => Some(data),
                        _ => None,
                    })
            })
            .unwrap_or_default();

        let new_content = right_entries
            .get(path)
            .and_then(|e| {
                repo.get_object(&e.oid)
                    .ok()
                    .flatten()
                    .and_then(|obj| match obj {
                        Object::Blob(data) => Some(data),
                        _ => None,
                    })
            })
            .unwrap_or_default();

        if old_content != new_content {
            file_diffs.push(FileDiffResult {
                path: path.to_owned(),
                old: old_content,
                new: new_content,
            });
        }
    }

    print_diff_results(&file_diffs, args);

    Ok(())
}

fn print_diff_results(file_diffs: &[FileDiffResult], args: &DiffArgs) {
    if args.name_only {
        for fd in file_diffs {
            if fd.old != fd.new {
                println!("{}", fd.path);
            }
        }
        return;
    }

    if args.stat {
        let mut total_insertions = 0usize;
        let mut total_deletions = 0usize;

        for fd in file_diffs {
            if diff::is_binary(&fd.old) || diff::is_binary(&fd.new) {
                println!(" {} | Bin", fd.path);
                continue;
            }
            let hunks = diff::diff_to_hunks(&fd.old, &fd.new, 0);
            let mut insertions = 0usize;
            let mut deletions = 0usize;
            for hunk in &hunks {
                for line in &hunk.lines {
                    match line {
                        diff::HunkLine::Addition(_) => insertions += 1,
                        diff::HunkLine::Deletion(_) => deletions += 1,
                        diff::HunkLine::Context(_) => {}
                    }
                }
            }
            total_insertions += insertions;
            total_deletions += deletions;
            let changes = insertions + deletions;
            let bar: String = std::iter::repeat_n('+', insertions.min(40))
                .chain(std::iter::repeat_n('-', deletions.min(40)))
                .collect();
            println!(" {} | {changes} {bar}", fd.path);
        }

        let file_count = file_diffs.len();
        println!(
            " {} file{} changed, {} insertion{}(+), {} deletion{}(-)",
            file_count,
            if file_count == 1 { "" } else { "s" },
            total_insertions,
            if total_insertions == 1 { "" } else { "s" },
            total_deletions,
            if total_deletions == 1 { "" } else { "s" },
        );
        return;
    }

    // Normal diff output.
    for fd in file_diffs {
        print_file_diff(&fd.path, &fd.old, &fd.new);
    }
}

fn print_file_diff(path: &str, old: &[u8], new: &[u8]) {
    if diff::is_binary(old) || diff::is_binary(new) {
        println!("Binary file {path} differs");
        return;
    }

    let hunks = diff::diff_to_hunks(old, new, 3);
    if hunks.is_empty() {
        return;
    }

    output::print_diff_header(&format!("a/{path}"), &format!("b/{path}"));
    for hunk in &hunks {
        output::print_diff_hunk(hunk);
    }
}
