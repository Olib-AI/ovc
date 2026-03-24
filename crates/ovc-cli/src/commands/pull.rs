//! `ovc pull` — Pull the latest version from a remote.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use ovc_cloud::local::LocalBackend;
use ovc_cloud::sync::SyncEngine;

use crate::app::PullArgs;
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &PullArgs) -> Result<()> {
    let ovc_path = ctx.find_ovc_file()?;

    // Open the repo to read the remote config, then drop it so the .ovc file
    // is not held open during the pull (which overwrites the file).
    let remote_config = {
        let (repo, _workdir) = ctx.open_repo()?;
        repo.config()
            .remotes
            .get(&args.remote)
            .ok_or_else(|| anyhow::anyhow!("remote '{}' not configured", args.remote))?
            .clone()
    };

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
    let result = rt.block_on(engine.pull(&ovc_path))?;

    output::print_success(&format!(
        "pulled from '{}': {} chunks downloaded, {} cached, {} bytes transferred",
        args.remote, result.chunks_downloaded, result.chunks_cached, result.bytes_downloaded,
    ));

    // Re-open the updated repo and checkout the HEAD branch to populate
    // the working directory with the pulled content.
    let (mut repo, workdir) = ctx.open_repo()?;
    if let Ok(head_oid) = repo.ref_store().resolve_head()
        && repo.get_object(&head_oid).ok().flatten().is_some()
    {
        // Determine the current branch name from HEAD.
        let branch = match repo.ref_store().head() {
            ovc_core::refs::RefTarget::Symbolic(s) => {
                s.strip_prefix("refs/heads/").unwrap_or(s).to_owned()
            }
            ovc_core::refs::RefTarget::Direct(_) => "main".to_owned(),
        };
        repo.checkout_branch(&branch, &workdir)
            .context("failed to checkout after pull")?;
        repo.save().context("failed to save after pull checkout")?;
    }

    Ok(())
}
