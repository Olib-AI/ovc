//! Action execution engine — runs shell commands and built-in actions.
//!
//! Supports parallel execution via DAG-based dependency ordering, matrix
//! strategy for parameterized runs, retry logic, output capture, and
//! secrets injection.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::Path;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::builtin::run_builtin;
use crate::config::{ActionCategory, ActionCondition, ActionsConfig, Trigger};
use crate::docker::{self, DockerAvailability};
use crate::error::{ActionsError, ActionsResult};
use crate::secrets::SecretsVault;

/// Result of running a single action.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActionResult {
    /// Action key name.
    pub name: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Action category.
    pub category: String,
    /// Outcome status.
    pub status: ActionStatus,
    /// Process exit code (if applicable).
    pub exit_code: Option<i32>,
    /// Captured stdout.
    pub stdout: String,
    /// Captured stderr.
    pub stderr: String,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// ISO-8601 start timestamp.
    pub started_at: String,
    /// ISO-8601 finish timestamp.
    pub finished_at: String,
    /// Whether this action is continue-on-error.
    pub continue_on_error: bool,
    /// Matrix variable values for this specific run (if matrix action).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix_values: Option<BTreeMap<String, String>>,
    /// Captured output variables.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, String>,
    /// Whether Docker was used for execution.
    #[serde(default)]
    pub docker_used: bool,
    /// Retry attempt number (1-based).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub attempt: u32,
}

/// Helper for conditional serde serialization.
/// Serde `skip_serializing_if` requires `&T` signature — cannot pass by value.
#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

/// Status of an action execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ActionStatus {
    Passed,
    Failed,
    #[default]
    Skipped,
    TimedOut,
    Error,
}

impl std::fmt::Display for ActionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Passed => write!(f, "passed"),
            Self::Failed => write!(f, "failed"),
            Self::Skipped => write!(f, "skipped"),
            Self::TimedOut => write!(f, "timed_out"),
            Self::Error => write!(f, "error"),
        }
    }
}

/// Executes actions defined in an [`ActionsConfig`].
pub struct ActionRunner {
    repo_root: std::path::PathBuf,
    config: ActionsConfig,
    secrets: SecretsVault,
    /// Cached Docker availability probe result.
    docker_availability: Option<DockerAvailability>,
    /// Whether `ensure_image` has already been called in this runner's lifetime.
    /// Avoids redundant Docker image checks when running multiple actions.
    docker_image_checked: std::sync::atomic::AtomicBool,
}

