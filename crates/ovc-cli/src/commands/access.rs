//! `ovc access` — Manage per-user access control for OVC repositories.

use anyhow::{Context, Result};

use crate::app::{AccessAction, AccessArgs};
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &AccessArgs) -> Result<()> {
    match &args.action {
        AccessAction::Grant { key, role } => grant(ctx, key, role),
        AccessAction::Revoke { fingerprint } => revoke(ctx, fingerprint),
        AccessAction::List => list(ctx),
        AccessAction::SetRole { fingerprint, role } => set_role(ctx, fingerprint, role),
    }
}

fn grant(ctx: &CliContext, key_path: &str, role_str: &str) -> Result<()> {
    let role = ovc_core::access::AccessRole::parse(role_str).ok_or_else(|| {
        anyhow::anyhow!("invalid role: {role_str} (use: read, write, admin, owner)")
    })?;

    // Resolve the public key: could be a file path or a key name in ~/.ssh/ovc/
    let resolved_path: std::path::PathBuf = if std::path::Path::new(key_path).exists() {
        std::path::PathBuf::from(key_path)
    } else {
        let dir = ovc_core::keys::ovc_keys_dir()
            .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
        let candidate = dir.join(format!("{key_path}.pub"));
        if candidate.exists() {
            candidate
        } else {
            anyhow::bail!(
                "public key not found: '{}' is neither an existing file nor a key name in {}",
                key_path,
                dir.display()
            );
        }
    };

    let pubkey =
        ovc_core::keys::OvcPublicKey::load(&resolved_path).context("failed to load public key")?;

    let (mut repo, _workdir) = ctx.open_repo()?;

    // Determine grantor fingerprint from the current user's key.
    let grantor = std::env::var("OVC_KEY")
        .ok()
        .and_then(|k| {
            ovc_core::keys::find_key(&k)
                .ok()
                .flatten()
                .and_then(|p| ovc_core::keys::OvcPublicKey::load(&p).ok())
                .map(|pk| pk.fingerprint)
        })
        .unwrap_or_else(|| "local-user".to_owned());

    repo.grant_access(&pubkey, role, &grantor)
        .context("failed to grant access")?;
    repo.save().context("failed to save repository")?;

    output::print_success(&format!(
        "Granted {} access to {}",
        role, pubkey.fingerprint
    ));
    if let Some(identity) = &pubkey.identity {
        println!("  Identity: {identity}");
    }

    Ok(())
}

fn revoke(ctx: &CliContext, fingerprint: &str) -> Result<()> {
    let (mut repo, _workdir) = ctx.open_repo()?;

    repo.revoke_access(fingerprint)
        .context("failed to revoke access")?;
    repo.save().context("failed to save repository")?;

    output::print_success(&format!("Revoked access for {fingerprint}"));

    Ok(())
}

fn list(ctx: &CliContext) -> Result<()> {
    let (repo, _workdir) = ctx.open_repo()?;

    let acl = repo.access_control();
    if acl.is_empty() {
        println!(
            "No access control configured (legacy mode — all authenticated users have full access)."
        );
        println!("Run 'ovc access grant <key> --role owner' to enable access control.");
        return Ok(());
    }

    println!("Authorized users ({}):", acl.users.len());
    println!();
    for user in &acl.users {
        let identity = user
            .identity
            .as_ref()
            .map_or(String::new(), |i| format!(" ({i})"));
        println!("  {} [{}]{}", user.fingerprint, user.role, identity);
        println!("    Added: {} by {}", user.added_at, user.added_by);
    }

    if !acl.branch_protection.is_empty() {
        println!();
        println!("Branch protection rules:");
        for (branch, protection) in &acl.branch_protection {
            println!("  {branch}:");
            println!("    Required approvals: {}", protection.required_approvals);
            println!("    Require CI pass: {}", protection.require_ci_pass);
            let merge_roles: Vec<_> = protection
                .allowed_merge_roles
                .iter()
                .map(ToString::to_string)
                .collect();
            let push_roles: Vec<_> = protection
                .allowed_push_roles
                .iter()
                .map(ToString::to_string)
                .collect();
            println!("    Allowed merge roles: {}", merge_roles.join(", "));
            println!("    Allowed push roles: {}", push_roles.join(", "));
        }
    }

    Ok(())
}

fn set_role(ctx: &CliContext, fingerprint: &str, role_str: &str) -> Result<()> {
    let role = ovc_core::access::AccessRole::parse(role_str).ok_or_else(|| {
        anyhow::anyhow!("invalid role: {role_str} (use: read, write, admin, owner)")
    })?;

    let (mut repo, _workdir) = ctx.open_repo()?;

    repo.set_role(fingerprint, role)
        .context("failed to set role")?;
    repo.save().context("failed to save repository")?;

    output::print_success(&format!("Set role for {fingerprint} to {role}"));

    Ok(())
}
