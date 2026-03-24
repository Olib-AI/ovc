//! `ovc stash` — Save and restore index state.

use anyhow::{Context, Result};

use crate::app::{StashAction, StashArgs};
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &StashArgs) -> Result<()> {
    match &args.action {
        None | Some(StashAction::Push { .. }) => {
            let message = match &args.action {
                Some(StashAction::Push { message }) => message.as_str(),
                _ => "WIP",
            };
            execute_push(ctx, message)
        }
        Some(StashAction::Pop { index }) => execute_pop(ctx, *index),
        Some(StashAction::Apply { index }) => execute_apply(ctx, *index),
        Some(StashAction::Drop { index }) => execute_drop(ctx, *index),
        Some(StashAction::List) => execute_list(ctx),
        Some(StashAction::Clear) => execute_clear(ctx),
    }
}

fn execute_push(ctx: &CliContext, message: &str) -> Result<()> {
    let (mut repo, workdir) = ctx.open_repo()?;

    repo.stash_push(message).context("failed to push stash")?;

    // Restore the working directory to match the HEAD tree so that files on
    // disk reflect the clean index state. This mirrors `git stash` semantics
    // where both the index and working tree are reset after stashing.
    if let Ok(head_oid) = repo.ref_store().resolve_head()
        && let Ok(Some(ovc_core::object::Object::Commit(head_commit))) = repo.get_object(&head_oid)
    {
        let head_tree_oid = head_commit.tree;
        // Collect HEAD entries.
        let mut head_index = ovc_core::index::Index::new();
        head_index
            .read_tree(&head_tree_oid, repo.object_store())
            .context("failed to read HEAD tree")?;
        let head_paths: std::collections::BTreeSet<String> = head_index
            .entries()
            .iter()
            .map(|e| e.path.clone())
            .collect();

        // Write HEAD files to working directory.
        for entry in head_index.entries() {
            if let Ok(Some(ovc_core::object::Object::Blob(data))) = repo.get_object(&entry.oid) {
                workdir.write_file(&entry.path, &data, entry.mode)?;
            }
        }

        // Remove files that exist on disk but not in HEAD.
        let ignore = CliContext::load_ignore(&workdir);
        if let Ok(disk_files) = workdir.scan_files(&ignore) {
            for f in &disk_files {
                if !head_paths.contains(&f.path) {
                    workdir.delete_file(&f.path)?;
                }
            }
        }
    }

    repo.save().context("failed to save repository")?;

    output::print_success(&format!("Saved working directory: {message}"));
    Ok(())
}

fn execute_pop(ctx: &CliContext, idx: usize) -> Result<()> {
    let (mut repo, workdir) = ctx.open_repo()?;

    repo.stash_pop(idx).context("failed to pop stash")?;

    // Write the restored index entries to the working directory so that
    // the on-disk files match the stashed state.
    for entry in repo.index().entries().to_vec() {
        if let Ok(Some(ovc_core::object::Object::Blob(data))) = repo.get_object(&entry.oid) {
            workdir.write_file(&entry.path, &data, entry.mode)?;
        }
    }

    repo.save().context("failed to save repository")?;

    output::print_success(&format!("Popped stash@{{{idx}}}"));
    Ok(())
}

fn execute_apply(ctx: &CliContext, idx: usize) -> Result<()> {
    let (mut repo, workdir) = ctx.open_repo()?;

    repo.stash()
        .list()
        .get(idx)
        .ok_or_else(|| anyhow::anyhow!("stash index {idx} out of range"))?;

    let store = repo.object_store();
    let mut index = repo.index().clone();
    repo.stash()
        .apply(idx, store, &mut index)
        .context("failed to apply stash")?;

    // Write back the index.
    *repo.index_mut() = index;

    // Write the restored index entries to the working directory so that
    // the on-disk files match the stashed state.
    for entry in repo.index().entries().to_vec() {
        if let Ok(Some(ovc_core::object::Object::Blob(data))) = repo.get_object(&entry.oid) {
            workdir.write_file(&entry.path, &data, entry.mode)?;
        }
    }

    repo.save().context("failed to save repository")?;

    output::print_success(&format!("Applied stash@{{{idx}}}"));
    Ok(())
}

fn execute_drop(ctx: &CliContext, idx: usize) -> Result<()> {
    let (mut repo, _workdir) = ctx.open_repo()?;

    let entry = repo
        .stash_mut()
        .drop_entry(idx)
        .context("failed to drop stash")?;
    repo.save().context("failed to save repository")?;

    output::print_success(&format!("Dropped stash@{{{idx}}}: {}", entry.message));
    Ok(())
}

fn execute_list(ctx: &CliContext) -> Result<()> {
    let (repo, _workdir) = ctx.open_repo()?;

    let entries = repo.stash().list();
    if entries.is_empty() {
        println!("No stash entries.");
        return Ok(());
    }

    for (i, entry) in entries.iter().enumerate() {
        let ts = chrono::DateTime::from_timestamp(entry.timestamp, 0);
        let time_str = ts.map_or_else(
            || "unknown".to_owned(),
            |dt| dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        );
        let base_hex = entry.base_commit_id.to_string();
        let short_base = &base_hex[..12.min(base_hex.len())];
        println!(
            "stash@{{{i}}}: On {short_base} ({time_str}): {}",
            entry.message
        );
    }

    Ok(())
}

fn execute_clear(ctx: &CliContext) -> Result<()> {
    let (mut repo, _workdir) = ctx.open_repo()?;

    repo.stash_mut().clear();
    repo.save().context("failed to save repository")?;

    output::print_success("Cleared all stash entries");
    Ok(())
}
