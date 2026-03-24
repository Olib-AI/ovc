//! `ovc actions` — Manage and run actions (lint, format, build, test, audit).

use std::path::Path;

use anyhow::{Context, Result, bail};
use ovc_actions::config::{ActionCategory, ActionsConfig, Trigger};
use ovc_actions::detect::detect_languages;
use ovc_actions::history::{ActionHistory, ActionRunRecord};
use ovc_actions::runner::{ActionRunner, ActionStatus};
use ovc_actions::secrets::SecretsVault;

use crate::app::ActionsArgs;
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &ActionsArgs) -> Result<()> {
    match &args.action {
        crate::app::ActionsAction::Init { force } => cmd_init(ctx, *force),
        crate::app::ActionsAction::List { trigger, category } => {
            cmd_list(ctx, trigger.as_deref(), category.as_deref())
        }
        crate::app::ActionsAction::Run {
            names,
            trigger,
            fix,
            no_verify: _,
        } => cmd_run(ctx, names, trigger.as_deref(), *fix),
        crate::app::ActionsAction::History { limit, run_id } => {
            cmd_history(ctx, *limit, run_id.as_deref())
        }
        crate::app::ActionsAction::Detect => cmd_detect(ctx),
        crate::app::ActionsAction::Secrets { action } => cmd_secrets(ctx, action),
    }
}

fn repo_root_from_ctx(ctx: &CliContext) -> Result<std::path::PathBuf> {
    match ctx.find_ovc_file() {
        Ok(ovc_path) => {
            // Use the workdir (where .ovc-link lives), not the remote .ovc file location
            let workdir = CliContext::workdir_for_with_cwd(&ovc_path, &ctx.cwd)?;
            Ok(workdir.root().to_path_buf())
        }
        Err(_) => Ok(ctx.cwd.clone()),
    }
}

fn cmd_init(ctx: &CliContext, force: bool) -> Result<()> {
    let repo_root = repo_root_from_ctx(ctx)?;
    let config_path = repo_root.join(".ovc").join("actions.yml");

    if config_path.exists() && !force {
        bail!(
            "actions config already exists at {}; use --force to overwrite",
            config_path.display()
        );
    }

    let result = detect_languages(&repo_root);

    if result.languages.is_empty() {
        output::print_warning("no languages detected; generating minimal config");
    } else {
        let langs: Vec<&str> = result
            .languages
            .iter()
            .map(|l| l.language.as_str())
            .collect();
        output::print_success(&format!("detected languages: {}", langs.join(", ")));
    }

    let yaml =
        serde_yaml::to_string(&result.suggested_config).context("failed to serialize config")?;

    std::fs::create_dir_all(repo_root.join(".ovc")).context("failed to create .ovc directory")?;
    std::fs::write(&config_path, &yaml).context("failed to write actions.yml")?;

    output::print_success(&format!("wrote {}", config_path.display()));

    add_to_ovcignore(&repo_root, ".ovc/actions-history/")?;

    Ok(())
}

fn add_to_ovcignore(repo_root: &Path, pattern: &str) -> Result<()> {
    let ignore_path = repo_root.join(".ovcignore");
    if ignore_path.exists() {
        let content = std::fs::read_to_string(&ignore_path).context("failed to read .ovcignore")?;
        if content.lines().any(|line| line.trim() == pattern) {
            return Ok(());
        }
        let mut new_content = content;
        if !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str(pattern);
        new_content.push('\n');
        std::fs::write(&ignore_path, new_content).context("failed to write .ovcignore")?;
    } else {
        std::fs::write(&ignore_path, format!("{pattern}\n"))
            .context("failed to write .ovcignore")?;
    }
    Ok(())
}

fn cmd_list(ctx: &CliContext, trigger: Option<&str>, category: Option<&str>) -> Result<()> {
    let repo_root = repo_root_from_ctx(ctx)?;
    let config = ActionsConfig::load(&repo_root)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .ok_or_else(|| anyhow::anyhow!("no actions config found; run `ovc actions init` first"))?;

    let trigger_filter: Option<Trigger> = trigger.map(parse_trigger).transpose()?;
    let category_filter: Option<ActionCategory> = category.map(parse_category).transpose()?;

    let header = format!(
        "{:<10} {:<25} {:<14} {:<12} {}",
        "CATEGORY", "NAME", "TRIGGER", "LANGUAGE", "TOOL"
    );
    println!("{header}");
    println!("{}", "-".repeat(75));

    for (name, def) in &config.actions {
        if trigger_filter.is_some_and(|t| def.trigger != t) {
            continue;
        }
        if category_filter.is_some_and(|c| def.category != c) {
            continue;
        }
        let tool_name = def
            .tool
            .as_deref()
            .or_else(|| def.builtin.as_ref().map(|_| "builtin"))
            .unwrap_or("-");
        println!(
            "{:<10} {:<25} {:<14} {:<12} {tool_name}",
            def.category,
            def.display_name.as_deref().unwrap_or(name),
            def.trigger,
            def.language.as_deref().unwrap_or("-"),
        );
    }

    Ok(())
}

