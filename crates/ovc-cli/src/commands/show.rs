//! `ovc show [commit]` — Display commit contents.

use anyhow::{Context, Result};
use console::Style;

use ovc_core::diff;
use ovc_core::id::ObjectId;
use ovc_core::index::Index;
use ovc_core::object::Object;

use crate::app::ShowArgs;
use crate::context::{self, CliContext};
use crate::output;

pub fn execute(ctx: &CliContext, args: &ShowArgs) -> Result<()> {
    let (repo, _workdir) = ctx.open_repo()?;

    let oid = if let Some(ref spec) = args.commit {
        context::resolve_commit(spec, &repo)?
    } else {
        repo.ref_store().resolve_head().context("no commits yet")?
    };

    let obj = repo
        .get_object(&oid)?
        .ok_or_else(|| anyhow::anyhow!("object not found: {oid}"))?;

    let commit = match obj {
        Object::Commit(c) => c,
        Object::Tag(t) => {
            let yellow = Style::new().yellow();
            println!("tag {}", yellow.apply_to(&t.tag_name));
            println!("Tagger: {} <{}>", t.tagger.name, t.tagger.email);
            let dt = chrono::DateTime::from_timestamp(t.tagger.timestamp, 0);
            if let Some(d) = dt {
                println!("Date:   {}", d.format("%a %b %d %H:%M:%S %Y %z"));
            }
            println!();
            for line in t.message.lines() {
                println!("    {line}");
            }
            return Ok(());
        }
        Object::Blob(data) => {
            let text = String::from_utf8_lossy(&data);
            print!("{text}");
            return Ok(());
        }
        Object::Tree(tree) => {
            for entry in &tree.entries {
                let name = String::from_utf8_lossy(&entry.name);
                println!("{:?}\t{}\t{name}", entry.mode, entry.oid);
            }
            return Ok(());
        }
    };

    // Print commit header.
    output::print_commit_with_signature(&oid, &commit.message, &commit.author, &[], None, false);

    // Print diff against parent.
    if commit.parents.is_empty() {
        // Initial commit: diff against empty tree.
        show_tree_diff(&[], commit.tree, &repo)?;
    } else {
        let parent_oid = commit.parents[0];
        if let Some(Object::Commit(parent)) = repo.get_object(&parent_oid)? {
            show_tree_diff_between(parent.tree, commit.tree, &repo)?;
        }
    }

    Ok(())
}

/// Shows diff for an initial commit (all files are new).
fn show_tree_diff(
    _empty: &[()],
    tree_oid: ObjectId,
    repo: &ovc_core::repository::Repository,
) -> Result<()> {
    let mut index = Index::new();
    index.read_tree(&tree_oid, repo.object_store())?;

    for entry in index.entries() {
        if let Some(Object::Blob(data)) = repo.get_object(&entry.oid)? {
            if diff::is_binary(&data) {
                println!("Binary file {} added", entry.path);
                continue;
            }
            let hunks = diff::diff_to_hunks(b"", &data, 3);
            if !hunks.is_empty() {
                output::print_diff_header(
                    &format!("a/{}", entry.path),
                    &format!("b/{}", entry.path),
                );
                for hunk in &hunks {
                    output::print_diff_hunk(hunk);
                }
            }
        }
    }

    Ok(())
}

/// Shows diff between two trees.
fn show_tree_diff_between(
    old_tree: ObjectId,
    new_tree: ObjectId,
    repo: &ovc_core::repository::Repository,
) -> Result<()> {
    let mut old_index = Index::new();
    old_index.read_tree(&old_tree, repo.object_store())?;

    let mut new_index = Index::new();
    new_index.read_tree(&new_tree, repo.object_store())?;

    let old_entries: std::collections::BTreeMap<&str, &ovc_core::index::IndexEntry> = old_index
        .entries()
        .iter()
        .map(|e| (e.path.as_str(), e))
        .collect();
    let new_entries: std::collections::BTreeMap<&str, &ovc_core::index::IndexEntry> = new_index
        .entries()
        .iter()
        .map(|e| (e.path.as_str(), e))
        .collect();

    // Modified and added files.
    for (path, new_entry) in &new_entries {
        let old_content = old_entries.get(path).and_then(|old_entry| {
            if old_entry.oid == new_entry.oid {
                return None; // unchanged
            }
            repo.get_object(&old_entry.oid)
                .ok()
                .flatten()
                .and_then(|obj| match obj {
                    Object::Blob(data) => Some(data),
                    _ => None,
                })
        });

        let new_content =
            repo.get_object(&new_entry.oid)
                .ok()
                .flatten()
                .and_then(|obj| match obj {
                    Object::Blob(data) => Some(data),
                    _ => None,
                });

        // Skip unchanged files.
        if old_entries
            .get(path)
            .is_some_and(|old| old.oid == new_entry.oid)
        {
            continue;
        }

        let old_data = old_content.unwrap_or_default();
        let new_data = new_content.unwrap_or_default();

        if diff::is_binary(&old_data) || diff::is_binary(&new_data) {
            println!("Binary file {path} changed");
            continue;
        }

        let hunks = diff::diff_to_hunks(&old_data, &new_data, 3);
        if !hunks.is_empty() {
            output::print_diff_header(&format!("a/{path}"), &format!("b/{path}"));
            for hunk in &hunks {
                output::print_diff_hunk(hunk);
            }
        }
    }

    // Deleted files.
    for (path, old_entry) in &old_entries {
        if !new_entries.contains_key(path) {
            let old_data = repo
                .get_object(&old_entry.oid)
                .ok()
                .flatten()
                .and_then(|obj| match obj {
                    Object::Blob(data) => Some(data),
                    _ => None,
                })
                .unwrap_or_default();

            if diff::is_binary(&old_data) {
                println!("Binary file {path} deleted");
                continue;
            }

            let hunks = diff::diff_to_hunks(&old_data, b"", 3);
            if !hunks.is_empty() {
                output::print_diff_header(&format!("a/{path}"), "/dev/null");
                for hunk in &hunks {
                    output::print_diff_hunk(hunk);
                }
            }
        }
    }

    Ok(())
}
