//! `ovc clean [-f] [-n]` — Remove untracked files.

use anyhow::{Context, Result, bail};
use console::Style;

use ovc_core::object::Object;
use ovc_core::workdir::FileStatus;

use crate::app::CleanArgs;
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &CleanArgs) -> Result<()> {
    let (repo, workdir) = ctx.open_repo()?;
    let ignore = CliContext::load_ignore(&workdir);

    let head_tree = repo
        .ref_store()
        .resolve_head()
        .ok()
        .and_then(|oid| repo.get_object(&oid).ok().flatten())
        .and_then(|obj| match obj {
            Object::Commit(c) => Some(c.tree),
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

    let untracked: Vec<&str> = status
        .iter()
        .filter(|s| s.staged == FileStatus::Untracked)
        .map(|s| s.path.as_str())
        .collect();

    if untracked.is_empty() {
        println!("nothing to clean");
        return Ok(());
    }

    if args.dry_run {
        let cyan = Style::new().cyan();
        println!("Would remove:");
        for path in &untracked {
            println!("  {}", cyan.apply_to(path));
        }
        return Ok(());
    }

    if !args.force {
        bail!("refusing to clean without -f (force). Use -n to preview what would be removed.");
    }

    let mut removed = 0usize;
    for path in &untracked {
        workdir.delete_file(path)?;
        removed += 1;
    }

    output::print_success(&format!("removed {removed} untracked file(s)"));

    Ok(())
}
