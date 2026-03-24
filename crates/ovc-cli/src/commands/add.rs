//! `ovc add` — Stage files for the next commit.

use anyhow::{Context, Result};

use ovc_core::object::FileMode;

use crate::app::AddArgs;
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &AddArgs) -> Result<()> {
    let (mut repo, workdir) = ctx.open_repo()?;
    let ignore = CliContext::load_ignore(&workdir);
    let mut staged_count = 0u64;

    if args.all {
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

        for entry in &status {
            use ovc_core::workdir::FileStatus;
            let should_stage = matches!(entry.unstaged, FileStatus::Modified | FileStatus::Deleted)
                || matches!(entry.staged, FileStatus::Untracked);

            if !should_stage {
                continue;
            }

            if entry.unstaged == FileStatus::Deleted || entry.staged == FileStatus::Deleted {
                repo.index_mut().unstage_file(&entry.path);
            } else {
                let content = workdir
                    .read_file(&entry.path)
                    .with_context(|| format!("failed to read: {}", entry.path))?;
                let (index, store) = repo.index_and_store_mut();
                index
                    .stage_file(&entry.path, &content, FileMode::Regular, store)
                    .with_context(|| format!("failed to stage: {}", entry.path))?;
            }
            staged_count += 1;
        }
    } else {
        for path in &args.paths {
            if !args.force && ignore.is_ignored(path) {
                output::print_warning(&format!("skipping ignored file: {path}"));
                continue;
            }

            let abs_path = workdir.root().join(path);
            if abs_path.is_dir() {
                let scan_ignore = if args.force {
                    ovc_core::ignore::IgnoreRules::empty()
                } else {
                    ignore.clone()
                };
                let sub_workdir = ovc_core::workdir::WorkDir::new(workdir.root().to_path_buf());
                let entries = sub_workdir.scan_files(&scan_ignore)?;
                // Determine whether the directory covers the entire workdir root.
                let is_root = path == "."
                    || path == "./"
                    || abs_path
                        .canonicalize()
                        .ok()
                        .zip(workdir.root().canonicalize().ok())
                        .is_some_and(|(a, b)| a == b);
                for entry in &entries {
                    if is_root
                        || entry.path.starts_with(path)
                        || entry.path.starts_with(&format!("{path}/"))
                    {
                        let content = workdir.read_file(&entry.path)?;
                        // Skip files whose content hasn't changed from the index.
                        let content_hash = ovc_core::id::hash_blob(&content);
                        if repo
                            .index()
                            .get_entry(&entry.path)
                            .is_some_and(|e| e.oid == content_hash)
                        {
                            continue;
                        }
                        let (index, store) = repo.index_and_store_mut();
                        index.stage_file(&entry.path, &content, FileMode::Regular, store)?;
                        staged_count += 1;
                    }
                }
            } else {
                let content = workdir
                    .read_file(path)
                    .with_context(|| format!("failed to read: {path}"))?;
                let (index, store) = repo.index_and_store_mut();
                index
                    .stage_file(path, &content, FileMode::Regular, store)
                    .with_context(|| format!("failed to stage: {path}"))?;
                staged_count += 1;
            }
        }
    }

    repo.save().context("failed to save repository")?;
    output::print_success(&format!("staged {staged_count} file(s)"));
    Ok(())
}
