//! `ovc serve` — Start the API server and embedded web UI.

use anyhow::{Context, Result};
use tracing_subscriber::EnvFilter;

use crate::app::ServeArgs;

pub fn execute(args: &ServeArgs) -> Result<()> {
    // Initialize tracing with env filter (default: info).
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let rt = tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;
    rt.block_on(async {
        // Parse --workdir flags: "repo_id:/path/to/workdir"
        let workdir_map: Vec<(String, std::path::PathBuf)> = args
            .workdir
            .iter()
            .filter_map(|entry| {
                let (name, path) = entry.split_once(':')?;
                Some((
                    name.trim().to_owned(),
                    std::path::PathBuf::from(path.trim()),
                ))
            })
            .collect();

        let workdir_scan: Vec<std::path::PathBuf> = args
            .workdir_scan
            .iter()
            .map(|s| std::path::PathBuf::from(s.trim()))
            .collect();

        let config = ovc_api::ServerConfig {
            port: args.port,
            bind: args.bind.clone(),
            repos_dir: args.repos_dir.clone(),
            jwt_secret: args.jwt_secret.clone(),
            cors_origins: args.cors_origin.clone(),
            workdir_map,
            workdir_scan,
        };
        ovc_api::start_server(config)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    })
}
