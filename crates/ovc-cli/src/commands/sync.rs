//! `ovc sync` — Merge remote changes from the `.ovc` file and save local state.
//!
//! When multiple users share a single `.ovc` file (e.g., via iCloud), each
//! working on their own branch, `ovc sync` imports remote branches, tags,
//! objects, and notes into the local repository state and writes the merged
//! result back to disk.

use anyhow::{Context, Result};

use crate::app::SyncArgs;
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, _args: &SyncArgs) -> Result<()> {
    let password = CliContext::get_password()?;
    let (mut repo, _workdir) = ctx.open_repo()?;

    repo.save_with_merge(password.as_bytes())
        .context("sync failed")?;

    output::print_success("Synced with remote changes.");
    Ok(())
}
