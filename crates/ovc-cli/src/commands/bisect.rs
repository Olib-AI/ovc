//! `ovc bisect` — Binary search for a regression-introducing commit.

use anyhow::{Context, Result};

use ovc_core::bisect::{BisectState, BisectStep};

use crate::app::{BisectAction, BisectCliArgs};
use crate::context::{self, CliContext};
use crate::output;

/// Filesystem path for persisting bisect state between invocations.
const BISECT_STATE_FILE: &str = ".ovc-bisect.json";

pub fn execute(ctx: &CliContext, args: &BisectCliArgs) -> Result<()> {
    match &args.action {
        BisectAction::Start { good, bad } => execute_start(ctx, good, bad),
        BisectAction::Good { commit } => execute_mark(ctx, true, commit.as_deref()),
        BisectAction::Bad { commit } => execute_mark(ctx, false, commit.as_deref()),
        BisectAction::Reset => execute_reset(ctx),
    }
}

fn execute_start(ctx: &CliContext, good_hex: &str, bad_hex: &str) -> Result<()> {
    let (repo, _workdir) = ctx.open_repo()?;

    let good = context::resolve_commit(good_hex, &repo)?;
    let bad = context::resolve_commit(bad_hex, &repo)?;

    let state =
        BisectState::start(good, bad, repo.object_store()).context("failed to start bisect")?;

    let remaining = state.remaining_steps();

    match state.current() {
        Some(oid) => {
            let hex = oid.to_string();
            println!(
                "Bisecting: {} candidates remaining (~{remaining} steps)",
                state.candidates.len()
            );
            println!("Test commit: {}", &hex[..12.min(hex.len())]);
        }
        None => {
            println!("No candidates found between good and bad commits.");
        }
    }

    save_bisect_state(ctx, &state)?;
    output::print_success("Bisect session started");
    Ok(())
}

fn execute_mark(ctx: &CliContext, is_good: bool, commit_spec: Option<&str>) -> Result<()> {
    let mut state = load_bisect_state(ctx)?;

    let target = if let Some(spec) = commit_spec {
        // Resolve the explicit commit argument using the repository.
        let (repo, _workdir) = ctx.open_repo()?;
        context::resolve_commit(spec, &repo)
            .with_context(|| format!("cannot resolve commit '{spec}'"))?
    } else {
        state
            .current()
            .ok_or_else(|| anyhow::anyhow!("no current bisect commit"))?
    };

    let step = if is_good {
        state.mark_good(target)
    } else {
        state.mark_bad(target)
    };

    match step {
        BisectStep::Test(oid) => {
            let hex = oid.to_string();
            let remaining = state.remaining_steps();
            println!(
                "Bisecting: {} candidates remaining (~{remaining} steps)",
                state.candidates.len()
            );
            println!("Test commit: {}", &hex[..12.min(hex.len())]);
            save_bisect_state(ctx, &state)?;
        }
        BisectStep::Found(oid) => {
            let hex = oid.to_string();
            output::print_success(&format!("First bad commit: {}", &hex[..12.min(hex.len())]));
            remove_bisect_state(ctx);
        }
        BisectStep::NoCandidates => {
            println!("No more candidates.");
            remove_bisect_state(ctx);
        }
    }

    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
fn execute_reset(ctx: &CliContext) -> Result<()> {
    remove_bisect_state(ctx);
    output::print_success("Bisect session reset");
    Ok(())
}

/// Serializable form of `BisectState` for filesystem persistence.
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedBisect {
    good: Vec<String>,
    bad: Vec<String>,
    candidates: Vec<String>,
    current_idx: usize,
}

fn save_bisect_state(ctx: &CliContext, state: &BisectState) -> Result<()> {
    let persisted = PersistedBisect {
        good: state.good.iter().map(ToString::to_string).collect(),
        bad: state.bad.iter().map(ToString::to_string).collect(),
        candidates: state.candidates.iter().map(ToString::to_string).collect(),
        current_idx: state.current_idx,
    };
    let path = ctx.cwd.join(BISECT_STATE_FILE);
    let json =
        serde_json::to_string_pretty(&persisted).context("failed to serialize bisect state")?;
    std::fs::write(&path, json).context("failed to write bisect state file")?;
    Ok(())
}

fn load_bisect_state(ctx: &CliContext) -> Result<BisectState> {
    let path = ctx.cwd.join(BISECT_STATE_FILE);
    let json = std::fs::read_to_string(&path)
        .context("no active bisect session (run 'ovc bisect start' first)")?;
    let persisted: PersistedBisect =
        serde_json::from_str(&json).context("failed to parse bisect state")?;

    let parse_oids = |hexes: &[String]| -> Result<Vec<ovc_core::id::ObjectId>> {
        hexes
            .iter()
            .map(|h| {
                h.parse()
                    .map_err(|e| anyhow::anyhow!("invalid object id in bisect state: {e}"))
            })
            .collect()
    };

    Ok(BisectState {
        good: parse_oids(&persisted.good)?,
        bad: parse_oids(&persisted.bad)?,
        candidates: parse_oids(&persisted.candidates)?,
        current_idx: persisted.current_idx,
    })
}

fn remove_bisect_state(ctx: &CliContext) {
    let path = ctx.cwd.join(BISECT_STATE_FILE);
    let _ = std::fs::remove_file(path);
}