/// Convert a Duration to milliseconds, saturating at `u64::MAX`.
fn saturating_millis(d: &std::time::Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

impl ActionRunner {
    /// Create a new runner with the given repo root and configuration.
    ///
    /// Loads the secrets vault from `.ovc/secrets.enc` if it exists.
    /// Does **not** probe Docker — use [`Self::new_with_docker_probe`] if
    /// Docker execution may be needed.
    #[must_use]
    pub fn new(repo_root: &Path, config: ActionsConfig) -> Self {
        let secrets = SecretsVault::load(repo_root).unwrap_or_default();
        Self {
            repo_root: repo_root.to_path_buf(),
            config,
            secrets,
            docker_availability: None,
            docker_image_checked: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Create a new runner, eagerly probing Docker availability.
    ///
    /// If `defaults.docker.enabled` is `true`, this probes the Docker daemon
    /// once and caches the result. If Docker is unavailable, a warning is
    /// emitted to stderr and actions will fall back to native execution.
    pub async fn new_with_docker_probe(repo_root: &Path, config: ActionsConfig) -> Self {
        let secrets = SecretsVault::load(repo_root).unwrap_or_default();
        let any_action_forces_docker = config
            .actions
            .values()
            .any(|def| def.docker_override == Some(true));
        let docker_availability = if config.defaults.docker.enabled || any_action_forces_docker {
            let avail = docker::probe_docker().await;
            if !avail.available {
                eprintln!(
                    "  \x1b[33m⚠ Docker enabled but unavailable: {}. Falling back to native execution.\x1b[0m",
                    avail.reason.as_deref().unwrap_or("unknown reason")
                );
            }
            Some(avail)
        } else {
            None
        };
        Self {
            repo_root: repo_root.to_path_buf(),
            config,
            secrets,
            docker_availability,
            docker_image_checked: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Reference to the loaded config.
    #[must_use]
    pub const fn config(&self) -> &ActionsConfig {
        &self.config
    }

    /// Run a single action by name.
    pub async fn run_action(&self, name: &str) -> ActionsResult<ActionResult> {
        let def = self
            .config
            .actions
            .get(name)
            .ok_or_else(|| ActionsError::ActionNotFound {
                name: name.to_owned(),
            })?;

        let display_name = def.display_name.as_deref().unwrap_or(name).to_owned();

        // Built-in actions
        if let Some(builtin) = def.builtin {
            return run_builtin(
                builtin,
                &def.config,
                &self.repo_root,
                &[],
                name,
                &display_name,
                def.continue_on_error,
            );
        }

        let command = def.command.as_deref().ok_or_else(|| ActionsError::Config {
            reason: format!("action '{name}' has no command"),
        })?;

        self.execute_command(
            name,
            &display_name,
            command,
            def.category,
            def.continue_on_error,
            &def.env,
            def.timeout,
            def.working_dir.as_deref(),
            &BTreeMap::new(),
            def.docker_override,
        )
        .await
    }

    /// Run a single action using its `fix_command` (if available).
    pub async fn run_action_fix(&self, name: &str) -> ActionsResult<ActionResult> {
        let def = self
            .config
            .actions
            .get(name)
            .ok_or_else(|| ActionsError::ActionNotFound {
                name: name.to_owned(),
            })?;

        let display_name = def.display_name.as_deref().unwrap_or(name).to_owned();

        let command = def
            .fix_command
            .as_deref()
            .or(def.command.as_deref())
            .ok_or_else(|| ActionsError::Config {
                reason: format!("action '{name}' has no fix_command or command"),
            })?;

        self.execute_command(
            name,
            &display_name,
            command,
            def.category,
            def.continue_on_error,
            &def.env,
            def.timeout,
            def.working_dir.as_deref(),
            &BTreeMap::new(),
            def.docker_override,
        )
        .await
    }

    /// Run all actions matching a trigger with parallel DAG-based execution.
    ///
    /// Actions are grouped into dependency levels via topological sort.
    /// Actions within the same level run concurrently. Matrix actions
    /// are expanded into multiple concurrent runs per combination.
    /// Failed actions are retried according to their `RetryConfig`.
    #[allow(clippy::too_many_lines)]
    pub async fn run_trigger(
        &self,
        trigger: Trigger,
        changed_paths: &[String],
    ) -> Vec<ActionResult> {
        let matching: Vec<(String, _)> = self
            .config
            .actions_for_trigger(trigger)
            .into_iter()
            .map(|(name, def)| (name.to_owned(), def.clone()))
            .collect();

        if matching.is_empty() {
            return Vec::new();
        }

        // Build the set of action names in this trigger group.
        let action_names: HashSet<&str> = matching.iter().map(|(n, _)| n.as_str()).collect();

        // Attempt topological sort. On cycle, fall back to sequential.
        let levels = topological_levels(&matching, &action_names).unwrap_or_else(|_| {
            // Cycle detected — fall back to sequential execution order.
            matching
                .iter()
                .map(|(name, _)| vec![name.as_str()])
                .collect()
        });

        // Index definitions by name for fast lookup.
        let def_map: HashMap<&str, _> = matching.iter().map(|(n, d)| (n.as_str(), d)).collect();

        let mut all_results: Vec<ActionResult> = Vec::new();
        // Accumulated outputs from completed actions, keyed by action name.
        let mut completed_outputs: HashMap<String, BTreeMap<String, String>> = HashMap::new();

        for level in &levels {
            let mut level_futures = Vec::new();

            for &action_name in level {
                let Some(def) = def_map.get(action_name) else {
                    continue;
                };

                // Check path condition.
                if let Some(ref condition) = def.condition
                    && !Self::matches_condition(condition, changed_paths)
                {
                    let now = chrono::Utc::now().to_rfc3339();
                    all_results.push(ActionResult {
                        name: action_name.to_owned(),
                        display_name: def
                            .display_name
                            .as_deref()
                            .unwrap_or(action_name)
                            .to_owned(),
                        category: def.category.to_string(),
                        status: ActionStatus::Skipped,
                        stderr: "condition not met: no matching changed paths".to_owned(),
                        started_at: now.clone(),
                        finished_at: now,
                        continue_on_error: def.continue_on_error,
                        ..ActionResult::default()
                    });
                    continue;
                }

                // Build extra env from dependency outputs.
                let mut extra_env = BTreeMap::new();
                for dep_name in &def.depends_on {
                    if let Some(outputs) = completed_outputs.get(dep_name.as_str()) {
                        for (k, v) in outputs {
                            extra_env.insert(
                                format!("OVC_OUTPUT_{dep_name}_{k}").to_uppercase(),
                                v.clone(),
                            );
                        }
                    }
                }

                // Matrix expansion.
                let matrix_combos = expand_matrix(def.matrix.as_ref());

                for combo in &matrix_combos {
                    let mut env_for_run = extra_env.clone();
                    for (k, v) in combo {
                        env_for_run.insert(format!("MATRIX_{}", k.to_uppercase()), v.clone());
                    }

                    let name_owned = action_name.to_owned();
                    let def_clone = (*def).clone();
                    let combo_clone = if combo.is_empty() {
                        None
                    } else {
                        Some(combo.clone())
                    };

                    level_futures.push(self.run_single_action_with_retry(
                        name_owned,
                        def_clone,
                        changed_paths,
                        env_for_run,
                        combo_clone,
                    ));
                }
            }

            if level_futures.is_empty() {
                continue;
            }

            let level_results = futures::future::join_all(level_futures).await;

            for result in level_results {
                // Store outputs for downstream actions.
                if !result.outputs.is_empty() {
                    completed_outputs
                        .entry(result.name.clone())
                        .or_default()
                        .extend(result.outputs.clone());
                }
                all_results.push(result);
            }
        }

        // Run on-fail actions if any action failed.
        let has_failures = all_results.iter().any(|r| {
            matches!(
                r.status,
                ActionStatus::Failed | ActionStatus::TimedOut | ActionStatus::Error
            )
        });
        if has_failures {
            let on_fail_actions: Vec<(String, _)> = self
                .config
                .actions_for_trigger(Trigger::OnFail)
                .into_iter()
                .map(|(name, def)| (name.to_owned(), def.clone()))
                .collect();

            if !on_fail_actions.is_empty() {
                // Build a summary of failures as env vars for on-fail actions.
                let failed_names: Vec<String> = all_results
                    .iter()
                    .filter(|r| {
                        matches!(
                            r.status,
                            ActionStatus::Failed | ActionStatus::TimedOut | ActionStatus::Error
                        )
                    })
                    .map(|r| r.name.clone())
                    .collect();

                for (name, def) in &on_fail_actions {
                    let mut extra_env = BTreeMap::new();
                    extra_env.insert("OVC_FAILED_ACTIONS".to_owned(), failed_names.join(","));
                    extra_env.insert("OVC_TRIGGER".to_owned(), trigger.to_string());

                    let result = self
                        .run_single_action_with_retry(
                            name.clone(),
                            def.clone(),
                            changed_paths,
                            extra_env,
                            None,
                        )
                        .await;
                    all_results.push(result);
                }
            }
        }

        all_results
    }

    /// Run a single action with retry logic, matrix env injection, and output capture.
    async fn run_single_action_with_retry(
        &self,
        name: String,
        def: crate::config::ActionDefinition,
        changed_paths: &[String],
        extra_env: BTreeMap<String, String>,
        matrix_values: Option<BTreeMap<String, String>>,
    ) -> ActionResult {
        let display_name = def.display_name.as_deref().unwrap_or(&name).to_owned();
        let max_attempts = def.retry.as_ref().map_or(1, |r| r.max_attempts.max(1));
        let retry_delay = def.retry.as_ref().map_or(0, |r| r.delay_secs);

        let mut last_result = None;

        for attempt in 1..=max_attempts {
            let result = if let Some(builtin) = def.builtin {
                match run_builtin(
                    builtin,
                    &def.config,
                    &self.repo_root,
                    changed_paths,
                    &name,
                    &display_name,
                    def.continue_on_error,
                ) {
                    Ok(mut r) => {
                        r.attempt = attempt;
                        r.matrix_values.clone_from(&matrix_values);
                        r
                    }
                    Err(e) => {
                        let mut r = error_result(
                            &name,
                            def.display_name.as_deref(),
                            def.category,
                            &e.to_string(),
                            def.continue_on_error,
                        );
                        r.attempt = attempt;
                        r.matrix_values.clone_from(&matrix_values);
                        r
                    }
                }
            } else if let Some(ref cmd) = def.command {
                // Substitute matrix variables in command: ${{ matrix.VAR }}
                let resolved_cmd = substitute_matrix_vars(cmd, matrix_values.as_ref());

                match self
                    .execute_command(
                        &name,
                        &display_name,
                        &resolved_cmd,
                        def.category,
                        def.continue_on_error,
                        &def.env,
                        def.timeout,
                        def.working_dir.as_deref(),
                        &extra_env,
                        def.docker_override,
                    )
                    .await
                {
                    Ok(mut r) => {
                        // Capture output variables.
                        let captured = capture_outputs(&r.stdout, &def.outputs);
                        r.outputs = captured;
                        r.attempt = attempt;
                        r.matrix_values.clone_from(&matrix_values);
                        r
                    }
                    Err(e) => {
                        let mut r = error_result(
                            &name,
                            def.display_name.as_deref(),
                            def.category,
                            &e.to_string(),
                            def.continue_on_error,
                        );
                        r.attempt = attempt;
                        r.matrix_values.clone_from(&matrix_values);
                        r
                    }
                }
            } else {
                let mut r = error_result(
                    &name,
                    def.display_name.as_deref(),
                    def.category,
                    "no command or builtin defined",
                    def.continue_on_error,
                );
                r.attempt = attempt;
                r.matrix_values.clone_from(&matrix_values);
                r
            };

            let passed = result.status == ActionStatus::Passed;
            last_result = Some(result);

            if passed || attempt == max_attempts {
                break;
            }

            // Wait before retrying.
            if retry_delay > 0 {
                tokio::time::sleep(std::time::Duration::from_secs(retry_delay)).await;
            }
        }

        // last_result is always Some because max_attempts >= 1.
        last_result.expect("invariant: at least one attempt executed")
    }

    /// Check if any changed path matches the condition's glob patterns.
    #[must_use]
    pub fn matches_condition(condition: &ActionCondition, changed_paths: &[String]) -> bool {
        if condition.paths.is_empty() {
            return true;
        }
        for pattern in &condition.paths {
            let Ok(glob) = globset::Glob::new(pattern) else {
                continue;
            };
            let matcher = glob.compile_matcher();
            for path in changed_paths {
                if matcher.is_match(path) {
                    return true;
                }
            }
        }
        false
    }

    /// Determine whether a shell command action should run inside Docker.
    fn should_use_docker(&self, docker_override: Option<bool>) -> bool {
        // Per-action override takes precedence.
        if docker_override == Some(false) {
            return false;
        }

        let globally_enabled = self.config.defaults.docker.enabled || docker_override == Some(true);
        if !globally_enabled {
            return false;
        }

        // Check Docker is actually available.
        self.docker_availability
            .as_ref()
            .is_some_and(|avail| avail.available)
    }

    /// Execute a shell command, dispatching to Docker or native based on config.
    #[allow(clippy::too_many_arguments)]
    async fn execute_command(
        &self,
        name: &str,
        display_name: &str,
        command: &str,
        category: ActionCategory,
        continue_on_error: bool,
        env: &BTreeMap<String, String>,
        timeout_override: Option<u64>,
        working_dir: Option<&str>,
        extra_env: &BTreeMap<String, String>,
        docker_override: Option<bool>,
    ) -> ActionsResult<ActionResult> {
        let shell = &self.config.defaults.shell;
        validate_shell(shell)?;

        let timeout_secs = timeout_override.unwrap_or(self.config.defaults.timeout);

        let work_dir = working_dir
            .or(self.config.defaults.working_dir.as_deref())
            .map_or_else(|| self.repo_root.clone(), |wd| self.repo_root.join(wd));

        validate_working_dir(&work_dir, &self.repo_root)?;

        if self.should_use_docker(docker_override) {
            self.execute_command_docker(
                name,
                display_name,
                command,
                category,
                continue_on_error,
                env,
                timeout_secs,
                &work_dir,
                extra_env,
            )
            .await
        } else {
            self.execute_command_native(
                name,
                display_name,
                command,
                category,
                continue_on_error,
                env,
                timeout_secs,
                &work_dir,
                extra_env,
            )
            .await
        }
    }

    /// Execute a shell command natively on the host.
    #[allow(clippy::too_many_arguments)]
    async fn execute_command_native(
        &self,
        name: &str,
        display_name: &str,
        command: &str,
        category: ActionCategory,
        continue_on_error: bool,
        env: &BTreeMap<String, String>,
        timeout_secs: u64,
        work_dir: &std::path::Path,
        extra_env: &BTreeMap<String, String>,
    ) -> ActionsResult<ActionResult> {
        let shell = &self.config.defaults.shell;

        let mut cmd = tokio::process::Command::new(shell);
        // Use login shell (-l) on macOS so the user's profile is sourced,
        // giving us the full PATH (e.g. ~/.cargo/bin, nvm, homebrew).
        // This is critical when the process runs under a macOS LaunchAgent
        // which only inherits a minimal PATH.
        #[cfg(target_os = "macos")]
        cmd.arg("-l");
        // Windows cmd.exe uses /c, PowerShell uses -Command, Unix shells use -c.
        if shell == "cmd" || shell == "cmd.exe" {
            cmd.arg("/c").arg(command);
        } else if shell.contains("powershell") || shell.contains("pwsh") {
            cmd.arg("-Command").arg(command);
        } else {
            cmd.arg("-c").arg(command);
        }
        cmd.current_dir(work_dir);

        // Enrich PATH with common tool directories so actions can find
        // cargo, npm, go, etc. even when launched from a daemon.
        let enriched_path = enrich_path();
        cmd.env("PATH", &enriched_path);

        // Inject default env vars.
        for (k, v) in &self.config.defaults.env {
            cmd.env(k, v);
        }
        // Inject action-level env vars.
        for (k, v) in env {
            cmd.env(k, v);
        }
        // Inject secrets as OVC_SECRET_* env vars.
        for (k, v) in &self.secrets.as_env_vars() {
            cmd.env(k, v);
        }
        // Inject extra env vars (matrix vars, dependency outputs).
        for (k, v) in extra_env {
            cmd.env(k, v);
        }

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => return Err(ActionsError::Io(e)),
        };

        await_child_with_timeout(
            child,
            name,
            display_name,
            category,
            continue_on_error,
            timeout_secs,
        )
        .await
    }

    /// Execute a shell command inside a Docker container.
    #[allow(clippy::too_many_arguments)]
    async fn execute_command_docker(
        &self,
        name: &str,
        display_name: &str,
        command: &str,
        category: ActionCategory,
        continue_on_error: bool,
        env: &BTreeMap<String, String>,
        timeout_secs: u64,
        work_dir: &std::path::Path,
        extra_env: &BTreeMap<String, String>,
    ) -> ActionsResult<ActionResult> {
        let docker_config = &self.config.defaults.docker;

        // Ensure the image is available (only on the first action).
        if !self
            .docker_image_checked
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            docker::ensure_image(&docker_config.image, &docker_config.pull_policy).await?;
            self.docker_image_checked
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }

        // Collect non-secret environment variables.
        let mut non_secret_env: Vec<(String, String)> = Vec::new();
        for (k, v) in &self.config.defaults.env {
            non_secret_env.push((k.clone(), v.clone()));
        }
        for (k, v) in env {
            non_secret_env.push((k.clone(), v.clone()));
        }
        for (k, v) in extra_env {
            non_secret_env.push((k.clone(), v.clone()));
        }

        // Collect secret environment variables separately so they are
        // passed via the parent process env rather than command-line args.
        let secret_env: Vec<(String, String)> = self.secrets.as_env_vars().into_iter().collect();

        // Generate a unique container name for identifiability.
        let short_uuid = &uuid::Uuid::new_v4().to_string()[..8];
        let container_name = format!("ovc-{short_uuid}");

        let params = docker::DockerRunParams {
            image: &docker_config.image,
            repo_root: &self.repo_root,
            work_dir,
            command,
            shell: &self.config.defaults.shell,
            env: non_secret_env,
            secret_env,
            container_name: &container_name,
            extra_flags: &docker_config.extra_flags,
        };

        let mut cmd = docker::build_docker_command(&params);
        let child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => return Err(ActionsError::Io(e)),
        };

        let mut result = await_child_with_timeout(
            child,
            name,
            display_name,
            category,
            continue_on_error,
            timeout_secs,
        )
        .await?;
        result.docker_used = true;
        Ok(result)
    }
}

/// Await a spawned child process with timeout, pipe reading, and result construction.
async fn await_child_with_timeout(
    mut child: tokio::process::Child,
    name: &str,
    display_name: &str,
    category: ActionCategory,
    continue_on_error: bool,
    timeout_secs: u64,
) -> ActionsResult<ActionResult> {
    let started_at = chrono::Utc::now();
    let start = Instant::now();

    // Take the pipe handles before waiting so we can read stdout/stderr
    // concurrently with waiting for exit. Reading after `child.wait()`
    // deadlocks when the child fills the OS pipe buffer.
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let combined_future = async {
        let (stdout_bytes, stderr_bytes, wait_res) =
            tokio::join!(read_pipe(stdout_pipe), read_pipe(stderr_pipe), child.wait(),);
        wait_res.map(|status| (stdout_bytes, stderr_bytes, status))
    };

    let timeout_dur = std::time::Duration::from_secs(timeout_secs);
    let wait_result = tokio::time::timeout(timeout_dur, combined_future).await;

    let elapsed = start.elapsed();
    let finished_at = chrono::Utc::now();

    match wait_result {
        Ok(Ok((stdout_bytes, stderr_bytes, exit_status))) => {
            let exit_code = exit_status.code().unwrap_or(-1);
            let status = if exit_status.success() {
                ActionStatus::Passed
            } else {
                ActionStatus::Failed
            };
            Ok(ActionResult {
                name: name.to_owned(),
                display_name: display_name.to_owned(),
                category: category.to_string(),
                status,
                exit_code: Some(exit_code),
                stdout: String::from_utf8_lossy(&stdout_bytes).into_owned(),
                stderr: String::from_utf8_lossy(&stderr_bytes).into_owned(),
                duration_ms: saturating_millis(&elapsed),
                started_at: started_at.to_rfc3339(),
                finished_at: finished_at.to_rfc3339(),
                continue_on_error,
                ..ActionResult::default()
            })
        }
        Ok(Err(e)) => Err(ActionsError::Io(e)),
        Err(_) => {
            // Timeout: kill the child process before returning.
            let _ = child.kill().await;
            Ok(ActionResult {
                name: name.to_owned(),
                display_name: display_name.to_owned(),
                category: category.to_string(),
                status: ActionStatus::TimedOut,
                exit_code: None,
                stdout: String::new(),
                stderr: format!("timed out after {timeout_secs}s"),
                duration_ms: saturating_millis(&elapsed),
                started_at: started_at.to_rfc3339(),
                finished_at: finished_at.to_rfc3339(),
                continue_on_error,
                ..ActionResult::default()
            })
        }
    }
}

/// Maximum bytes to read from a child process pipe (16 MiB).
///
/// Prevents a malicious or runaway action from exhausting memory by writing
/// unlimited data to stdout/stderr. Excess output is silently discarded.
const MAX_PIPE_BYTES: usize = 16 * 1024 * 1024;

/// Read up to [`MAX_PIPE_BYTES`] from an optional piped child stream.
///
/// If the child produces more output than the limit, the excess is consumed
/// and discarded to prevent the pipe from blocking the child, but only the
/// first [`MAX_PIPE_BYTES`] are retained.
async fn read_pipe<R: tokio::io::AsyncRead + Unpin>(pipe: Option<R>) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let Some(reader) = pipe else {
        return Vec::new();
    };
    let mut buf = Vec::with_capacity(8192);
    let limited = reader.take(MAX_PIPE_BYTES as u64);
    let mut limited = tokio::io::BufReader::new(limited);
    let _ = limited.read_to_end(&mut buf).await;
    buf
}

/// Shells that are allowed for action execution.
const ALLOWED_SHELLS: &[&str] = &[
    "/bin/sh",
    "/bin/bash",
    "sh",
    "bash",
    "/usr/bin/env",
    "cmd",
    "cmd.exe",
    "powershell",
    "powershell.exe",
    "pwsh",
    "pwsh.exe",
];

/// Validate that the configured shell is in the allowlist.
fn validate_shell(shell: &str) -> ActionsResult<()> {
    if ALLOWED_SHELLS.contains(&shell) {
        Ok(())
    } else {
        Err(ActionsError::Config {
            reason: format!(
                "shell '{shell}' is not allowed; permitted values: {}",
                ALLOWED_SHELLS.join(", ")
            ),
        })
    }
}

/// Validate that the resolved working directory does not escape the repo root.
fn validate_working_dir(
    work_dir: &std::path::Path,
    repo_root: &std::path::Path,
) -> ActionsResult<()> {
    let canon_root = repo_root
        .canonicalize()
        .map_err(|_| ActionsError::PathTraversal {
            path: repo_root.display().to_string(),
        })?;
    let canon_work = work_dir
        .canonicalize()
        .map_err(|_| ActionsError::PathTraversal {
            path: work_dir.display().to_string(),
        })?;

    if !canon_work.starts_with(&canon_root) {
        return Err(ActionsError::PathTraversal {
            path: work_dir.display().to_string(),
        });
    }
    Ok(())
}

/// Build an enriched PATH that includes common tool directories.
///
/// When OVC runs as a daemon (macOS `LaunchAgent`, Linux systemd service, etc.),
/// the process inherits a minimal PATH (`/usr/bin:/bin`). This function prepends
/// directories where `cargo`, `npm`, `go`, `python`, etc. are typically installed
/// so that actions can find them.
///
/// The directories are OS-aware: macOS-specific paths (Homebrew) are only included
/// on macOS, and so on.
fn enrich_path() -> String {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/home/user".to_owned());

    // Universal directories (all UNIX platforms).
    let mut extra_dirs: Vec<String> = vec![
        // Rust / Cargo
        format!("{home}/.cargo/bin"),
        // Local bin (pip --user, pipx)
        format!("{home}/.local/bin"),
        // Go
        format!("{home}/go/bin"),
        "/usr/local/go/bin".to_owned(),
        // Node version managers
        format!("{home}/.nvm/versions/node/default/bin"),
        format!("{home}/.volta/bin"),
        format!("{home}/.fnm/aliases/default/bin"),
        // Python (pyenv)
        format!("{home}/.pyenv/shims"),
        // Ruby (rbenv)
        format!("{home}/.rbenv/shims"),
        // Deno, Bun
        format!("{home}/.deno/bin"),
        format!("{home}/.bun/bin"),
    ];

    // macOS-specific: Homebrew directories.
    #[cfg(target_os = "macos")]
    {
        extra_dirs.push("/opt/homebrew/bin".to_owned());
        extra_dirs.push("/opt/homebrew/sbin".to_owned());
    }

    // Common system directories.
    extra_dirs.push("/usr/local/bin".to_owned());
    extra_dirs.push("/usr/local/sbin".to_owned());

    // Linux-specific: SDKMAN for Java/Kotlin.
    #[cfg(target_os = "linux")]
    {
        extra_dirs.push(format!("{home}/.sdkman/candidates/java/current/bin"));
        extra_dirs.push(format!("{home}/.sdkman/candidates/kotlin/current/bin"));
        extra_dirs.push(format!("{home}/.sdkman/candidates/gradle/current/bin"));
    }

    let current_path = std::env::var("PATH").unwrap_or_default();

    // Prepend extra dirs that actually exist on this machine.
    let mut parts: Vec<String> = extra_dirs
        .into_iter()
        .filter(|d| std::path::Path::new(d).is_dir())
        .collect();

    if current_path.is_empty() {
        parts.push("/usr/bin:/bin:/usr/sbin:/sbin".to_owned());
    } else {
        parts.push(current_path);
    }

    parts.join(":")
}

fn error_result(
    name: &str,
    display_name: Option<&str>,
    category: ActionCategory,
    error_msg: &str,
    continue_on_error: bool,
) -> ActionResult {
    let now = chrono::Utc::now().to_rfc3339();
    ActionResult {
        name: name.to_owned(),
        display_name: display_name.unwrap_or(name).to_owned(),
        category: category.to_string(),
        status: ActionStatus::Error,
        stderr: error_msg.to_owned(),
        started_at: now.clone(),
        finished_at: now,
        continue_on_error,
        ..ActionResult::default()
    }
}

/// Perform a topological sort of actions into dependency levels.
///
/// Returns a `Vec<Vec<&str>>` where each inner vec contains actions that
/// can run concurrently (all their dependencies are in earlier levels).
/// Returns `Err` if a cycle is detected.
fn topological_levels<'a>(
    actions: &'a [(String, crate::config::ActionDefinition)],
    valid_names: &HashSet<&str>,
) -> Result<Vec<Vec<&'a str>>, ActionsError> {
    let names: Vec<&str> = actions.iter().map(|(n, _)| n.as_str()).collect();
    let name_set: HashSet<&str> = names.iter().copied().collect();

    // Build adjacency: action -> set of dependencies (within this trigger group).
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    for (name, def) in actions {
        in_degree.entry(name.as_str()).or_insert(0);
        for dep in &def.depends_on {
            if valid_names.contains(dep.as_str()) && name_set.contains(dep.as_str()) {
                *in_degree.entry(name.as_str()).or_insert(0) += 1;
                dependents
                    .entry(dep.as_str())
                    .or_default()
                    .push(name.as_str());
            }
        }
    }

    let mut levels = Vec::new();
    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|&(_, deg)| *deg == 0)
        .map(|(&name, _)| name)
        .collect();

    let mut processed = 0usize;

    while !queue.is_empty() {
        let current_queue = std::mem::take(&mut queue);
        let level: Vec<&str> = current_queue.into_iter().collect();
        processed += level.len();

        for &action in &level {
            if let Some(deps) = dependents.get(action) {
                for &dep in deps {
                    if let Some(deg) = in_degree.get_mut(dep) {
                        *deg = deg.saturating_sub(1);
                        if *deg == 0 {
                            queue.push_back(dep);
                        }
                    }
                }
            }
        }

        levels.push(level);
    }

    if processed < names.len() {
        return Err(ActionsError::DependencyCycle {
            details: "cycle detected among action dependencies".to_owned(),
        });
    }

    Ok(levels)
}

