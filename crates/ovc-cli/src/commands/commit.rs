//! `ovc commit` — Record staged changes as a new commit.

use anyhow::{Context, Result, bail};
use dialoguer;

use ovc_core::object::FileMode;
use ovc_core::workdir::FileStatus;

use crate::app::CommitArgs;
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &CommitArgs) -> Result<()> {
    let (mut repo, workdir) = ctx.open_repo()?;

    if args.all {
        stage_all_modified(&mut repo, &workdir)?;
    }

    // Handle --amend: merge the old commit's tree entries into the current index.
    if args.amend {
        return execute_amend(ctx, args, repo, workdir);
    }

    let message = args
        .message
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("commit message is required (use -m)"))?;

    let head_tree = repo
        .ref_store()
        .resolve_head()
        .ok()
        .and_then(|oid| repo.get_object(&oid).ok().flatten())
        .and_then(|obj| match obj {
            ovc_core::object::Object::Commit(c) => Some(c.tree),
            _ => None,
        });
    let ignore = CliContext::load_ignore(&workdir);
    let status = workdir.compute_status(
        repo.index(),
        head_tree.as_ref(),
        repo.object_store(),
        &ignore,
    )?;
    let has_staged = status.iter().any(|s| {
        matches!(
            s.staged,
            FileStatus::Added | FileStatus::Modified | FileStatus::Deleted
        )
    });

    if !has_staged {
        bail!("nothing to commit (no staged changes)");
    }

    if !args.no_verify {
        run_pre_commit(&status, &workdir)?;
    }

    let author = CliContext::resolve_author(args.author.as_deref(), &repo)?;

    let should_sign =
        args.sign || std::env::var("OVC_SIGN_COMMITS").is_ok_and(|v| v == "true" || v == "1");

    let commit_oid = if should_sign {
        let keypair = load_signing_key()?;
        repo.create_commit_signed(message, &author, &keypair)
            .context("failed to create signed commit")?
    } else {
        repo.create_commit(message, &author)
            .context("failed to create commit")?
    };

    match repo.save() {
        Ok(()) => {}
        Err(ovc_core::error::CoreError::ConflictDetected { reason }) => {
            output::print_warning(&format!(
                "Repository was modified by another user while you were working.\n\
                 {reason}\n\n\
                 Attempting to merge remote changes and save..."
            ));
            let password = CliContext::get_password()?;
            repo.save_with_merge(password.as_bytes())
                .context("failed to merge and save after conflict")?;
            output::print_success("Merged remote changes and saved successfully.");
        }
        Err(e) => return Err(e).context("failed to save repository"),
    }

    let branch_name = match repo.ref_store().head() {
        ovc_core::refs::RefTarget::Symbolic(ref_name) => ref_name
            .strip_prefix("refs/heads/")
            .unwrap_or(ref_name)
            .to_owned(),
        ovc_core::refs::RefTarget::Direct(_) => "detached HEAD".to_owned(),
    };

    let hex = commit_oid.to_string();
    let short = &hex[..12];
    output::print_success(&format!("[{branch_name} {short}] {message}"));

    // Run post-commit hooks (non-blocking — warn but don't fail the commit).
    if !args.no_verify {
        run_post_commit(&workdir);
    }

    Ok(())
}

