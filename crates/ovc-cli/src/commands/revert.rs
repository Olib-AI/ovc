//! `ovc revert <commit>` — Create a new commit that undoes a previous commit's changes.

use anyhow::{Context, Result};

use ovc_core::object::Object;

use crate::app::RevertArgs;
use crate::context::{self, CliContext};
use crate::output;

pub fn execute(ctx: &CliContext, args: &RevertArgs) -> Result<()> {
    let (mut repo, workdir) = ctx.open_repo()?;

    // Set the author on the repo config so revert_commit picks it up.
    let author = CliContext::resolve_author(None, &repo)?;
    {
        let config = repo.config_mut();
        config.user_name.clone_from(&author.name);
        config.user_email.clone_from(&author.email);
    }

    let commit_oid = context::resolve_commit(&args.commit, &repo)?;

    let new_oid = repo.revert_commit(&commit_oid).context("revert failed")?;

    // Update the working directory to match the reverted tree.
    // We need to both write new/changed files AND delete files that
    // were removed by the revert.
    if let Some(Object::Commit(new_commit)) = repo.get_object(&new_oid)? {
        let tree_oid = new_commit.tree;
        let mut new_index = ovc_core::index::Index::new();
        new_index.read_tree(&tree_oid, repo.object_store())?;

        let new_paths: std::collections::BTreeSet<String> =
            new_index.entries().iter().map(|e| e.path.clone()).collect();

        // Delete files that exist on disk but are not in the new tree.
        let ignore = CliContext::load_ignore(&workdir);
        let disk_files = workdir.scan_files(&ignore)?;
        for wf in &disk_files {
            if !new_paths.contains(&wf.path) {
                workdir.delete_file(&wf.path)?;
            }
        }

        // Write all files from the new tree.
        for entry in new_index.entries() {
            if let Some(Object::Blob(data)) = repo.get_object(&entry.oid)? {
                workdir.write_file(&entry.path, &data, entry.mode)?;
            }
        }
    }

    repo.save().context("failed to save repository")?;

    let old_hex = commit_oid.to_string();
    let new_hex = new_oid.to_string();
    output::print_success(&format!(
        "Reverted {} -> {}",
        &old_hex[..12.min(old_hex.len())],
        &new_hex[..12.min(new_hex.len())]
    ));

    Ok(())
}