/// Maximum number of matrix combinations allowed before aborting.
///
/// Prevents a denial of service where a matrix with many variables and
/// values produces a combinatorial explosion (e.g., 10 variables x 10
/// values = 10^10 combinations, each spawning a subprocess).
const MAX_MATRIX_COMBINATIONS: usize = 256;

/// Expand a matrix strategy into a list of variable combinations.
///
/// Each combination is a `BTreeMap<variable_name, value>`. If no matrix
/// is configured, returns a single empty map (one run with no matrix vars).
///
/// The total number of combinations is capped at [`MAX_MATRIX_COMBINATIONS`]
/// to prevent unbounded resource consumption from a crafted config.
fn expand_matrix(matrix: Option<&crate::config::MatrixStrategy>) -> Vec<BTreeMap<String, String>> {
    let Some(strategy) = matrix else {
        return vec![BTreeMap::new()];
    };

    if strategy.variables.is_empty() {
        return vec![BTreeMap::new()];
    }

    let vars: Vec<(&String, &Vec<String>)> = strategy.variables.iter().collect();
    let mut combos = vec![BTreeMap::new()];

    for (var_name, values) in vars {
        let mut new_combos = Vec::new();
        for combo in &combos {
            for val in values {
                let mut new = combo.clone();
                new.insert(var_name.clone(), val.clone());
                new_combos.push(new);
                if new_combos.len() > MAX_MATRIX_COMBINATIONS {
                    // Truncate to the limit and return immediately.
                    // This ensures we never allocate more than
                    // MAX_MATRIX_COMBINATIONS + 1 entries.
                    new_combos.truncate(MAX_MATRIX_COMBINATIONS);
                    return new_combos;
                }
            }
        }
        combos = new_combos;
    }

    combos
}

/// Substitute `${{ matrix.VAR }}` patterns in a command string.
fn substitute_matrix_vars(
    command: &str,
    matrix_values: Option<&BTreeMap<String, String>>,
) -> String {
    let Some(values) = matrix_values else {
        return command.to_owned();
    };

    let mut result = command.to_owned();
    for (key, val) in values {
        let pattern = format!("${{{{ matrix.{key} }}}}");
        result = result.replace(&pattern, val);
    }
    result
}

/// Capture output variables from stdout using configured regex patterns.
fn capture_outputs(
    stdout: &str,
    output_configs: &[crate::config::OutputCapture],
) -> BTreeMap<String, String> {
    let mut captured = BTreeMap::new();

    for output in output_configs {
        let Ok(re) = regex::Regex::new(&output.pattern) else {
            continue;
        };
        if let Some(caps) = re.captures(stdout) {
            // Use the first capture group if it exists, otherwise the whole match.
            let value = caps
                .get(1)
                .or_else(|| caps.get(0))
                .map_or("", |m| m.as_str());
            captured.insert(output.name.clone(), value.to_owned());
        }
    }

    captured
}
