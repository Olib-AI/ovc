//! `ovc merge` — Merge a branch into the current branch.

use anyhow::{Result, bail};

use ovc_core::merge;
use ovc_core::object::Object;
use ovc_core::refs::RefTarget;

use crate::app::MergeArgs;
use crate::context::CliContext;
use crate::output;

#[allow(clippy::too_many_lines)]
pub fn execute(ctx: &CliContext, args: &MergeArgs) -> Result<()> {
    let (mut repo, workdir) = ctx.open_repo()?;

    // Run pre-merge hooks unless --no-verify.
    if !args.no_verify {
        run_pre_merge(workdir.root())?;
    }

    // Check branch protection rules on the target (current) branch.
    if !args.no_verify {
        let current_branch = match repo.ref_store().head() {
            RefTarget::Symbolic(s) => s.strip_prefix("refs/heads/").unwrap_or(s).to_owned(),
            RefTarget::Direct(_) => String::new(),
        };
        if !current_branch.is_empty() {
            let violations =
                ovc_actions::hooks::check_branch_protection(workdir.root(), &current_branch)
                    .unwrap_or_default();
            if !violations.is_empty() {
                for v in &violations {
                    output::print_warning(&format!("[branch protection] {v}"));
                }
                bail!(
                    "branch '{}' is protected; {} violation(s). Use --no-verify to bypass.",
                    current_branch,
                    violations.len()
                );
            }
        }
    }

    let our_oid = repo
        .ref_store()
        .resolve_head()
        .map_err(|_| anyhow::anyhow!("cannot merge: HEAD has no commits"))?;

    let Some(Object::Commit(our_commit)) = repo.get_object(&our_oid)? else {
        bail!("HEAD does not point to a commit");
    };

    let their_ref = format!("refs/heads/{}", args.branch);
    let their_oid = repo
        .ref_store()
        .resolve(&their_ref)
        .map_err(|_| anyhow::anyhow!("branch '{}' not found", args.branch))?;

    let Some(Object::Commit(their_commit)) = repo.get_object(&their_oid)? else {
        bail!("target branch does not point to a commit");
    };

    if our_oid == their_oid {
        println!("Already up to date.");
        return Ok(());
    }

    let base_oid = find_merge_base(&repo, our_oid, their_oid)?;

    let Some(Object::Commit(base_commit)) = repo.get_object(&base_oid)? else {
        bail!("merge base is not a valid commit");
    };

    let result = merge::merge_trees(
        &base_commit.tree,
        &our_commit.tree,
        &their_commit.tree,
        repo.object_store_mut(),
    )
    .map_err(|e| anyhow::anyhow!("tree merge failed: {e}"))?;

    if result.conflicts.is_empty() {
        let tree = ovc_core::object::Tree {
            entries: result.entries,
        };
        let tree_oid = repo.insert_object(&Object::Tree(tree))?;

        // Populate the index from the merged tree by collecting entries first,
        // then staging them (avoids simultaneous &mut and & borrows on repo).
        let entries_data: Vec<_> = {
            let mut temp_index = ovc_core::index::Index::new();
            temp_index.read_tree(&tree_oid, repo.object_store())?;
            temp_index
                .entries()
                .iter()
                .map(|e| (e.path.clone(), e.oid, e.mode))
                .collect()
        };
        repo.index_mut().clear();
        for (path, oid, mode) in &entries_data {
            let content = repo
                .get_object(oid)?
                .and_then(|o| match o {
                    Object::Blob(data) => Some(data),
                    _ => None,
                })
                .unwrap_or_default();
            // Write merged content to the working directory.
            workdir.write_file(path, &content, *mode)?;
            let (idx, st) = repo.index_and_store_mut();
            idx.stage_file(path, &content, *mode, st)?;
        }

        let author = CliContext::resolve_author(None, &repo)?;
        let message = format!("Merge branch '{}' into current branch", args.branch);

        let merge_commit = ovc_core::object::Commit {
            tree: tree_oid,
            parents: vec![our_oid, their_oid],
            author: author.clone(),
            committer: author,
            message: message.clone(),
            signature: None,
            sequence: our_commit.sequence.max(their_commit.sequence) + 1,
        };

        let merge_oid = repo.insert_object(&Object::Commit(merge_commit))?;

        match repo.ref_store().head().clone() {
            RefTarget::Symbolic(ref_name) => {
                let branch_name = ref_name
                    .strip_prefix("refs/heads/")
                    .unwrap_or(&ref_name)
                    .to_owned();
                let id = ovc_core::object::Identity {
                    name: String::new(),
                    email: String::new(),
                    timestamp: 0,
                    tz_offset_minutes: 0,
                };
                repo.ref_store_mut()
                    .set_branch(&branch_name, merge_oid, &id, &message)?;
            }
            RefTarget::Direct(_) => {
                repo.ref_store_mut().set_head(RefTarget::Direct(merge_oid));
            }
        }

        repo.save()?;

        let hex = merge_oid.to_string();
        output::print_success(&format!("merge commit {}", &hex[..12]));

        // Run post-merge hooks (non-blocking — warn but don't fail).
        if !args.no_verify {
            run_post_merge(workdir.root());
        }
    } else {
        // Write conflict markers to working directory files.
        for conflict in &result.conflicts {
            if let merge::TreeConflictKind::ModifyModify { ref content } = conflict.kind {
                workdir.write_file(&conflict.path, content, ovc_core::object::FileMode::Regular)?;
            }
        }

        output::print_warning("merge conflicts detected:");
        for conflict in &result.conflicts {
            let kind = match &conflict.kind {
                merge::TreeConflictKind::ModifyModify { .. } => "modify/modify",
                merge::TreeConflictKind::ModifyDelete { .. } => "modify/delete",
                merge::TreeConflictKind::AddAdd => "add/add",
            };
            println!("  {kind}: {}", conflict.path);
        }
        println!();
        println!("Fix the conflicts and commit the result.");

        bail!("automatic merge failed; fix conflicts and then commit the result");
    }

    Ok(())
}

