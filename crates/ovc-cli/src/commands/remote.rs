//! `ovc remote` — Manage remote repositories (config-only stub for Phase 3).

use anyhow::{Context, Result};

use ovc_core::config::RemoteConfig;

use crate::app::{RemoteAction, RemoteArgs};
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &RemoteArgs) -> Result<()> {
    let (mut repo, _workdir) = ctx.open_repo()?;

    match args.action {
        Some(RemoteAction::Add {
            ref name,
            ref url,
            ref backend,
        }) => {
            let config = repo.config_mut();
            if config.remotes.contains_key(name) {
                anyhow::bail!("remote '{name}' already exists");
            }
            config.remotes.insert(
                name.clone(),
                RemoteConfig {
                    url: url.clone(),
                    backend_type: backend.clone(),
                },
            );
            repo.save().context("failed to save repository")?;
            output::print_success(&format!("added remote '{name}'"));
        }
        Some(RemoteAction::Remove { ref name }) => {
            let config = repo.config_mut();
            if config.remotes.remove(name).is_none() {
                anyhow::bail!("remote '{name}' not found");
            }
            repo.save().context("failed to save repository")?;
            output::print_success(&format!("removed remote '{name}'"));
        }
        Some(RemoteAction::List) | None => {
            let remotes = &repo.config().remotes;
            if remotes.is_empty() {
                println!("no remotes configured");
            } else {
                for (name, config) in remotes {
                    println!("{name}\t{}", config.url);
                }
            }
        }
    }

    Ok(())
}
