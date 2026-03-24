//! `ovc notes [add|show|remove]` — Commit annotations.

use anyhow::{Context, Result};

use ovc_core::id::ObjectId;
use ovc_core::notes;

use crate::app::{NotesAction, NotesArgs};
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &NotesArgs) -> Result<()> {
    let action = args
        .action
        .as_ref()
        .unwrap_or(&NotesAction::Show { commit: None });

    match action {
        NotesAction::Show { commit } => show(ctx, commit.as_deref()),
        NotesAction::Add { message, commit } => add(ctx, message, commit.as_deref()),
        NotesAction::Remove { commit } => remove(ctx, commit.as_deref()),
    }
}

fn resolve_commit(spec: Option<&str>, repo: &ovc_core::repository::Repository) -> Result<ObjectId> {
    if let Some(s) = spec {
        if s.eq_ignore_ascii_case("HEAD") {
            return repo
                .ref_store()
                .resolve_head()
                .context("cannot resolve HEAD");
        }
        s.parse::<ObjectId>()
            .map_err(|e| anyhow::anyhow!("invalid commit id: {e}"))
    } else {
        repo.ref_store().resolve_head().context("no commits yet")
    }
}

fn show(ctx: &CliContext, commit: Option<&str>) -> Result<()> {
    let (repo, _workdir) = ctx.open_repo()?;
    let oid = resolve_commit(commit, &repo)?;

    match notes::get_note(repo.notes(), &oid) {
        Some(note) => println!("{note}"),
        None => println!("no note found for {}", &oid.to_string()[..12]),
    }

    Ok(())
}

fn add(ctx: &CliContext, message: &str, commit: Option<&str>) -> Result<()> {
    let (mut repo, _workdir) = ctx.open_repo()?;
    let oid = resolve_commit(commit, &repo)?;

    notes::set_note(repo.notes_mut(), oid, message.to_owned());
    repo.save().context("failed to save repository")?;

    let short = &oid.to_string()[..12];
    output::print_success(&format!("note added to {short}"));

    Ok(())
}

fn remove(ctx: &CliContext, commit: Option<&str>) -> Result<()> {
    let (mut repo, _workdir) = ctx.open_repo()?;
    let oid = resolve_commit(commit, &repo)?;

    notes::remove_note(repo.notes_mut(), &oid).map_err(|e| anyhow::anyhow!("{e}"))?;

    repo.save().context("failed to save repository")?;

    let short = &oid.to_string()[..12];
    output::print_success(&format!("note removed from {short}"));

    Ok(())
}
