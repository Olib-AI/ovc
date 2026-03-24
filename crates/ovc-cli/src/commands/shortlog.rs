//! `ovc shortlog` — Summarize commits by author.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anyhow::{Context, Result};

use ovc_core::object::Object;

use crate::app::ShortlogArgs;
use crate::context::CliContext;

pub fn execute(ctx: &CliContext, args: &ShortlogArgs) -> Result<()> {
    let (repo, _workdir) = ctx.open_repo()?;

    let head_oid = repo.ref_store().resolve_head().context("no commits yet")?;

    // Walk all commits from HEAD.
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(head_oid);
    visited.insert(head_oid);

    let mut by_author: BTreeMap<String, Vec<String>> = BTreeMap::new();

    while let Some(oid) = queue.pop_front() {
        let Some(Object::Commit(commit)) = repo.get_object(&oid)? else {
            continue;
        };

        by_author
            .entry(commit.author.name.clone())
            .or_default()
            .push(commit.message.lines().next().unwrap_or("").to_owned());

        for parent in &commit.parents {
            if visited.insert(*parent) {
                queue.push_back(*parent);
            }
        }
    }

    // Collect and optionally sort by count.
    let mut entries: Vec<(String, Vec<String>)> = by_author.into_iter().collect();
    if args.sort_by_count {
        entries.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    }

    for (author, messages) in &entries {
        if args.summary {
            println!("{:>6}\t{author}", messages.len());
        } else {
            println!("{author} ({}):", messages.len());
            for msg in messages {
                println!("      {msg}");
            }
            println!();
        }
    }

    Ok(())
}
