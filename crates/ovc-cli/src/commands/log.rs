//! `ovc log` — Show commit history.

use std::collections::BTreeSet;

use anyhow::Result;

use ovc_core::id::ObjectId;
use ovc_core::keys::{OvcPublicKey, verify_commit};
use ovc_core::object::Object;
use ovc_core::refs::RefTarget;

use crate::app::LogArgs;
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &LogArgs) -> Result<()> {
    let (repo, _workdir) = ctx.open_repo()?;

    let mut start_oids: Vec<ObjectId> = Vec::new();

    if args.all {
        for (_name, oid) in repo.ref_store().list_branches() {
            start_oids.push(*oid);
        }
        if start_oids.is_empty() {
            println!("no commits yet");
            return Ok(());
        }
    } else if let Ok(oid) = repo.ref_store().resolve_head() {
        start_oids.push(oid);
    } else {
        println!("no commits yet");
        return Ok(());
    }

    // Load authorized keys for signature verification.
    let authorized_keys: Vec<OvcPublicKey> = load_authorized_keys(&repo);

    let mut ref_decorations: std::collections::BTreeMap<ObjectId, Vec<String>> =
        std::collections::BTreeMap::new();

    let current_branch = match repo.ref_store().head() {
        RefTarget::Symbolic(ref_name) => ref_name
            .strip_prefix("refs/heads/")
            .map(std::borrow::ToOwned::to_owned),
        RefTarget::Direct(_) => None,
    };

    for (name, oid) in repo.ref_store().list_branches() {
        let label = if current_branch.as_deref() == Some(name) {
            format!("HEAD -> {name}")
        } else {
            name.to_owned()
        };
        ref_decorations.entry(*oid).or_default().push(label);
    }
    for (name, oid, _msg) in repo.ref_store().list_tags() {
        ref_decorations
            .entry(*oid)
            .or_default()
            .push(format!("tag: {name}"));
    }

    let mut visited = BTreeSet::new();
    let mut queue = std::collections::VecDeque::new();
    for oid in &start_oids {
        if visited.insert(*oid) {
            queue.push_back(*oid);
        }
    }

    let max = args.max_count.unwrap_or(usize::MAX);
    let mut shown = 0usize;

    // Track active graph columns for --graph mode.
    let mut graph_columns: Vec<ObjectId> = Vec::new();

    while let Some(oid) = queue.pop_front() {
        if shown >= max {
            break;
        }

        let Some(Object::Commit(commit)) = repo.get_object(&oid)? else {
            continue;
        };

        let refs: Vec<&str> = ref_decorations
            .get(&oid)
            .map(|v| v.iter().map(String::as_str).collect())
            .unwrap_or_default();

        let sig_status = if commit.signature.is_some() || args.show_signatures {
            Some(verify_commit(&commit, &authorized_keys))
        } else {
            None
        };

        let graph_prefix = if args.graph {
            build_graph_prefix(&oid, &commit, &mut graph_columns)
        } else {
            String::new()
        };

        if args.graph {
            print!("{graph_prefix}");
        }
        if args.oneline {
            output::print_commit_oneline(&oid, &commit.message, &refs);
            if let Some(ref status) = sig_status {
                output::print_signature_inline(status);
            }
        } else {
            output::print_commit_with_signature(
                &oid,
                &commit.message,
                &commit.author,
                &refs,
                sig_status.as_ref(),
                args.show_signatures,
            );
        }

        shown += 1;

        for parent_oid in &commit.parents {
            if visited.insert(*parent_oid) {
                queue.push_back(*parent_oid);
            }
        }
    }

    Ok(())
}

/// Builds an ASCII graph prefix for a commit line.
///
/// Manages `graph_columns` to track active branches being drawn. Each column
/// represents a branch line being tracked. The current commit is shown with `*`,
/// merge points with `|\` and fork points with `|/`.
fn build_graph_prefix(
    oid: &ObjectId,
    commit: &ovc_core::object::Commit,
    graph_columns: &mut Vec<ObjectId>,
) -> String {
    use std::fmt::Write;

    use console::Style;

    let red = Style::new().red();

    // Find which column this commit belongs to, or add a new one.
    let col_idx = graph_columns
        .iter()
        .position(|c| c == oid)
        .unwrap_or_else(|| {
            graph_columns.push(*oid);
            graph_columns.len() - 1
        });

    let mut prefix = String::new();

    // Draw pipes for columns before this one.
    for i in 0..graph_columns.len() {
        if i == col_idx {
            let _ = write!(prefix, "{} ", red.apply_to("*"));
        } else {
            prefix.push_str("| ");
        }
    }

    let is_merge = commit.parents.len() > 1;

    // Update columns: replace this column with first parent,
    // add additional parents as new columns.
    if commit.parents.is_empty() {
        // Root commit — remove column.
        graph_columns.remove(col_idx);
    } else {
        // Replace column with first parent.
        graph_columns[col_idx] = commit.parents[0];

        // For merge commits, add additional parents.
        if is_merge {
            for parent in &commit.parents[1..] {
                if !graph_columns.contains(parent) {
                    graph_columns.push(*parent);
                }
            }
            // Append a merge indicator.
            prefix.push_str("  ");
        }
    }

    prefix
}

/// Loads authorized public keys from the repository's key slots.
///
/// For each key slot fingerprint, attempts to find the corresponding `.pub`
/// file in `~/.ssh/ovc/`. Falls back to an empty list if no keys are found.
fn load_authorized_keys(repo: &ovc_core::repository::Repository) -> Vec<OvcPublicKey> {
    let fingerprints = repo.list_keys();
    if fingerprints.is_empty() {
        // No key slots — try loading all local keys as a fallback.
        return ovc_core::keys::list_keys()
            .ok()
            .map(|keys| {
                keys.into_iter()
                    .filter_map(|(_name, _fp, path)| OvcPublicKey::load(&path).ok())
                    .collect()
            })
            .unwrap_or_default();
    }

    fingerprints
        .into_iter()
        .filter_map(|fp| {
            ovc_core::keys::find_key(fp)
                .ok()
                .flatten()
                .and_then(|path| OvcPublicKey::load(&path).ok())
        })
        .collect()
}
