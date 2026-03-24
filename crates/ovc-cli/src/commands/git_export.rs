//! `ovc git-export` — Export an OVC repository to Git format.

use anyhow::{Context, Result};

use crate::app::GitExportArgs;
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &GitExportArgs) -> Result<()> {
    let ovc_path = if args.ovc_file.is_absolute() {
        args.ovc_file.clone()
    } else {
        ctx.cwd.join(&args.ovc_file)
    };

    if !ovc_path.exists() {
        anyhow::bail!("OVC repository file does not exist: {}", ovc_path.display());
    }

    let output_dir = resolve_output_dir(ctx, args, &ovc_path)?;

    if output_dir.join(".git").exists() {
        anyhow::bail!(
            "output directory already contains a git repository: {}",
            output_dir.display()
        );
    }

    std::fs::create_dir_all(&output_dir).with_context(|| {
        format!(
            "failed to create output directory: {}",
            output_dir.display()
        )
    })?;

    output::print_success(&format!(
        "Exporting {} to {} ...",
        ovc_path.display(),
        output_dir.display()
    ));

    // Prefer key-based decryption when OVC_KEY is set; fall back to password.
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

        ovc_git::export::export_to_git_with_key(&ovc_path, &output_dir, &keypair)
            .context("failed to export OVC repository to git")?
    } else {
        let password = if let Ok(pw) = std::env::var("OVC_PASSWORD") {
            pw
        } else {
            dialoguer::Password::new()
                .with_prompt("OVC repository password")
                .interact()
                .context("failed to read password")?
        };

        ovc_git::export::export_to_git(&ovc_path, &output_dir, password.as_bytes())
            .context("failed to export OVC repository to git")?
    };

    output::print_success(&format!(
        "Export complete: {} blobs, {} trees, {} commits, {} tags, {} refs",
        result.blobs_exported,
        result.trees_exported,
        result.commits_exported,
        result.tags_exported,
        result.refs_exported,
    ));
    output::print_success(&format!(
        "Git repository created at {}",
        output_dir.display()
    ));

    Ok(())
}

fn resolve_output_dir(
    ctx: &CliContext,
    args: &GitExportArgs,
    ovc_path: &std::path::Path,
) -> Result<std::path::PathBuf> {
    args.output.as_ref().map_or_else(
        || {
            let stem = ovc_path.file_stem().map_or_else(
                || "exported".to_owned(),
                |s| s.to_string_lossy().into_owned(),
            );
            Ok(ctx.cwd.join(stem))
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
