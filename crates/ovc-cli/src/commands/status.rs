//! `ovc status` — Show the working tree status.

use anyhow::{Context, Result};
use console::Style;

use ovc_core::workdir::FileStatus;

use crate::app::StatusArgs;
use crate::context::CliContext;

pub fn execute(ctx: &CliContext, args: &StatusArgs) -> Result<()> {
    let (repo, workdir) = ctx.open_repo()?;
    let ignore = CliContext::load_ignore(&workdir);

    let branch_name = match repo.ref_store().head() {
        ovc_core::refs::RefTarget::Symbolic(ref_name) => ref_name
            .strip_prefix("refs/heads/")
            .unwrap_or(ref_name)
            .to_owned(),
        ovc_core::refs::RefTarget::Direct(oid) => {
            let hex = oid.to_string();
            format!("detached at {}", &hex[..12])
        }
    };

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

    if args.short {
        for entry in &status {
            let x = short_code(entry.staged);
            let y = short_code(entry.unstaged);
            if x != ' ' || y != ' ' {
                println!("{x}{y} {}", entry.path);
            }
        }
        return Ok(());
    }

    println!("On branch {branch_name}");

    let staged: Vec<_> = status
        .iter()
        .filter(|s| {
            matches!(
                s.staged,
                FileStatus::Added | FileStatus::Modified | FileStatus::Deleted
            )
        })
        .collect();

    let unstaged: Vec<_> = status
        .iter()
        .filter(|s| matches!(s.unstaged, FileStatus::Modified | FileStatus::Deleted))
        .collect();

    let untracked: Vec<_> = status
        .iter()
        .filter(|s| s.staged == FileStatus::Untracked)
        .collect();

    let green = Style::new().green();
    let red = Style::new().red();

    if staged.is_empty() && unstaged.is_empty() && untracked.is_empty() {
        println!("nothing to commit, working tree clean");
        return Ok(());
    }

    if !staged.is_empty() {
        println!();
        println!("Changes to be committed:");
        for entry in &staged {
            let label = match entry.staged {
                FileStatus::Added => "new file:   ",
                FileStatus::Modified => "modified:   ",
                FileStatus::Deleted => "deleted:    ",
                _ => "            ",
            };
            println!("    {}", green.apply_to(format!("{label}{}", entry.path)));
        }
    }

    if !unstaged.is_empty() {
        println!();
        println!("Changes not staged for commit:");
        for entry in &unstaged {
            let label = match entry.unstaged {
                FileStatus::Modified => "modified:   ",
                FileStatus::Deleted => "deleted:    ",
                _ => "            ",
            };
            println!("    {}", red.apply_to(format!("{label}{}", entry.path)));
        }
    }

    if !untracked.is_empty() {
        println!();
        println!("Untracked files:");
        for entry in &untracked {
            println!("    {}", red.apply_to(&entry.path));
        }
    }

    println!();
    Ok(())
}

const fn short_code(s: FileStatus) -> char {
    match s {
        FileStatus::Unmodified => ' ',
        FileStatus::Modified => 'M',
        FileStatus::Added => 'A',
        FileStatus::Deleted => 'D',
        FileStatus::Untracked => '?',
        FileStatus::Ignored => '!',
    }
}
