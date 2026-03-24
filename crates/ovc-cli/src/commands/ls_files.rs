//! `ovc ls-files` — List tracked files.

use anyhow::{Context, Result};

use ovc_core::object::Object;
use ovc_core::workdir::FileStatus;

use crate::app::LsFilesArgs;
use crate::context::CliContext;

pub fn execute(ctx: &CliContext, args: &LsFilesArgs) -> Result<()> {
    let (repo, workdir) = ctx.open_repo()?;

    if args.modified || args.deleted || args.untracked {
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

        for entry in &status {
            if args.modified
                && (entry.unstaged == FileStatus::Modified || entry.staged == FileStatus::Modified)
            {
                println!("{}", entry.path);
            }
            if args.deleted
                && (entry.unstaged == FileStatus::Deleted || entry.staged == FileStatus::Deleted)
            {
                println!("{}", entry.path);
            }
            if args.untracked && entry.staged == FileStatus::Untracked {
                println!("{}", entry.path);
            }
        }
    } else {
        // Default: show staged files (index entries).
        for entry in repo.index().entries() {
            println!("{}", entry.path);
        }
    }

    Ok(())
}
