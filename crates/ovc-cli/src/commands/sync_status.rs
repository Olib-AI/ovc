//! `ovc sync-status` — Show sync status with the remote.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use ovc_cloud::local::LocalBackend;
use ovc_cloud::sync::{SyncEngine, SyncStatus};

use crate::app::SyncStatusArgs;
use crate::context::CliContext;

pub fn execute(ctx: &CliContext, args: &SyncStatusArgs) -> Result<()> {
    let (repo, _workdir) = ctx.open_repo()?;
    let ovc_path = ctx.find_ovc_file()?;

    let remote_config = repo
        .config()
        .remotes
        .get(&args.remote)
        .ok_or_else(|| anyhow::anyhow!("remote '{}' not configured", args.remote))?
        .clone();

    let repo_id = args.remote.clone();

    let backend: Box<dyn ovc_cloud::StorageBackend> = match remote_config.backend_type.as_str() {
        "local" => {
            let path = PathBuf::from(&remote_config.url);
            let local = LocalBackend::new(path).context("failed to create local backend")?;
            Box::new(local)
        }
        "gcs" => {
            bail!(
                "GCS backend requires authentication. Set OVC_GCS_TOKEN environment variable \
                 and use the URL format 'bucket/prefix'."
            );
        }
        other => bail!("unknown backend type: {other}"),
    };

    let engine = SyncEngine::new(backend, repo_id);

    let rt = tokio::runtime::Runtime::new().context("failed to create async runtime")?;
    let status = rt.block_on(engine.status(&ovc_path))?;

    match status {
        SyncStatus::InSync { version } => {
            println!("In sync with '{}' (version {version})", args.remote);
        }
        SyncStatus::LocalAhead => {
            println!(
                "Local is ahead of '{}'. Run `ovc push` to upload changes.",
                args.remote
            );
        }
        SyncStatus::RemoteAhead { remote_version } => {
            println!(
                "Remote '{}' is ahead (version {remote_version}). Run `ovc pull` to download.",
                args.remote
            );
        }
        SyncStatus::Diverged => {
            println!(
                "Local and remote '{}' have diverged. Manual resolution required.",
                args.remote
            );
        }
        SyncStatus::NoRemote => {
            println!(
                "No data on remote '{}'. Run `ovc push` to upload.",
                args.remote
            );
        }
    }

    Ok(())
}
