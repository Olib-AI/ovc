//! `ovc grep <pattern>` — Search file contents in the repository tree.

use anyhow::{Context, Result};
use console::Style;

use ovc_core::grep;
use ovc_core::object::Object;

use crate::app::GrepArgs;
use crate::context::CliContext;

pub fn execute(ctx: &CliContext, args: &GrepArgs) -> Result<()> {
    let (repo, _workdir) = ctx.open_repo()?;

    let head_oid = repo.ref_store().resolve_head().context("no commits yet")?;

    let head_obj = repo
        .get_object(&head_oid)?
        .ok_or_else(|| anyhow::anyhow!("HEAD commit not found"))?;

    let tree_oid = match head_obj {
        Object::Commit(c) => c.tree,
        _ => anyhow::bail!("HEAD does not point to a commit"),
    };

    let matches = grep::grep_tree(
        &args.pattern,
        &tree_oid,
        repo.object_store(),
        args.case_insensitive,
    )
    .context("grep failed")?;

    if matches.is_empty() {
        return Ok(());
    }

    let magenta = Style::new().magenta();
    let green = Style::new().green();
    let red = Style::new().red().bold();

    if args.count {
        // Group by file and show count.
        let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for m in &matches {
            *counts.entry(m.path.as_str()).or_insert(0) += 1;
        }
        for (path, count) in &counts {
            println!("{}:{}", magenta.apply_to(path), count);
        }
    } else {
        let re_pattern = if args.case_insensitive {
            format!("(?i){}", &args.pattern)
        } else {
            args.pattern.clone()
        };
        let re = regex::Regex::new(&re_pattern).ok();

        for m in &matches {
            let highlighted = re.as_ref().map_or_else(
                || m.line.clone(),
                |r| {
                    r.replace_all(&m.line, |caps: &regex::Captures<'_>| {
                        format!("{}", red.apply_to(&caps[0]))
                    })
                    .into_owned()
                },
            );

            println!(
                "{}:{}:{}",
                magenta.apply_to(&m.path),
                green.apply_to(m.line_number),
                highlighted,
            );
        }
    }

    Ok(())
}
