//! `ovc describe [commit]` — Find the nearest tag ancestor.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anyhow::{Context, Result};

use ovc_core::id::ObjectId;
use ovc_core::object::Object;

use crate::app::DescribeArgs;
use crate::context::{self, CliContext};

pub fn execute(ctx: &CliContext, args: &DescribeArgs) -> Result<()> {
    let (repo, _workdir) = ctx.open_repo()?;

    let start_oid = if let Some(ref spec) = args.commit {
        context::resolve_commit(spec, &repo)?
    } else {
        repo.ref_store().resolve_head().context("no commits yet")?
    };

    // Build tag -> commit_oid mapping.
    // Tags can point to Tag objects (annotated) or directly to commits (lightweight).
    // Dereference Tag objects to their target commit.
    let mut tags: BTreeMap<ObjectId, String> = BTreeMap::new();
    for (name, oid, _msg) in repo.ref_store().list_tags() {
        let commit_oid = match repo.get_object(oid)? {
            Some(Object::Tag(tag_obj)) => tag_obj.target,
            _ => *oid,
        };
        tags.insert(commit_oid, name.to_owned());
    }

    if tags.is_empty() {
        anyhow::bail!("no tags found; cannot describe");
    }

    // If the commit itself is tagged, just print the tag name.
    if let Some(tag_name) = tags.get(&start_oid) {
        println!("{tag_name}");
        return Ok(());
    }

    // BFS through ancestors to find the nearest tagged commit.
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::new();
    queue.push_back((start_oid, 0usize));
    visited.insert(start_oid);

    let max_depth = 10_000;

    while let Some((oid, depth)) = queue.pop_front() {
        if depth > max_depth {
            break;
        }

        let Some(Object::Commit(commit)) = repo.get_object(&oid)? else {
            continue;
        };

        for parent_oid in &commit.parents {
            if !visited.insert(*parent_oid) {
                continue;
            }

            let parent_depth = depth + 1;

            if let Some(tag_name) = tags.get(parent_oid) {
                let short = &start_oid.to_string()[..12];
                println!("{tag_name}-{parent_depth}-g{short}");
                return Ok(());
            }

            queue.push_back((*parent_oid, parent_depth));
        }
    }

    anyhow::bail!("no tag found in ancestry of commit {start_oid}");
}
