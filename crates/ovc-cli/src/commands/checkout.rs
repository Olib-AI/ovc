//! `ovc checkout` — Switch branches or restore working tree files.

use anyhow::{Context, Result, bail};

use ovc_core::workdir::FileStatus;

use crate::app::CheckoutArgs;
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &CheckoutArgs) -> Result<()> {
    let (mut repo, workdir) = ctx.open_repo()?;

    // Handle `ovc checkout -- <paths>`: restore files from HEAD.
    if !args.paths.is_empty() {
        return restore_paths(&mut repo, &workdir, &args.paths);
    }

    // Handle `ovc checkout -b <name> [<start-point>]`: create and switch to a new branch.
    // When no start-point is provided the new branch is rooted at HEAD.
    if let Some(ref new_branch) = args.new_branch {
        if let Some(start_point) = args.target.as_deref() {
            // Checkout the start-point first so HEAD moves there, then create the branch.
            repo.checkout_branch(start_point, &workdir)
                .with_context(|| format!("failed to checkout start-point '{start_point}'"))?;
        }
        // HEAD is now the desired start-point (either explicitly switched or already there).
        repo.create_branch(new_branch)
            .with_context(|| format!("failed to create branch '{new_branch}'"))?;
        repo.checkout_branch(new_branch, &workdir)
            .with_context(|| format!("failed to checkout branch '{new_branch}'"))?;
        repo.save()?;
        output::print_success(&format!("switched to new branch '{new_branch}'"));
        return Ok(());
    }

    let target = args
        .target
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("branch name or -- <paths> required"))?;

    let ignore = CliContext::load_ignore(&workdir);
    let head_tree = repo
        .ref_store()
        .resolve_head()
        .ok()
        .and_then(|oid| repo.get_object(&oid).ok().flatten())
        .and_then(|obj| match obj {
            ovc_core::object::Object::Commit(c) => Some(c.tree),
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

    let has_uncommitted = status.iter().any(|s| {
        matches!(s.unstaged, FileStatus::Modified | FileStatus::Deleted)
            || matches!(
                s.staged,
                FileStatus::Added | FileStatus::Modified | FileStatus::Deleted
            )
    });

    if has_uncommitted && !args.force {
        bail!("you have uncommitted changes; commit or stash them before switching branches");
    }

    repo.checkout_branch(target, &workdir)
        .with_context(|| format!("failed to checkout '{target}'"))?;
    repo.save()?;

    output::print_success(&format!("switched to branch '{target}'"));
    Ok(())
}

/// Restores specific file paths from HEAD, writing blob content to disk
/// and updating the staging index to match HEAD.
fn restore_paths(
    repo: &mut ovc_core::repository::Repository,
    workdir: &ovc_core::workdir::WorkDir,
    paths: &[String],
) -> Result<()> {
    use ovc_core::object::Object;

    let head_oid = repo
        .ref_store()
        .resolve_head()
        .context("cannot restore: no commits yet")?;
    let head_obj = repo
        .get_object(&head_oid)?
        .ok_or_else(|| anyhow::anyhow!("HEAD commit not found"))?;
    let Object::Commit(head_commit) = head_obj else {
        bail!("HEAD does not point to a commit");
    };

    // Build an index from HEAD's tree to look up file entries.
    let mut head_index = ovc_core::index::Index::new();
    head_index
        .read_tree(&head_commit.tree, repo.object_store())
        .context("failed to read HEAD tree")?;

    for path in paths {
        let head_entry = head_index
            .get_entry(path)
            .ok_or_else(|| anyhow::anyhow!("path '{path}' not found in HEAD"))?;

        let blob_oid = head_entry.oid;
        let mode = head_entry.mode;

        let blob_obj = repo
            .get_object(&blob_oid)?
            .ok_or_else(|| anyhow::anyhow!("blob not found for '{path}'"))?;
        let Object::Blob(data) = blob_obj else {
            bail!("object for '{path}' is not a blob");
        };

        // Write to working directory.
        workdir
            .write_file(path, &data, mode)
            .with_context(|| format!("failed to write '{path}'"))?;

        // Update the staging index to match HEAD.
        let (index, store) = repo.index_and_store_mut();
        index
            .stage_file(path, &data, mode, store)
            .with_context(|| format!("failed to update index for '{path}'"))?;
    }

    repo.save().context("failed to save repository")?;

    for path in paths {
        output::print_success(&format!("restored '{path}' from HEAD"));
    }

    Ok(())
}
