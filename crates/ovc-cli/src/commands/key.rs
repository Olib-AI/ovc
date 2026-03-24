//! `ovc key` — Manage SSH key pairs for OVC repository encryption.

use std::io::Read;

use anyhow::{Context, Result};

use crate::app::{KeyAction, KeyArgs};
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &KeyArgs) -> Result<()> {
    match &args.action {
        KeyAction::Generate { name, identity } => generate(name, identity.as_deref()),
        KeyAction::List => list(),
        KeyAction::Export { name } => export(name),
        KeyAction::Import { path, name } => import(path, name.as_deref()),
        KeyAction::Add { public_key_path } => add_key(ctx, public_key_path),
        KeyAction::Remove { fingerprint } => remove_key(ctx, fingerprint),
        KeyAction::Authorized => authorized(ctx),
    }
}

fn generate(name: &str, identity_str: Option<&str>) -> Result<()> {
    let dir = ovc_core::keys::ovc_keys_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;

    let priv_path = dir.join(format!("{name}.key"));
    let pub_path = dir.join(format!("{name}.pub"));

    if priv_path.exists() || pub_path.exists() {
        anyhow::bail!("key '{name}' already exists at {}", dir.display());
    }

    let passphrase = if let Ok(pw) = std::env::var("OVC_KEY_PASSPHRASE") {
        pw
    } else {
        dialoguer::Password::new()
            .with_prompt("Set key passphrase")
            .with_confirmation("Confirm passphrase", "Passphrases do not match")
            .interact()
            .context("failed to read passphrase")?
    };

    let keypair = if let Some(id_str) = identity_str {
        let identity =
            ovc_core::keys::KeyIdentity::parse(id_str).map_err(|e| anyhow::anyhow!("{e}"))?;
        ovc_core::keys::OvcKeyPair::generate_with_identity(identity)
    } else {
        ovc_core::keys::OvcKeyPair::generate()
    };

    keypair
        .save_private(&priv_path, passphrase.as_bytes())
        .context("failed to save private key")?;
    keypair
        .save_public(&pub_path)
        .context("failed to save public key")?;

    output::print_success(&format!("Generated key pair '{name}'"));
    println!("  Fingerprint: {}", keypair.fingerprint());
    if let Some(id) = keypair.identity() {
        println!("  Identity:    {id}");
    }
    println!("  Private key: {}", priv_path.display());
    println!("  Public key:  {}", pub_path.display());

    println!();
    println!("--- Export for password manager ---");
    let export_text = keypair
        .export_for_password_manager(passphrase.as_bytes())
        .context("failed to generate export")?;
    println!("{export_text}");

    Ok(())
}

fn list() -> Result<()> {
    let keys = ovc_core::keys::list_keys().context("failed to list keys")?;

    if keys.is_empty() {
        println!("No keys found in ~/.ssh/ovc/");
        println!("Run 'ovc key generate' to create one.");
        return Ok(());
    }

    println!("{:<20} FINGERPRINT", "NAME");
    println!("{:<20} -----------", "----");
    for (name, fingerprint, _path) in &keys {
        println!("{name:<20} {fingerprint}");
    }

    Ok(())
}

fn export(name: &str) -> Result<()> {
    let dir = ovc_core::keys::ovc_keys_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;

    let priv_path = dir.join(format!("{name}.key"));
    if !priv_path.exists() {
        anyhow::bail!("private key '{name}' not found at {}", priv_path.display());
    }

    let passphrase = if let Ok(pw) = std::env::var("OVC_KEY_PASSPHRASE") {
        pw
    } else {
        dialoguer::Password::new()
            .with_prompt("Key passphrase")
            .interact()
            .context("failed to read passphrase")?
    };

    let keypair = ovc_core::keys::OvcKeyPair::load_private(&priv_path, passphrase.as_bytes())
        .context("failed to load private key")?;

    let export_text = keypair
        .export_for_password_manager(passphrase.as_bytes())
        .context("failed to generate export")?;

    println!("{export_text}");
    Ok(())
}

