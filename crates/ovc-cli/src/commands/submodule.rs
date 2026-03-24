//! `ovc submodule [add|status|update|remove]` — Manage nested repositories.

use anyhow::{Context, Result, bail};

use ovc_core::submodule::SubmoduleConfig;

use crate::app::{SubmoduleAction, SubmoduleArgs};
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &SubmoduleArgs) -> Result<()> {
    let action = args.action.as_ref().unwrap_or(&SubmoduleAction::Status);

    match action {
        SubmoduleAction::Add { name, url, path } => add(ctx, name, url, path.as_deref()),
        SubmoduleAction::Status => status(ctx),
        SubmoduleAction::Update => update(ctx),
        SubmoduleAction::Remove { name } => remove(ctx, name),
    }
}

fn add(ctx: &CliContext, name: &str, url: &str, path: Option<&str>) -> Result<()> {
    let (mut repo, _workdir) = ctx.open_repo()?;

    // Use the provided name directly; fall back to name when path is omitted.
    let resolved_path = path.unwrap_or(name);

    if repo.submodules().contains_key(name) {
        bail!("submodule '{name}' already exists");
    }

    let ovc_file = if std::path::Path::new(resolved_path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ovc"))
    {
        resolved_path.to_owned()
    } else {
        format!("{resolved_path}/repo.ovc")
    };

    let config = SubmoduleConfig {
        path: resolved_path.to_owned(),
        url: url.to_owned(),
        ovc_file,
        pinned_sequence: 0,
        status: ovc_core::submodule::SubmoduleStatus::Configured,
    };

    repo.submodules_mut().insert(name.to_owned(), config);
    repo.save().context("failed to save repository")?;

    output::print_success(&format!("submodule '{name}' added at {resolved_path}"));

    Ok(())
}

fn status(ctx: &CliContext) -> Result<()> {
    let (repo, workdir) = ctx.open_repo()?;

    let submodules = repo.submodules();

    if submodules.is_empty() {
        println!("no submodules configured");
        return Ok(());
    }

    for (name, config) in submodules {
        let ovc_path = workdir.root().join(&config.ovc_file);
        let exists = ovc_path.exists();
        let status = if exists { "present" } else { "missing" };

        println!(
            " {status}\t{name} ({}) -> {} [seq:{}]",
            config.path, config.url, config.pinned_sequence
        );
    }

    Ok(())
}

fn update(ctx: &CliContext) -> Result<()> {
    let (repo, workdir) = ctx.open_repo()?;

    let submodules = repo.submodules();
    if submodules.is_empty() {
        println!("no submodules to update");
        return Ok(());
    }

    for (name, config) in submodules {
        let ovc_path = workdir.root().join(&config.ovc_file);
        if ovc_path.exists() {
            println!(
                "submodule '{name}' at {} is present (manual update required)",
                config.path
            );
        } else {
            output::print_warning(&format!(
                "submodule '{name}' not found at {}; clone from {} first",
                config.ovc_file, config.url
            ));
        }
    }

    Ok(())
}

fn remove(ctx: &CliContext, name: &str) -> Result<()> {
    let (mut repo, _workdir) = ctx.open_repo()?;

    if repo.submodules_mut().remove(name).is_none() {
        bail!("submodule '{name}' not found");
    }

    repo.save().context("failed to save repository")?;

    output::print_success(&format!("submodule '{name}' removed"));

    Ok(())
}
