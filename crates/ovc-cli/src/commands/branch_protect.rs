//! `ovc branch-protect` — Manage branch protection rules.

use anyhow::{Context, Result};

use ovc_core::access::{AccessRole, BranchProtection};

use crate::app::BranchProtectArgs;
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &BranchProtectArgs) -> Result<()> {
    if args.remove {
        return remove(ctx, &args.branch);
    }

    let (mut repo, _workdir) = ctx.open_repo()?;

    let protection = BranchProtection {
        required_approvals: args.required_approvals,
        require_ci_pass: args.require_ci,
        allowed_merge_roles: vec![AccessRole::Admin, AccessRole::Owner],
        allowed_push_roles: vec![AccessRole::Owner],
    };

    repo.set_branch_protection(&args.branch, protection)
        .context("failed to set branch protection")?;
    repo.save().context("failed to save repository")?;

    output::print_success(&format!(
        "Protected branch '{}': {} required approvals, CI required: {}",
        args.branch, args.required_approvals, args.require_ci
    ));

    Ok(())
}

fn remove(ctx: &CliContext, branch: &str) -> Result<()> {
    let (mut repo, _workdir) = ctx.open_repo()?;

    repo.remove_branch_protection(branch)
        .context("failed to remove branch protection")?;
    repo.save().context("failed to save repository")?;

    output::print_success(&format!("Removed protection from branch '{branch}'"));

    Ok(())
}
