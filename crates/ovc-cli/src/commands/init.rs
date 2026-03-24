//! `ovc init` — Initialize a new OVC repository.

use anyhow::{Context, Result};

use crate::app::InitArgs;
use crate::context::CliContext;
use crate::output;

#[allow(clippy::too_many_lines)]
pub fn execute(ctx: &CliContext, args: &InitArgs) -> Result<()> {
    let dir = if args.path == std::path::Path::new(".") {
        ctx.cwd.clone()
    } else {
        args.path.clone()
    };

    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create directory: {}", dir.display()))?;
    }

    // Determine the actual .ovc file path: either in --store location or local directory.
    let (ovc_path, link_path) = if let Some(ref store_path) = args.store {
        // If --store is a directory (or ends with /), append the filename inside it.
        let actual_store = if store_path.is_dir()
            || store_path.to_string_lossy().ends_with('/')
            || store_path
                .to_string_lossy()
                .ends_with(std::path::MAIN_SEPARATOR)
        {
            std::fs::create_dir_all(store_path).with_context(|| {
                format!("failed to create store directory: {}", store_path.display())
            })?;
            store_path.join(&args.name)
        } else {
            // --store points to a specific file path
            if let Some(parent) = store_path.parent()
                && !parent.exists()
            {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create store directory: {}", parent.display())
                })?;
            }
            store_path.clone()
        };
        let link = dir.join(".ovc-link");
        (actual_store, Some(link))
    } else {
        (dir.join(&args.name), None)
    };

    if ovc_path.exists() {
        anyhow::bail!("repository already exists at {}", ovc_path.display());
    }

    if let Some(ref key_query) = args.key {
        // Key-based initialization.
        let pub_path = ovc_core::keys::find_key(key_query)
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

        let mut repo = ovc_core::repository::Repository::init_with_key(&ovc_path, &keypair)
            .context("failed to initialize repository with key")?;

        repo.config_mut()
            .default_branch
            .clone_from(&args.default_branch);
        // Update HEAD to point to the configured default branch.
        let head_ref = format!("refs/heads/{}", args.default_branch);
        repo.set_head_ref(head_ref.clone());
        repo.ref_store_mut()
            .set_head(ovc_core::refs::RefTarget::Symbolic(head_ref));
        repo.save().context("failed to save repository")?;

        output::print_success(&format!(
            "Initialized empty OVC repository at {} (key: {})",
            ovc_path.display(),
            keypair.fingerprint()
        ));
    } else {
        // Password-based initialization (original flow).
        let password = if let Ok(pw) = std::env::var("OVC_PASSWORD") {
            pw
        } else {
            dialoguer::Password::new()
                .with_prompt("Set repository password")
                .with_confirmation("Confirm password", "Passwords do not match")
                .interact()
                .context("failed to read password")?
        };

        let mut repo = ovc_core::repository::Repository::init(&ovc_path, password.as_bytes())
            .context("failed to initialize repository")?;

        repo.config_mut()
            .default_branch
            .clone_from(&args.default_branch);
        // Update HEAD to point to the configured default branch.
        let head_ref = format!("refs/heads/{}", args.default_branch);
        repo.set_head_ref(head_ref.clone());
        repo.ref_store_mut()
            .set_head(ovc_core::refs::RefTarget::Symbolic(head_ref));
        repo.save().context("failed to save repository")?;

        output::print_success(&format!(
            "Initialized empty OVC repository at {}",
            ovc_path.display()
        ));
    }

    // If --store was used, write a .ovc-link file pointing to the actual location.
    if let Some(link) = link_path {
        let canonical = ovc_path.canonicalize().with_context(|| {
            format!("failed to canonicalize store path: {}", ovc_path.display())
        })?;
        std::fs::write(&link, canonical.to_string_lossy().as_bytes())
            .with_context(|| format!("failed to write .ovc-link at {}", link.display()))?;

        output::print_success(&format!(
            "Created .ovc-link at {} -> {}",
            link.display(),
            canonical.display()
        ));
    }

    Ok(())
}
