//! `ovc push` — Push the local repository to a remote.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use ovc_cloud::local::LocalBackend;
use ovc_cloud::sync::SyncEngine;

use crate::app::PushArgs;
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &PushArgs) -> Result<()> {
    let (repo, _workdir) = ctx.open_repo()?;
    let ovc_path = ctx.find_ovc_file()?;

    // Run pre-push hooks unless --no-verify
    if !args.no_verify {
        let repo_root = ovc_path
            .parent()
            .context("cannot determine parent directory of .ovc file")?;
        let hook_results = match ovc_actions::hooks::run_pre_push_hooks(repo_root, &[]) {
            Ok(results) => results,
            Err(e) => {
                output::print_warning(&format!("pre-push hooks config error: {e}"));
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
                bail!("pre-push hooks failed; use --no-verify to bypass");
            }
        }
    }

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
    let result = rt.block_on(engine.push(&ovc_path))?;

    output::print_success(&format!(
        "pushed to '{}': {} chunks uploaded, {} reused, {} bytes transferred",
        args.remote, result.chunks_uploaded, result.chunks_reused, result.bytes_uploaded,
    ));

    Ok(())
}