/// Validates a key name supplied via `--name` on import.
///
/// Rejects names that are empty, contain path separators (`/`, `\`), or
/// contain characters that would be unsafe in a filename or shell context.
/// Only alphanumeric characters, hyphens, underscores, and dots are allowed.
fn validate_import_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("key name must not be empty");
    }
    if name.len() > 64 {
        anyhow::bail!("key name must not exceed 64 characters");
    }
    for ch in name.chars() {
        if !ch.is_alphanumeric() && ch != '-' && ch != '_' && ch != '.' {
            anyhow::bail!(
                "key name contains invalid character '{ch}'; \
                 only alphanumeric characters, hyphens, underscores, and dots are allowed"
            );
        }
    }
    Ok(())
}

fn import(path: &str, custom_name: Option<&str>) -> Result<()> {
    // Validate the custom name early, before doing any IO.
    if let Some(n) = custom_name {
        validate_import_name(n)?;
    }

    let text = if path == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("failed to read from stdin")?;
        buf
    } else {
        std::fs::read_to_string(path).with_context(|| format!("failed to read file: {path}"))?
    };

    let passphrase = if let Ok(pw) = std::env::var("OVC_KEY_PASSPHRASE") {
        pw
    } else {
        dialoguer::Password::new()
            .with_prompt("Key passphrase")
            .interact()
            .context("failed to read passphrase")?
    };

    let keypair =
        ovc_core::keys::OvcKeyPair::import_from_password_manager(&text, passphrase.as_bytes())
            .context("failed to import key")?;

    // Determine the key name: use the custom name if provided, otherwise
    // derive one from the fingerprint (first 8 chars of the base64 hash).
    let name = custom_name.map_or_else(
        || {
            let fp = keypair.fingerprint();
            let short = fp
                .strip_prefix("SHA256:")
                .unwrap_or(fp)
                .chars()
                .take(8)
                .collect::<String>();
            format!("imported-{short}")
        },
        std::borrow::ToOwned::to_owned,
    );

    let dir = ovc_core::keys::ovc_keys_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;

    let priv_path = dir.join(format!("{name}.key"));
    let pub_path = dir.join(format!("{name}.pub"));

    if priv_path.exists() {
        anyhow::bail!("key already exists at {}", priv_path.display());
    }

    keypair
        .save_private(&priv_path, passphrase.as_bytes())
        .context("failed to save private key")?;
    keypair
        .save_public(&pub_path)
        .context("failed to save public key")?;

    output::print_success(&format!("Imported key as '{name}'"));
    println!("  Fingerprint: {}", keypair.fingerprint());
    println!("  Private key: {}", priv_path.display());
    println!("  Public key:  {}", pub_path.display());

    Ok(())
}

fn add_key(ctx: &CliContext, public_key_path: &std::path::Path) -> Result<()> {
    // If the argument is not a file that exists on disk, treat it as a key name
    // and look it up in ~/.ssh/ovc/<name>.pub before giving up.
    let resolved_path: std::path::PathBuf = if public_key_path.exists() {
        public_key_path.to_path_buf()
    } else {
        let name = public_key_path.to_string_lossy();
        let dir = ovc_core::keys::ovc_keys_dir()
            .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
        let candidate = dir.join(format!("{name}.pub"));
        if candidate.exists() {
            candidate
        } else {
            anyhow::bail!(
                "public key not found: '{}' is neither an existing file nor a key name in {}",
                public_key_path.display(),
                dir.display()
            );
        }
    };

    let pubkey =
        ovc_core::keys::OvcPublicKey::load(&resolved_path).context("failed to load public key")?;

    let (mut repo, _workdir) = ctx.open_repo()?;

    repo.add_key(&pubkey)
        .context("failed to add key to repository")?;
    repo.save().context("failed to save repository")?;

    output::print_success(&format!("Added key {} to repository", pubkey.fingerprint));

    Ok(())
}

fn remove_key(ctx: &CliContext, fingerprint: &str) -> Result<()> {
    let (mut repo, _workdir) = ctx.open_repo()?;

    repo.remove_key(fingerprint)
        .context("failed to remove key from repository")?;
    repo.save().context("failed to save repository")?;

    output::print_success(&format!("Removed key {fingerprint} from repository"));

    Ok(())
}

fn authorized(ctx: &CliContext) -> Result<()> {
    let (repo, _workdir) = ctx.open_repo()?;

    let keys = repo.list_keys();
    if keys.is_empty() {
        println!("No authorized keys (this is a password-only repository).");
        return Ok(());
    }

    println!("Authorized keys:");
    for fingerprint in keys {
        println!("  {fingerprint}");
    }

    Ok(())
}
