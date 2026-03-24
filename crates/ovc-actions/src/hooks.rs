//! Hook integration — runs configured actions as part of commit/push workflows.

use std::path::Path;

use crate::config::{ActionsConfig, Trigger};
use crate::error::ActionsResult;
use crate::runner::{ActionResult, ActionRunner, ActionStatus};

/// Load config and run all pre-commit actions.
/// Returns an empty vec if no config is found.
pub fn run_pre_commit_hooks(
    repo_root: &Path,
    changed_paths: &[String],
) -> ActionsResult<Vec<ActionResult>> {
    run_hooks_for_trigger(repo_root, Trigger::PreCommit, changed_paths)
}

/// Load config and run all pre-push actions.
/// Returns an empty vec if no config is found.
pub fn run_pre_push_hooks(
    repo_root: &Path,
    changed_paths: &[String],
) -> ActionsResult<Vec<ActionResult>> {
    run_hooks_for_trigger(repo_root, Trigger::PrePush, changed_paths)
}

/// Load config and run all pre-merge actions.
/// Returns an empty vec if no config is found.
pub fn run_pre_merge_hooks(
    repo_root: &Path,
    changed_paths: &[String],
) -> ActionsResult<Vec<ActionResult>> {
    run_hooks_for_trigger(repo_root, Trigger::PreMerge, changed_paths)
}

/// Load config and run all post-merge actions (non-blocking).
/// Returns an empty vec if no config is found.
pub fn run_post_merge_hooks(
    repo_root: &Path,
    changed_paths: &[String],
) -> ActionsResult<Vec<ActionResult>> {
    run_hooks_for_trigger(repo_root, Trigger::PostMerge, changed_paths)
}

/// Load config and run all post-commit actions (non-blocking).
/// Returns an empty vec if no config is found.
pub fn run_post_commit_hooks(
    repo_root: &Path,
    changed_paths: &[String],
) -> ActionsResult<Vec<ActionResult>> {
    run_hooks_for_trigger(repo_root, Trigger::PostCommit, changed_paths)
}

/// Load config and run all pull-request actions.
/// Returns an empty vec if no config is found.
pub fn run_pull_request_hooks(
    repo_root: &Path,
    changed_paths: &[String],
) -> ActionsResult<Vec<ActionResult>> {
    run_hooks_for_trigger(repo_root, Trigger::PullRequest, changed_paths)
}

/// Returns `true` if any result is a blocking failure
/// (failed/timed-out/error and not `continue_on_error`).
#[must_use]
pub fn has_blocking_failures(results: &[ActionResult]) -> bool {
    results.iter().any(|r| {
        !r.continue_on_error
            && matches!(
                r.status,
                ActionStatus::Failed | ActionStatus::TimedOut | ActionStatus::Error
            )
    })
}

fn run_hooks_for_trigger(
    repo_root: &Path,
    trigger: Trigger,
    changed_paths: &[String],
) -> ActionsResult<Vec<ActionResult>> {
    let Some(config) = ActionsConfig::load(repo_root)? else {
        return Ok(Vec::new());
    };

    // Collect names of actions that have auto_fix enabled for this trigger.
    let auto_fix_actions: Vec<String> = config
        .actions_for_trigger(trigger)
        .into_iter()
        .filter(|(_, def)| def.auto_fix && def.fix_command.is_some())
        .map(|(name, _)| name.to_owned())
        .collect();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let runner = rt.block_on(ActionRunner::new_with_docker_probe(repo_root, config));

    let mut results = rt.block_on(runner.run_trigger(trigger, changed_paths));

    // For actions that failed and have auto_fix enabled, run the fix command
    // and then re-run the check.
    for result in &mut results {
        if result.status != ActionStatus::Failed {
            continue;
        }
        if !auto_fix_actions.contains(&result.name) {
            continue;
        }

        eprintln!("  \x1b[33m→ auto-fixing: {}\x1b[0m", result.display_name);

        // Run the fix command.
        let fix_result = rt.block_on(runner.run_action_fix(&result.name));
        if let Ok(fr) = fix_result
            && fr.status == ActionStatus::Passed
        {
            // Fix succeeded — re-run the check.
            if let Ok(recheck_result) = rt.block_on(runner.run_action(&result.name)) {
                *result = recheck_result;
            }
        }
    }

    Ok(results)
}

/// Check branch protection rules for a target branch.
///
/// Runs all `required_checks` defined in the branch protection rule, and returns
/// a list of violations (empty = all checks passed). Each violation is a
/// human-readable string describing the failure.
pub fn check_branch_protection(
    repo_root: &Path,
    target_branch: &str,
) -> ActionsResult<Vec<String>> {
    let Some(config) = ActionsConfig::load(repo_root)? else {
        return Ok(Vec::new());
    };

    let Some(rule) = config.protection_for_branch(target_branch).cloned() else {
        return Ok(Vec::new());
    };

    let mut violations = Vec::new();

    // Run required checks.
    if !rule.required_checks.is_empty() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        let runner = rt.block_on(ActionRunner::new_with_docker_probe(repo_root, config));

        for check_name in &rule.required_checks {
            match rt.block_on(runner.run_action(check_name)) {
                Ok(result) => {
                    if matches!(
                        result.status,
                        ActionStatus::Failed | ActionStatus::TimedOut | ActionStatus::Error
                    ) {
                        violations.push(format!(
                            "required check '{}' {}: {}",
                            result.display_name,
                            result.status,
                            result.stderr.lines().next().unwrap_or("(no output)")
                        ));
                    }
                }
                Err(e) => {
                    violations.push(format!("required check '{check_name}' error: {e}"));
                }
            }
        }
    }

    if rule.require_pull_request {
        violations.push("branch protection requires merging via pull request".to_owned());
    }

    Ok(violations)
}

/// Returns the branch protection rule for a branch, if any.
pub fn get_branch_protection(
    repo_root: &Path,
    branch: &str,
) -> ActionsResult<Option<crate::config::BranchProtectionRule>> {
    let Some(config) = ActionsConfig::load(repo_root)? else {
        return Ok(None);
    };
    Ok(config.protection_for_branch(branch).cloned())
}
