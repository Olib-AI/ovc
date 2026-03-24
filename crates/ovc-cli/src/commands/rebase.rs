//! `ovc rebase` — Rebase current branch onto another.

use anyhow::{Context, Result};

use ovc_core::rebase::RebaseError;
use ovc_core::refs::RefTarget;

use crate::app::RebaseCliArgs;
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &RebaseCliArgs) -> Result<()> {
    let (mut repo, workdir) = ctx.open_repo()?;

    // Determine current branch.
    let current_branch = match repo.ref_store().head() {
        RefTarget::Symbolic(ref_name) => ref_name
            .strip_prefix("refs/heads/")
            .unwrap_or(ref_name)
            .to_owned(),
        RefTarget::Direct(_) => {
            anyhow::bail!("cannot rebase from detached HEAD");
        }
    };

    match repo.rebase_branch(&current_branch, &args.onto) {
        Ok(result) => {
            for (old, new) in &result.replayed {
                let old_hex = old.to_string();
                let new_hex = new.to_string();
                println!(
                    "  {} -> {}",
                    &old_hex[..12.min(old_hex.len())],
                    &new_hex[..12.min(new_hex.len())]
                );
            }
            // Update the working directory to reflect the rebased branch.
            repo.checkout_branch(&current_branch, &workdir)
                .context("failed to update working directory after rebase")?;
            repo.save().context("failed to save repository")?;
            output::print_success(&format!(
                "Rebased {} commits onto {}",
                result.replayed.len(),
                args.onto
            ));
        }
        Err(RebaseError::Conflict {
            commit,
            conflicts,
            completed,
        }) => {
            if !completed.is_empty() {
                println!(
                    "Successfully replayed {} commits before conflict:",
                    completed.len()
                );
                for (old, new) in &completed {
                    let old_hex = old.to_string();
                    let new_hex = new.to_string();
                    println!(
                        "  {} -> {}",
                        &old_hex[..12.min(old_hex.len())],
                        &new_hex[..12.min(new_hex.len())]
                    );
                }
            }
            let commit_hex = commit.to_string();
            output::print_error(&format!(
                "Conflict while replaying {}",
                &commit_hex[..12.min(commit_hex.len())]
            ));
            for path in &conflicts {
                eprintln!("  conflict: {path}");
            }
            anyhow::bail!("rebase aborted due to conflicts");
        }
        Err(RebaseError::NoCommonAncestor) => {
            anyhow::bail!(
                "no common ancestor between '{}' and '{}'",
                current_branch,
                args.onto
            );
        }
        Err(RebaseError::MergeCommitInChain { commit }) => {
            anyhow::bail!(
                "cannot rebase: commit {commit} is a merge commit; use 'ovc merge' instead"
            );
        }
        Err(RebaseError::BaseNotReachable) => {
            anyhow::bail!(
                "cannot rebase: '{}' is not reachable from '{}' via first-parent chain",
                args.onto,
                current_branch
            );
        }
        Err(RebaseError::Core(e)) => {
            return Err(e).context("rebase failed");
        }
    }

    Ok(())
}