fn find_merge_base(
    repo: &ovc_core::repository::Repository,
    oid_a: ovc_core::id::ObjectId,
    oid_b: ovc_core::id::ObjectId,
) -> Result<ovc_core::id::ObjectId> {
    use std::collections::BTreeSet;

    let mut ancestors_a = BTreeSet::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(oid_a);
    ancestors_a.insert(oid_a);

    while let Some(oid) = queue.pop_front() {
        if let Some(Object::Commit(c)) = repo.get_object(&oid)? {
            for parent in &c.parents {
                if ancestors_a.insert(*parent) {
                    queue.push_back(*parent);
                }
            }
        }
    }

    let mut visited = BTreeSet::new();
    queue.push_back(oid_b);
    visited.insert(oid_b);

    while let Some(oid) = queue.pop_front() {
        if ancestors_a.contains(&oid) {
            return Ok(oid);
        }
        if let Some(Object::Commit(c)) = repo.get_object(&oid)? {
            for parent in &c.parents {
                if visited.insert(*parent) {
                    queue.push_back(*parent);
                }
            }
        }
    }

    bail!("no common ancestor found between the two branches");
}

fn run_pre_merge(repo_root: &std::path::Path) -> Result<()> {
    let hook_results = match ovc_actions::hooks::run_pre_merge_hooks(repo_root, &[]) {
        Ok(results) => results,
        Err(e) => {
            output::print_warning(&format!("pre-merge hooks config error: {e}"));
            Vec::new()
        }
    };

    if !hook_results.is_empty() {
        for r in &hook_results {
            let label = match r.status {
                ovc_actions::runner::ActionStatus::Passed => "pass",
                ovc_actions::runner::ActionStatus::Failed => "FAIL",
                ovc_actions::runner::ActionStatus::Skipped => "skip",
                ovc_actions::runner::ActionStatus::TimedOut => "TIME",
                ovc_actions::runner::ActionStatus::Error => "ERR ",
            };
            output::print_warning(&format!("[{label}] {}", r.display_name));
        }

        if ovc_actions::hooks::has_blocking_failures(&hook_results) {
            bail!("pre-merge hooks failed; use --no-verify to bypass");
        }
    }

    Ok(())
}

fn run_post_merge(repo_root: &std::path::Path) {
    let hook_results = match ovc_actions::hooks::run_post_merge_hooks(repo_root, &[]) {
        Ok(results) => results,
        Err(e) => {
            output::print_warning(&format!("post-merge hooks config error: {e}"));
            return;
        }
    };

    for r in &hook_results {
        let label = match r.status {
            ovc_actions::runner::ActionStatus::Passed => "pass",
            ovc_actions::runner::ActionStatus::Failed => "FAIL",
            ovc_actions::runner::ActionStatus::Skipped => "skip",
            ovc_actions::runner::ActionStatus::TimedOut => "TIME",
            ovc_actions::runner::ActionStatus::Error => "ERR ",
        };
        output::print_warning(&format!("[post-merge] [{label}] {}", r.display_name));
    }
}