fn execute_amend(
    _ctx: &CliContext,
    args: &CommitArgs,
    mut repo: ovc_core::repository::Repository,
    _workdir: ovc_core::workdir::WorkDir,
) -> Result<()> {
    // Get the current HEAD commit.
    let head_oid = repo
        .ref_store()
        .resolve_head()
        .context("cannot amend: no commits yet")?;
    let head_obj = repo
        .get_object(&head_oid)?
        .ok_or_else(|| anyhow::anyhow!("HEAD commit not found"))?;
    let ovc_core::object::Object::Commit(old_commit) = head_obj else {
        bail!("HEAD does not point to a commit");
    };

    // Use the new message if provided, otherwise keep the old one.
    let message = args.message.as_deref().unwrap_or(&old_commit.message);

    // The current index already reflects HEAD's tree plus any newly staged
    // changes. For a message-only amend the index matches HEAD's tree, which
    // is correct. For an amend with new staged files, they are already in the
    // index from `ovc add`.

    let author = CliContext::resolve_author(args.author.as_deref(), &repo)?;

    // Build tree from the current index.
    let (index, store) = repo.index_and_store_mut();
    let tree_oid = index.write_tree(store)?;

    // Create the amended commit with the old commit's parents (not HEAD as parent).
    let new_commit = ovc_core::object::Commit {
        tree: tree_oid,
        parents: old_commit.parents.clone(),
        author: author.clone(),
        committer: author.clone(),
        message: message.to_owned(),
        signature: None,
        sequence: old_commit.sequence,
    };

    let commit_oid = repo
        .insert_object(&ovc_core::object::Object::Commit(new_commit))
        .context("failed to create amended commit")?;

    // Update the branch ref to point to the new commit (replacing HEAD).
    match repo.ref_store().head().clone() {
        ovc_core::refs::RefTarget::Symbolic(ref_name) => {
            let branch_name = ref_name
                .strip_prefix("refs/heads/")
                .unwrap_or(&ref_name)
                .to_owned();
            repo.ref_store_mut().set_branch(
                &branch_name,
                commit_oid,
                &author,
                &format!("commit (amend): {message}"),
            )?;
        }
        ovc_core::refs::RefTarget::Direct(_) => {
            repo.ref_store_mut()
                .set_head(ovc_core::refs::RefTarget::Direct(commit_oid));
        }
    }

    match repo.save() {
        Ok(()) => {}
        Err(ovc_core::error::CoreError::ConflictDetected { reason }) => {
            output::print_warning(&format!(
                "Repository was modified by another user while you were working.\n\
                 {reason}\n\n\
                 Attempting to merge remote changes and save..."
            ));
            let password = CliContext::get_password()?;
            repo.save_with_merge(password.as_bytes())
                .context("failed to merge and save after conflict")?;
            output::print_success("Merged remote changes and saved successfully.");
        }
        Err(e) => return Err(e).context("failed to save repository"),
    }

    let branch_name = match repo.ref_store().head() {
        ovc_core::refs::RefTarget::Symbolic(ref_name) => ref_name
            .strip_prefix("refs/heads/")
            .unwrap_or(ref_name)
            .to_owned(),
        ovc_core::refs::RefTarget::Direct(_) => "detached HEAD".to_owned(),
    };

    let hex = commit_oid.to_string();
    let short = &hex[..12];
    output::print_success(&format!("[{branch_name} {short}] (amend) {message}"));

    Ok(())
}

fn stage_all_modified(
    repo: &mut ovc_core::repository::Repository,
    workdir: &ovc_core::workdir::WorkDir,
) -> Result<()> {
    let ignore = CliContext::load_ignore(workdir);
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

    let modified_paths: Vec<String> = status
        .iter()
        .filter(|s| s.unstaged == FileStatus::Modified)
        .map(|s| s.path.clone())
        .collect();
    let deleted_paths: Vec<String> = status
        .iter()
        .filter(|s| s.unstaged == FileStatus::Deleted)
        .map(|s| s.path.clone())
        .collect();

    for path in &modified_paths {
        let content = workdir.read_file(path)?;
        let (index, store) = repo.index_and_store_mut();
        index.stage_file(path, &content, FileMode::Regular, store)?;
    }
    for path in &deleted_paths {
        repo.index_mut().unstage_file(path);
    }

    Ok(())
}

fn load_signing_key() -> Result<ovc_core::keys::OvcKeyPair> {
    let key_query = std::env::var("OVC_KEY").unwrap_or_else(|_| "default".to_owned());

    let pub_path = ovc_core::keys::find_key(&key_query)
        .context("failed to search for signing key")?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no key found matching '{key_query}'; generate one with 'ovc key generate' \
                 or set OVC_KEY to the key name/fingerprint"
            )
        })?;

    let priv_path = ovc_core::keys::private_key_path_for(&pub_path);
    if !priv_path.exists() {
        bail!(
            "private key not found at {} (public key at {})",
            priv_path.display(),
            pub_path.display()
        );
    }

    let passphrase = if let Ok(pw) = std::env::var("OVC_KEY_PASSPHRASE") {
        pw
    } else {
        dialoguer::Password::new()
            .with_prompt("Key passphrase (for signing)")
            .interact()
            .context("failed to read key passphrase")?
    };

    ovc_core::keys::OvcKeyPair::load_private(&priv_path, passphrase.as_bytes())
        .context("failed to load signing key")
}

fn run_pre_commit(
    status: &[ovc_core::workdir::StatusEntry],
    workdir: &ovc_core::workdir::WorkDir,
) -> Result<()> {
    let staged_paths: Vec<String> = status
        .iter()
        .filter(|s| {
            matches!(
                s.staged,
                FileStatus::Added | FileStatus::Modified | FileStatus::Deleted
            )
        })
        .map(|s| s.path.clone())
        .collect();

    let hook_results = match ovc_actions::hooks::run_pre_commit_hooks(workdir.root(), &staged_paths)
    {
        Ok(results) => results,
        Err(e) => {
            output::print_warning(&format!("pre-commit hooks config error: {e}"));
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
            bail!("pre-commit hooks failed; use --no-verify to bypass");
        }
    }

    Ok(())
}

fn run_post_commit(workdir: &ovc_core::workdir::WorkDir) {
    let hook_results = match ovc_actions::hooks::run_post_commit_hooks(workdir.root(), &[]) {
        Ok(results) => results,
        Err(e) => {
            output::print_warning(&format!("post-commit hooks config error: {e}"));
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
        output::print_warning(&format!("[post-commit] [{label}] {}", r.display_name));
    }
}
