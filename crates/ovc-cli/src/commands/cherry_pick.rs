//! `ovc cherry-pick` — Apply a commit's changes onto HEAD.

use anyhow::{Context, Result};

use crate::app::CherryPickArgs;
use crate::context::{self, CliContext};
use crate::output;

pub fn execute(ctx: &CliContext, args: &CherryPickArgs) -> Result<()> {
    let (mut repo, workdir) = ctx.open_repo()?;

    let commit_oid = context::resolve_commit(&args.commit, &repo)?;

    let new_oid = repo
        .cherry_pick_commit(&commit_oid)
        .context("cherry-pick failed")?;

    // Update the working directory to reflect the cherry-picked changes.
    // Rebuild the index from the new commit's tree and write files.
    if let Some(ovc_core::object::Object::Commit(new_commit)) = repo.get_object(&new_oid)? {
        let tree_oid = new_commit.tree;
        let mut temp_index = ovc_core::index::Index::new();
        temp_index.read_tree(&tree_oid, repo.object_store())?;
        let entries_data: Vec<_> = temp_index
            .entries()
            .iter()
            .map(|e| (e.path.clone(), e.oid, e.mode))
            .collect();
        for (path, oid, mode) in &entries_data {
            if let Some(ovc_core::object::Object::Blob(data)) = repo.get_object(oid)? {
                workdir.write_file(path, &data, *mode)?;
            }
        }
    }

    repo.save().context("failed to save repository")?;

    let old_hex = commit_oid.to_string();
    let new_hex = new_oid.to_string();
    output::print_success(&format!(
        "Cherry-picked {} -> {}",
        &old_hex[..12.min(old_hex.len())],
        &new_hex[..12.min(new_hex.len())]
    ));

    Ok(())
}
