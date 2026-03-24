//! `ovc git-import` — Import a Git repository into OVC format.

use anyhow::{Context, Result};

use crate::app::GitImportArgs;
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &GitImportArgs) -> Result<()> {
    let git_repo_path = if args.git_repo.is_absolute() {
        args.git_repo.clone()
    } else {
        ctx.cwd.join(&args.git_repo)
    };

    if !git_repo_path.exists() {
        anyhow::bail!(
            "git repository path does not exist: {}",
            git_repo_path.display()
        );
    }

    let ovc_path = resolve_output_path(ctx, args, &git_repo_path)?;

    if ovc_path.exists() {
        anyhow::bail!("output path already exists: {}", ovc_path.display());
    }

    output::print_success(&format!("Importing from {} ...", git_repo_path.display()));

    // Prefer key-based encryption when OVC_KEY is set; fall back to password.
    let result = if let Ok(key_query) = std::env::var("OVC_KEY") {
        let pub_path = ovc_core::keys::find_key(&key_query)
            .context("failed to search for key")?
            .ok_or_else(|| anyhow::anyhow!("no key found matching '{key_query}'"))?;

        let priv_path = ovc_core::keys::private_key_path_for(&pub_path);
        if !priv_path.exists() {
            anyhow::bail!(
                "private key not found at {} (public key found at {})",
                priv_path.display(),
                pub_path.display()
            );
        }

        let passphrase = if let Ok(pw) = std::env::var("OVC_KEY_PASSPHRASE") {
            pw
        } else {
            dialoguer::Password::new()
                .with_prompt("Key passphrase")
                .interact()
                .context("failed to read key passphrase")?
        };

        let keypair = ovc_core::keys::OvcKeyPair::load_private(&priv_path, passphrase.as_bytes())
            .context("failed to load private key")?;

        ovc_git::import::import_git_repo_with_key(&git_repo_path, &ovc_path, &keypair)
            .context("failed to import git repository")?
    } else {
        let password = if let Ok(pw) = std::env::var("OVC_PASSWORD") {
            pw
        } else {
            dialoguer::Password::new()
                .with_prompt("Set encryption password for OVC repository")
                .with_confirmation("Confirm password", "Passwords do not match")
                .interact()
                .context("failed to read password")?
        };

        ovc_git::import::import_git_repo(&git_repo_path, &ovc_path, password.as_bytes())
            .context("failed to import git repository")?
    };

    output::print_success(&format!(
        "Import complete: {} blobs, {} trees, {} commits, {} tags, {} refs",
        result.blobs_imported,
        result.trees_imported,
        result.commits_imported,
        result.tags_imported,
        result.refs_imported,
    ));
    output::print_success(&format!("OVC repository saved to {}", ovc_path.display()));

    Ok(())
}

fn resolve_output_path(
    ctx: &CliContext,
    args: &GitImportArgs,
    git_repo_path: &std::path::Path,
) -> Result<std::path::PathBuf> {
    args.output.as_ref().map_or_else(
        || {
            let repo_name = git_repo_path.file_name().map_or_else(
                || "imported".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            );
            Ok(ctx.cwd.join(format!("{repo_name}.ovc")))
        },
        |out| {
            if out.is_absolute() {
                Ok(out.clone())
            } else {
                Ok(ctx.cwd.join(out))
            }
        },
    )
}
