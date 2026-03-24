//! `ovc reset [--soft|--mixed|--hard] [commit]` — Reset HEAD.

use anyhow::{Context, Result, bail};

use ovc_core::object::Object;

use crate::app::ResetArgs;
use crate::context::{self, CliContext};
use crate::output;

pub fn execute(ctx: &CliContext, args: &ResetArgs) -> Result<()> {
    let (mut repo, workdir) = ctx.open_repo()?;

    // Handle `ovc reset -- <paths>`: unstage specific files.
    if !args.paths.is_empty() {
        return unstage_paths(&mut repo, &args.paths);
    }

    // Resolve the target commit.
    let target_oid = if let Some(ref spec) = args.commit {
        context::resolve_commit(spec, &repo)?
    } else {
        // Default: HEAD~1 (parent of current HEAD).
        let head = repo
            .ref_store()
            .resolve_head()
            .context("cannot reset: no commits yet")?;
        let head_obj = repo
            .get_object(&head)?
            .ok_or_else(|| anyhow::anyhow!("HEAD commit not found"))?;
        match head_obj {
            Object::Commit(c) => {
                if c.parents.is_empty() {
                    bail!("HEAD has no parent; specify a target commit explicitly");
                }
                c.parents[0]
            }
            _ => bail!("HEAD does not point to a commit"),
        }
    };

    // Verify the target is a commit.
    let target_obj = repo
        .get_object(&target_oid)?
        .ok_or_else(|| anyhow::anyhow!("target commit not found: {target_oid}"))?;
    let Object::Commit(commit) = target_obj else {
        bail!("target is not a commit: {target_oid}")
    };

    // Determine the current branch name.
    let branch = match repo.ref_store().head() {
        ovc_core::refs::RefTarget::Symbolic(ref_name) => ref_name
            .strip_prefix("refs/heads/")
            .unwrap_or(ref_name)
            .to_owned(),
        ovc_core::refs::RefTarget::Direct(_) => bail!("HEAD is detached; cannot reset"),
    };

    let author = CliContext::resolve_author(None, &repo)?;

    // Move the branch ref.
    repo.ref_store_mut().set_branch(
        &branch,
        target_oid,
        &author,
        &format!("reset: moving to {}", &target_oid.to_string()[..12]),
    )?;

    if !args.soft {
        // Reset index to target tree.
        // Use index_and_store_mut to avoid overlapping borrows.
        let tree_oid = commit.tree;
        let (index, store) = repo.index_and_store_mut();
        index
            .read_tree(&tree_oid, store)
            .context("failed to rebuild index from target tree")?;
    }

    if args.hard {
        // Reset working directory.
        let ignore = CliContext::load_ignore(&workdir);
        let workdir_files = workdir.scan_files(&ignore)?;

        // Delete files not in the target tree.
        let mut target_index = ovc_core::index::Index::new();
        target_index.read_tree(&commit.tree, repo.object_store())?;
        let target_paths: std::collections::BTreeSet<&str> = target_index
            .entries()
            .iter()
            .map(|e| e.path.as_str())
            .collect();

        for wf in &workdir_files {
            if !target_paths.contains(wf.path.as_str()) {
                workdir.delete_file(&wf.path)?;
            }
        }

        // Write all files from target tree.
        for entry in target_index.entries() {
            if let Some(Object::Blob(data)) = repo.get_object(&entry.oid)? {
                workdir.write_file(&entry.path, &data, entry.mode)?;
            }
        }
    }

    repo.save().context("failed to save repository")?;

    let mode = if args.soft {
        "soft"
    } else if args.hard {
        "hard"
    } else {
        "mixed"
    };

    let short = &target_oid.to_string()[..12];
    output::print_success(&format!("HEAD is now at {short} ({mode} reset)"));

    Ok(())
}

/// Unstage specific files by restoring their index entries to match HEAD's tree.
///
/// If a file does not exist in HEAD, it is removed from the index entirely.
/// This is equivalent to `git reset HEAD -- <file>`.
fn unstage_paths(repo: &mut ovc_core::repository::Repository, paths: &[String]) -> Result<()> {
    let head_tree_oid = repo
        .ref_store()
        .resolve_head()
        .ok()
        .and_then(|oid| repo.get_object(&oid).ok().flatten())
        .and_then(|obj| match obj {
            Object::Commit(c) => Some(c.tree),
            _ => None,
        });

    // Build HEAD index if we have a HEAD tree.
    let head_index = if let Some(ref tree_oid) = head_tree_oid {
        let mut idx = ovc_core::index::Index::new();
        idx.read_tree(tree_oid, repo.object_store())
            .context("failed to read HEAD tree")?;
        Some(idx)
    } else {
        None
    };

    for path in paths {
        let head_entry = head_index.as_ref().and_then(|idx| idx.get_entry(path));

        if let Some(entry) = head_entry {
            // Restore to HEAD version: re-stage the HEAD blob into the index.
            let blob = repo
                .get_object(&entry.oid)?
                .ok_or_else(|| anyhow::anyhow!("blob not found for '{path}'"))?;
            let Object::Blob(data) = blob else {
                bail!("object for '{path}' is not a blob");
            };
            let mode = entry.mode;
            let (index, store) = repo.index_and_store_mut();
            index
                .stage_file(path, &data, mode, store)
                .with_context(|| format!("failed to restore index entry for '{path}'"))?;
        } else {
            // File does not exist in HEAD — remove from index.
            repo.index_mut().unstage_file(path);
        }

        output::print_success(&format!("unstaged '{path}'"));
    }

    repo.save().context("failed to save repository")?;

    Ok(())
}
