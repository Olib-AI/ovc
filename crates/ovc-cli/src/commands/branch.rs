//! `ovc branch` — Manage branches.

use std::collections::HashSet;

use anyhow::{Context, Result};
use console::Style;

use ovc_core::id::ObjectId;
use ovc_core::object::Object;
use ovc_core::refs::RefTarget;
use ovc_core::repository::Repository;

use crate::app::BranchArgs;
use crate::context::{self, CliContext};
use crate::output;

pub fn execute(ctx: &CliContext, args: &BranchArgs) -> Result<()> {
    let (mut repo, _workdir) = ctx.open_repo()?;

    // Rename (-m / --move): `ovc branch -m old-name new-name`.
    if let Some(ref names) = args.rename {
        // clap ensures exactly 2 values due to `num_args = 2`.
        let old_name = &names[0];
        let new_name = &names[1];
        repo.rename_branch(old_name, new_name)
            .with_context(|| format!("failed to rename branch '{old_name}' to '{new_name}'"))?;
        repo.save()?;
        output::print_success(&format!("renamed branch '{old_name}' to '{new_name}'"));
        return Ok(());
    }

    // Force delete (-D): skip the merge check.
    if let Some(ref name) = args.force_delete {
        repo.delete_branch(name)
            .with_context(|| format!("failed to force-delete branch '{name}'"))?;
        repo.save()?;
        output::print_success(&format!("deleted branch '{name}' (force)"));
        return Ok(());
    }

    if let Some(ref name) = args.delete {
        // Safety check: ensure the branch is fully merged into the current HEAD.
        let branch_ref = if name.starts_with("refs/heads/") {
            name.clone()
        } else {
            format!("refs/heads/{name}")
        };
        if let Ok(branch_tip) = repo.ref_store().resolve(&branch_ref)
            && let Ok(head_oid) = repo.ref_store().resolve_head()
            && !is_ancestor(branch_tip, head_oid, &repo)
        {
            anyhow::bail!(
                "branch '{name}' is not fully merged into HEAD.\n\
                 If you are sure you want to delete it, use 'ovc branch -D {name}',\n\
                 or merge first."
            );
        }
        repo.delete_branch(name)
            .with_context(|| format!("failed to delete branch '{name}'"))?;
        repo.save()?;
        output::print_success(&format!("deleted branch '{name}'"));
        return Ok(());
    }

    if let Some(ref name) = args.name
        && !args.list
    {
        if let Some(ref start_spec) = args.start_point {
            // Create branch at an explicit commit.
            let oid = context::resolve_commit(start_spec, &repo)
                .with_context(|| format!("cannot resolve start point '{start_spec}'"))?;
            repo.create_branch_at(name, oid)
                .with_context(|| format!("failed to create branch '{name}' at '{start_spec}'"))?;
            let hex = oid.to_string();
            output::print_success(&format!(
                "created branch '{}' at {}",
                name,
                &hex[..12.min(hex.len())]
            ));
        } else {
            repo.create_branch(name)
                .with_context(|| format!("failed to create branch '{name}'"))?;
            output::print_success(&format!("created branch '{name}'"));
        }
        repo.save()?;
        return Ok(());
    }

    let current_branch = match repo.ref_store().head() {
        RefTarget::Symbolic(ref_name) => ref_name
            .strip_prefix("refs/heads/")
            .map(std::borrow::ToOwned::to_owned),
        RefTarget::Direct(_) => None,
    };

    let branches = repo.ref_store().list_branches();

    if branches.is_empty() {
        let default = &repo.config().default_branch;
        let green = Style::new().green().bold();
        println!("* {}", green.apply_to(default));
    } else {
        let green = Style::new().green().bold();
        for (name, _oid) in &branches {
            let is_current = current_branch.as_deref() == Some(*name);
            if is_current {
                println!("* {}", green.apply_to(name));
            } else {
                println!("  {name}");
            }
        }
    }

    Ok(())
}

/// Check if `ancestor` is reachable from `descendant` by walking parents (BFS).
fn is_ancestor(ancestor: ObjectId, descendant: ObjectId, repo: &Repository) -> bool {
    if ancestor == descendant {
        return true;
    }
    let mut visited = HashSet::new();
    let mut queue = vec![descendant];
    while let Some(oid) = queue.pop() {
        if oid == ancestor {
            return true;
        }
        if !visited.insert(oid) {
            continue;
        }
        if let Ok(Some(Object::Commit(c))) = repo.get_object(&oid) {
            queue.extend_from_slice(&c.parents);
        }
    }
    false
}