fn cmd_run(ctx: &CliContext, names: &[String], trigger: Option<&str>, fix: bool) -> Result<()> {
    let repo_root = repo_root_from_ctx(ctx)?;
    let config = ActionsConfig::load(&repo_root)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .ok_or_else(|| anyhow::anyhow!("no actions config found; run `ovc actions init` first"))?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to create async runtime")?;

    let runner = rt.block_on(ActionRunner::new_with_docker_probe(&repo_root, config));

    let results = if !names.is_empty() {
        let mut results = Vec::new();
        for name in names {
            let result = if fix {
                rt.block_on(runner.run_action_fix(name))
            } else {
                rt.block_on(runner.run_action(name))
            };
            match result {
                Ok(r) => {
                    print_action_result(&r);
                    results.push(r);
                }
                Err(e) => {
                    output::print_error(&format!("{name}: {e}"));
                }
            }
        }
        results
    } else if let Some(trigger_str) = trigger {
        let trigger_val = parse_trigger(trigger_str)?;
        let results = rt.block_on(runner.run_trigger(trigger_val, &[]));
        for r in &results {
            print_action_result(r);
        }
        results
    } else {
        bail!("specify action names or --trigger");
    };

    let passed = results
        .iter()
        .filter(|r| r.status == ActionStatus::Passed)
        .count();
    let failed = results
        .iter()
        .filter(|r| r.status == ActionStatus::Failed)
        .count();
    let skipped = results
        .iter()
        .filter(|r| r.status == ActionStatus::Skipped)
        .count();
    let timed_out = results
        .iter()
        .filter(|r| r.status == ActionStatus::TimedOut)
        .count();
    let errored = results
        .iter()
        .filter(|r| r.status == ActionStatus::Error)
        .count();

    println!();
    println!(
        "Summary: {passed} passed, {failed} failed, {skipped} skipped, {timed_out} timed out, {errored} errors"
    );

    // Record to history
    let run_id = uuid::Uuid::new_v4().to_string();
    let overall = if failed + timed_out + errored > 0 {
        "failed"
    } else {
        "passed"
    };
    let total_ms: u64 = results.iter().map(|r| r.duration_ms).sum();
    let record = ActionRunRecord {
        run_id,
        trigger: trigger.unwrap_or("manual").to_owned(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        results,
        overall_status: overall.to_owned(),
        total_duration_ms: total_ms,
    };
    let history = ActionHistory::new(&repo_root);
    if let Err(e) = history.record_run(&record) {
        output::print_warning(&format!("failed to record history: {e}"));
    }

    if failed + timed_out + errored > 0 {
        bail!("some actions failed");
    }

    Ok(())
}

fn cmd_history(ctx: &CliContext, limit: usize, run_id: Option<&str>) -> Result<()> {
    let repo_root = repo_root_from_ctx(ctx)?;
    let history = ActionHistory::new(&repo_root);

    if let Some(id) = run_id {
        let record = history
            .get_run(id)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .ok_or_else(|| anyhow::anyhow!("run not found: {id}"))?;
        println!("Run: {}", record.run_id);
        println!("Trigger: {}", record.trigger);
        println!("Time: {}", record.timestamp);
        println!("Status: {}", record.overall_status);
        println!("Duration: {}ms", record.total_duration_ms);
        println!();
        for r in &record.results {
            print_action_result(r);
        }
    } else {
        let runs = history
            .list_runs(limit)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        if runs.is_empty() {
            println!("No action runs recorded yet.");
            return Ok(());
        }
        let header = format!(
            "{:<38} {:<14} {:<10} {:<10}",
            "RUN ID", "TRIGGER", "STATUS", "DURATION"
        );
        println!("{header}");
        println!("{}", "-".repeat(75));
        for run in &runs {
            println!(
                "{:<38} {:<14} {:<10} {}ms",
                run.run_id, run.trigger, run.overall_status, run.total_duration_ms
            );
        }
    }

    Ok(())
}

fn cmd_detect(ctx: &CliContext) -> Result<()> {
    let repo_root = repo_root_from_ctx(ctx)?;
    let result = detect_languages(&repo_root);

    if result.languages.is_empty() {
        println!("No languages detected.");
        return Ok(());
    }

    let header = format!(
        "{:<15} {:<12} {:<20} {}",
        "LANGUAGE", "CONFIDENCE", "MARKER FILE", "ROOT DIR"
    );
    println!("{header}");
    println!("{}", "-".repeat(60));
    for lang in &result.languages {
        println!(
            "{:<15} {:<12} {:<20} {}",
            lang.language, lang.confidence, lang.marker_file, lang.root_dir
        );
    }

    println!();
    println!("Run `ovc actions init` to generate a starter configuration.");

    Ok(())
}

fn cmd_secrets(ctx: &CliContext, action: &crate::app::SecretsAction) -> Result<()> {
    let repo_root = repo_root_from_ctx(ctx)?;

    match action {
        crate::app::SecretsAction::List => {
            let vault = SecretsVault::load(&repo_root).context("failed to load secrets vault")?;
            let names = vault.list_names();
            if names.is_empty() {
                println!("No secrets configured.");
            } else {
                for name in &names {
                    println!("{name}");
                }
            }
        }
        crate::app::SecretsAction::Set { name, value } => {
            let mut vault =
                SecretsVault::load(&repo_root).context("failed to load secrets vault")?;
            vault.set(name.clone(), value.clone());
            vault
                .save(&repo_root)
                .context("failed to save secrets vault")?;
            output::print_success(&format!("secret '{name}' set"));
        }
        crate::app::SecretsAction::Remove { name } => {
            let mut vault =
                SecretsVault::load(&repo_root).context("failed to load secrets vault")?;
            if vault.remove(name) {
                vault
                    .save(&repo_root)
                    .context("failed to save secrets vault")?;
                output::print_success(&format!("secret '{name}' removed"));
            } else {
                bail!("secret '{name}' not found");
            }
        }
    }

    Ok(())
}

fn print_action_result(r: &ovc_actions::runner::ActionResult) {
    let status_str = match r.status {
        ActionStatus::Passed => console::Style::new().green().apply_to("PASS").to_string(),
        ActionStatus::Failed => console::Style::new().red().apply_to("FAIL").to_string(),
        ActionStatus::Skipped => console::Style::new().dim().apply_to("SKIP").to_string(),
        ActionStatus::TimedOut => console::Style::new().yellow().apply_to("TIME").to_string(),
        ActionStatus::Error => console::Style::new()
            .red()
            .bold()
            .apply_to("ERR ")
            .to_string(),
    };

    println!("  [{status_str}] {} ({}ms)", r.display_name, r.duration_ms);

    if !r.stdout.is_empty() && r.status != ActionStatus::Passed {
        for line in r.stdout.lines() {
            println!("       {line}");
        }
    }
    if !r.stderr.is_empty() {
        for line in r.stderr.lines() {
            println!("       {line}");
        }
    }
}

fn parse_trigger(s: &str) -> Result<Trigger> {
    match s {
        "pre-commit" => Ok(Trigger::PreCommit),
        "post-commit" => Ok(Trigger::PostCommit),
        "pre-push" => Ok(Trigger::PrePush),
        "pre-merge" => Ok(Trigger::PreMerge),
        "post-merge" => Ok(Trigger::PostMerge),
        "on-fail" => Ok(Trigger::OnFail),
        "pull-request" => Ok(Trigger::PullRequest),
        "manual" => Ok(Trigger::Manual),
        "schedule" => Ok(Trigger::Schedule),
        other => bail!("unknown trigger: {other}"),
    }
}

fn parse_category(s: &str) -> Result<ActionCategory> {
    match s {
        "lint" => Ok(ActionCategory::Lint),
        "format" => Ok(ActionCategory::Format),
        "build" => Ok(ActionCategory::Build),
        "test" => Ok(ActionCategory::Test),
        "audit" => Ok(ActionCategory::Audit),
        "builtin" => Ok(ActionCategory::Builtin),
        "custom" => Ok(ActionCategory::Custom),
        other => bail!("unknown category: {other}"),
    }
}
