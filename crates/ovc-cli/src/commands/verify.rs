//! `ovc verify` — Verify the Ed25519 signature on a commit.

use anyhow::{Context, Result};

use ovc_core::keys::{OvcPublicKey, VerifyResult, verify_commit};
use ovc_core::object::Object;

use crate::app::VerifyArgs;
use crate::context::{self, CliContext};
use crate::output;

pub fn execute(ctx: &CliContext, args: &VerifyArgs) -> Result<()> {
    let (repo, _workdir) = ctx.open_repo()?;

    // Delegate to the shared resolver so HEAD, HEAD~N, branch names, full OIDs,
    // and short hex prefixes are all handled consistently with other commands.
    let oid = context::resolve_commit(&args.commit, &repo)?;

    let obj = repo
        .get_object(&oid)
        .context("failed to read object")?
        .ok_or_else(|| anyhow::anyhow!("object not found: {oid}"))?;

    let Object::Commit(commit) = obj else {
        anyhow::bail!("object {oid} is not a commit");
    };

    // Load authorized keys.
    let authorized_keys: Vec<OvcPublicKey> = load_authorized_keys(&repo);

    let result = verify_commit(&commit, &authorized_keys);

    let hex = oid.to_string();
    let short = &hex[..12.min(hex.len())];

    match &result {
        VerifyResult::Verified {
            fingerprint,
            identity,
        } => {
            output::print_success(&format!("Commit {short} has a valid signature"));
            println!("  Key:      {fingerprint}");
            if let Some(id) = identity {
                println!("  Identity: {id}");
            }
        }
        VerifyResult::Unverified { reason } => {
            output::print_error(&format!("Commit {short} has an INVALID signature"));
            println!("  Reason: {reason}");
        }
        VerifyResult::NotSigned => {
            println!("Commit {short} is not signed.");
        }
    }

    Ok(())
}

/// Loads authorized public keys from repo key slots or local keys.
fn load_authorized_keys(repo: &ovc_core::repository::Repository) -> Vec<OvcPublicKey> {
    let fingerprints = repo.list_keys();
    if fingerprints.is_empty() {
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
