//! Actions API endpoints — configuration, execution, detection, and history.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;

use ovc_actions::config::{ActionsConfig, Trigger};
use ovc_actions::detect::detect_languages;
use ovc_actions::history::{ActionHistory, ActionRunRecord};
use ovc_actions::runner::{ActionRunner, ActionStatus};
use ovc_actions::templates::generate_template;

use crate::auth::Claims;
use crate::error::ApiError;
use crate::models::{
    ActionHistoryListResponse, ActionInfo, ActionLastRun, ActionListResponse, ActionResultResponse,
    ActionRunSummary, DependencyProposal, DependencyProposalsResponse, DependencyReportResponse,
    DependencyStatusResponse, DependencyUpdateItem, DependencyUpdateRequest,
    DependencyUpdateResponse, DetectedLanguageInfo, DetectionResponse, InitActionsRequest,
    ManifestReportResponse, ProposalInfo, PutActionsConfigRequest, RunActionsRequest,
    RunActionsResponse, RunSingleActionRequest,
};
use crate::state::AppState;

/// Marker files that indicate a directory is a project working directory.
const PROJECT_MARKERS: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "go.mod",
    "pyproject.toml",
    "setup.py",
    "Makefile",
    "CMakeLists.txt",
    "build.gradle",
    "pom.xml",
    ".gitignore",
    "README.md",
    "README",
];

/// Derives the working directory (repo root) from the `.ovc` file path.
///
/// Strategies (in order):
/// 1. Check `AppState::workdir_for()` (populated from `OVC_WORKDIR_MAP`,
///    `OVC_WORKDIR_SCAN`, `.ovc-link` files, or `--workdir` flags at startup).
/// 2. Check if the parent directory of the `.ovc` file contains recognizable
///    project files — if so, the parent IS the working directory.
/// 3. Fall back to `<name>.ovc.d` as a last resort (legacy convention).
fn repo_working_dir(app: &AppState, repo_id: &str) -> std::path::PathBuf {
    // Strategy 1: Pre-configured workdir mapping.
    if let Some(workdir) = app.workdir_for(repo_id)
        && workdir.is_dir()
    {
        return workdir;
    }

    // Strategy 2: Parent of the .ovc file has project marker files.
    let ovc_path = app.repos_dir.join(format!("{repo_id}.ovc"));
    if let Some(parent) = ovc_path.parent() {
        for marker in PROJECT_MARKERS {
            if parent.join(marker).exists() {
                return parent.to_path_buf();
            }
        }
    }

    // Strategy 3: Legacy convention — `<name>.ovc.d` sibling directory.
    app.repos_dir.join(format!("{repo_id}.ovc.d"))
}

/// Validate repo id and ensure the `.ovc` file exists.
fn validate_repo_exists(app: &AppState, repo_id: &str) -> Result<(), ApiError> {
    // Reuse the validation from repos module (safe chars, no traversal).
    if repo_id.is_empty()
        || repo_id.contains('/')
        || repo_id.contains('\\')
        || repo_id.contains("..")
    {
        return Err(ApiError::bad_request("invalid repository id"));
    }
    if !repo_id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(ApiError::bad_request("invalid repository id"));
    }
    let ovc_path = app.repos_dir.join(format!("{repo_id}.ovc"));
    if !ovc_path.exists() {
        return Err(ApiError::not_found("repository not found"));
    }
    Ok(())
}

/// Load the actions config from the repo's working directory.
fn load_config(work_dir: &std::path::Path) -> Result<ActionsConfig, ApiError> {
    ActionsConfig::load(work_dir)
        .map_err(|e| ApiError::internal(&format!("failed to load actions config: {e}")))?
        .ok_or_else(|| {
            ApiError::not_found("no actions.yml found — call POST /actions/init to generate one")
        })
}

/// Parse a trigger string into a `Trigger` enum value.
fn parse_trigger(s: &str) -> Result<Trigger, ApiError> {
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
        _ => Err(ApiError::bad_request(&format!("unknown trigger: {s}"))),
    }
}

/// Convert an `ActionResult` into the API response type.
fn result_to_response(r: &ovc_actions::runner::ActionResult) -> ActionResultResponse {
    ActionResultResponse {
        name: r.name.clone(),
        display_name: r.display_name.clone(),
        category: r.category.clone(),
        status: r.status.to_string(),
        exit_code: r.exit_code,
        stdout: r.stdout.clone(),
        stderr: r.stderr.clone(),
        duration_ms: r.duration_ms,
        started_at: r.started_at.clone(),
        finished_at: r.finished_at.clone(),
        docker_used: r.docker_used,
    }
}

/// Compute the overall status from a collection of action results.
fn compute_overall_status(results: &[ovc_actions::runner::ActionResult]) -> String {
    if results
        .iter()
        .all(|r| r.status == ActionStatus::Passed || r.status == ActionStatus::Skipped)
    {
        "passed".to_owned()
    } else {
        "failed".to_owned()
    }
}

// ── Handlers ────────────────────────────────────────────────────────────

/// `GET /api/v1/repos/:id/actions/config`
///
/// Returns the raw YAML content from `.ovc/actions.yml` so the frontend
/// can display and edit it directly.
pub async fn get_config(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    validate_repo_exists(&app, &id)?;
    let work_dir = repo_working_dir(&app, &id);

    let result = tokio::task::spawn_blocking(move || -> Result<serde_json::Value, ApiError> {
        let config_path = work_dir.join(".ovc").join("actions.yml");
        if config_path.is_file() {
            let content = std::fs::read_to_string(&config_path)
                .map_err(|e| ApiError::internal(&format!("failed to read actions.yml: {e}")))?;
            Ok(serde_json::json!({ "content": content, "exists": true }))
        } else {
            Ok(serde_json::json!({ "content": "", "exists": false }))
        }
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(result))
}

/// `PUT /api/v1/repos/:id/actions/config`
///
/// Validates and writes new YAML content to `.ovc/actions.yml`.
pub async fn put_config(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<PutActionsConfigRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    validate_repo_exists(&app, &id)?;

    // Reject oversized payloads early. Actions config files should never
    // approach 1 MB; this prevents abuse via the global 16 MB body limit.
    if body.content.len() > 1_048_576 {
        return Err(ApiError::bad_request(
            "actions config content must not exceed 1 MB",
        ));
    }

    let work_dir = repo_working_dir(&app, &id);

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        // Validate that the content is syntactically valid YAML and
        // deserializes into our config schema.
        let config: ActionsConfig = serde_yaml::from_str(&body.content)
            .map_err(|e| ApiError::bad_request(&format!("invalid actions YAML: {e}")))?;

        // Run semantic validation (shell allowlist, path traversal checks,
        // docker extra_flags allowlist, etc.).
        let issues = config.validate();
        if !issues.is_empty() {
            return Err(ApiError::bad_request(&issues.join("\n")));
        }

        let ovc_dir = work_dir.join(".ovc");
        std::fs::create_dir_all(&ovc_dir)
            .map_err(|e| ApiError::internal(&format!("failed to create .ovc dir: {e}")))?;

        let config_path = ovc_dir.join("actions.yml");
        std::fs::write(&config_path, &body.content)
            .map_err(|e| ApiError::internal(&format!("failed to write actions.yml: {e}")))?;

        Ok(())
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(serde_json::json!({ "success": true })))
}

/// `GET /api/v1/repos/:id/actions/list`
///
/// Lists all configured actions with optional last-run information.
pub async fn list_actions(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ActionListResponse>, ApiError> {
    validate_repo_exists(&app, &id)?;
    let work_dir = repo_working_dir(&app, &id);

    let response = tokio::task::spawn_blocking(move || -> Result<ActionListResponse, ApiError> {
        let config = load_config(&work_dir)?;
        let history = ActionHistory::new(&work_dir);

        // Load only the 5 most recent runs to find last-run info per action.
        // This avoids reading dozens of history files while still covering the
        // common case where actions run regularly.
        let recent_runs = history
            .list_runs(5)
            .map_err(|e| ApiError::internal(&format!("failed to read history: {e}")))?;

        let mut actions = Vec::new();
        for (name, def) in &config.actions {
            let display_name = def.display_name.as_deref().unwrap_or(name).to_owned();

            // Find the most recent result for this action across runs.
            let last_run = recent_runs.iter().find_map(|run| {
                run.results
                    .iter()
                    .find(|r| r.name == *name)
                    .map(|r| ActionLastRun {
                        status: r.status.to_string(),
                        duration_ms: r.duration_ms,
                        timestamp: r.finished_at.clone(),
                    })
            });

            actions.push(ActionInfo {
                name: name.clone(),
                display_name,
                category: def.category.to_string(),
                language: def.language.clone(),
                tool: def.tool.clone(),
                triggers: vec![def.trigger.to_string()],
                last_run,
            });
        }

        Ok(ActionListResponse { actions })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(response))
}

/// `POST /api/v1/repos/:id/actions/run`
///
/// Runs actions matching the given trigger or specific named actions.
pub async fn run_actions(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<RunActionsRequest>,
) -> Result<Json<RunActionsResponse>, ApiError> {
    validate_repo_exists(&app, &id)?;
    let work_dir = repo_working_dir(&app, &id);

    let config = {
        let wd = work_dir.clone();
        tokio::task::spawn_blocking(move || load_config(&wd))
            .await
            .map_err(|e| {
                tracing::error!("task join error: {e}");
                ApiError::internal("internal task error")
            })??
    };

    let trigger_str = req.trigger.as_deref().unwrap_or("manual");
    let trigger = parse_trigger(trigger_str)?;
    let runner = ActionRunner::new_with_docker_probe(&work_dir, config).await;

    let start = std::time::Instant::now();
    let results = match &req.names {
        Some(names) if !names.is_empty() => {
            let mut results = Vec::new();
            for name in names {
                let result = if req.fix {
                    runner.run_action_fix(name).await
                } else {
                    runner.run_action(name).await
                };
                match result {
                    Ok(r) => results.push(r),
                    Err(e) => {
                        return Err(ApiError::bad_request(&format!(
                            "failed to run action '{name}': {e}"
                        )));
                    }
                }
            }
            results
        }
        _ => {
            let paths = req.changed_paths.unwrap_or_default();
            runner.run_trigger(trigger, &paths).await
        }
    };
    let total_duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    let overall_status = compute_overall_status(&results);
    let run_id = uuid::Uuid::new_v4().to_string();

    // Record to history.
    let record = ActionRunRecord {
        run_id: run_id.clone(),
        trigger: trigger_str.to_owned(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        results: results.clone(),
        overall_status: overall_status.clone(),
        total_duration_ms,
    };

    let wd_for_history = work_dir.clone();
    tokio::task::spawn_blocking(move || {
        let history = ActionHistory::new(&wd_for_history);
        if let Err(e) = history.record_run(&record) {
            tracing::warn!("failed to record action run history: {e}");
        }
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })?;

    let response = RunActionsResponse {
        run_id,
        trigger: trigger_str.to_owned(),
        overall_status,
        total_duration_ms,
        results: results.iter().map(result_to_response).collect(),
    };

    Ok(Json(response))
}

/// `POST /api/v1/repos/:id/actions/run/:name`
///
/// Runs a single action by name.
pub async fn run_single_action(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, name)): Path<(String, String)>,
    body: Option<Json<RunSingleActionRequest>>,
) -> Result<Json<RunActionsResponse>, ApiError> {
    validate_repo_exists(&app, &id)?;
    let work_dir = repo_working_dir(&app, &id);

    let fix = body.is_some_and(|b| b.fix);

    let config = {
        let wd = work_dir.clone();
        tokio::task::spawn_blocking(move || load_config(&wd))
            .await
            .map_err(|e| {
                tracing::error!("task join error: {e}");
                ApiError::internal("internal task error")
            })??
    };

    let runner = ActionRunner::new_with_docker_probe(&work_dir, config).await;

    let start = std::time::Instant::now();
    let result = if fix {
        runner.run_action_fix(&name).await
    } else {
        runner.run_action(&name).await
    }
    .map_err(|e| ApiError::bad_request(&format!("failed to run action '{name}': {e}")))?;
    let total_duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    let overall_status = compute_overall_status(std::slice::from_ref(&result));
    let run_id = uuid::Uuid::new_v4().to_string();

    // Record to history.
    let record = ActionRunRecord {
        run_id: run_id.clone(),
        trigger: "manual".to_owned(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        results: vec![result.clone()],
        overall_status: overall_status.clone(),
        total_duration_ms,
    };

    let wd_for_history = work_dir.clone();
    tokio::task::spawn_blocking(move || {
        let history = ActionHistory::new(&wd_for_history);
        if let Err(e) = history.record_run(&record) {
            tracing::warn!("failed to record action run history: {e}");
        }
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })?;

    let response = RunActionsResponse {
        run_id,
        trigger: "manual".to_owned(),
        overall_status,
        total_duration_ms,
        results: vec![result_to_response(&result)],
    };

    Ok(Json(response))
}

/// `GET /api/v1/repos/:id/actions/detect`
///
/// Detects languages and toolchains in the repository's working directory.
pub async fn detect(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DetectionResponse>, ApiError> {
    validate_repo_exists(&app, &id)?;
    let work_dir = repo_working_dir(&app, &id);

    let response = tokio::task::spawn_blocking(move || -> Result<DetectionResponse, ApiError> {
        let detection = detect_languages(&work_dir);

        let suggested_config = serde_json::to_value(&detection.suggested_config)
            .map_err(|e| ApiError::internal(&format!("serialization error: {e}")))?;

        Ok(DetectionResponse {
            languages: detection
                .languages
                .iter()
                .map(|l| DetectedLanguageInfo {
                    language: l.language.clone(),
                    confidence: l.confidence.to_string(),
                    marker_file: l.marker_file.clone(),
                    root_dir: l.root_dir.clone(),
                })
                .collect(),
            suggested_config,
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(response))
}

/// Query parameters for the history list endpoint.
#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    /// Maximum number of runs to return.
    pub limit: Option<usize>,
}

/// `GET /api/v1/repos/:id/actions/history`
///
/// Lists recent action run history.
pub async fn list_history(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<ActionHistoryListResponse>, ApiError> {
    validate_repo_exists(&app, &id)?;
    let work_dir = repo_working_dir(&app, &id);
    // Cap the limit to a reasonable maximum to prevent a single request
    // from scanning and deserializing thousands of history files.
    let limit = query.limit.unwrap_or(20).min(200);

    let response =
        tokio::task::spawn_blocking(move || -> Result<ActionHistoryListResponse, ApiError> {
            let history = ActionHistory::new(&work_dir);
            let runs = history
                .list_runs(limit)
                .map_err(|e| ApiError::internal(&format!("failed to read history: {e}")))?;

            let summaries = runs
                .iter()
                .map(|run| {
                    let passed_count = run
                        .results
                        .iter()
                        .filter(|r| r.status == ActionStatus::Passed)
                        .count();
                    let failed_count = run
                        .results
                        .iter()
                        .filter(|r| {
                            r.status == ActionStatus::Failed
                                || r.status == ActionStatus::Error
                                || r.status == ActionStatus::TimedOut
                        })
                        .count();

                    ActionRunSummary {
                        run_id: run.run_id.clone(),
                        trigger: run.trigger.clone(),
                        timestamp: run.timestamp.clone(),
                        overall_status: run.overall_status.clone(),
                        total_duration_ms: run.total_duration_ms,
                        action_count: run.results.len(),
                        passed_count,
                        failed_count,
                    }
                })
                .collect();

            Ok(ActionHistoryListResponse { runs: summaries })
        })
        .await
        .map_err(|e| {
            tracing::error!("task join error: {e}");
            ApiError::internal("internal task error")
        })??;

    Ok(Json(response))
}

/// `GET /api/v1/repos/:id/actions/history/:run_id`
///
/// Returns full details for a specific action run.
pub async fn get_run(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, run_id)): Path<(String, String)>,
) -> Result<Json<RunActionsResponse>, ApiError> {
    validate_repo_exists(&app, &id)?;
    let work_dir = repo_working_dir(&app, &id);

    let response = tokio::task::spawn_blocking(move || -> Result<RunActionsResponse, ApiError> {
        let history = ActionHistory::new(&work_dir);
        let record = history
            .get_run(&run_id)
            .map_err(|e| ApiError::internal(&format!("failed to read run: {e}")))?
            .ok_or_else(|| ApiError::not_found("run not found"))?;

        Ok(RunActionsResponse {
            run_id: record.run_id,
            trigger: record.trigger,
            overall_status: record.overall_status,
            total_duration_ms: record.total_duration_ms,
            results: record.results.iter().map(result_to_response).collect(),
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(response))
}

/// `DELETE /api/v1/repos/:id/actions/history`
///
/// Clears all action run history.
pub async fn clear_history(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    validate_repo_exists(&app, &id)?;
    let work_dir = repo_working_dir(&app, &id);

    let removed = tokio::task::spawn_blocking(move || -> Result<usize, ApiError> {
        let history = ActionHistory::new(&work_dir);
        history
            .clear()
            .map_err(|e| ApiError::internal(&format!("failed to clear history: {e}")))
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(serde_json::json!({ "removed": removed })))
}

/// `POST /api/v1/repos/:id/actions/init`
///
/// Detects languages, generates a starter config, and writes `.ovc/actions.yml`.
pub async fn init_config(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    body: Option<Json<InitActionsRequest>>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    validate_repo_exists(&app, &id)?;
    let work_dir = repo_working_dir(&app, &id);
    let force = body.is_some_and(|b| b.force);

    let config = tokio::task::spawn_blocking(move || -> Result<ActionsConfig, ApiError> {
        let config_path = work_dir.join(".ovc").join("actions.yml");

        if config_path.is_file() && !force {
            return Err(ApiError::conflict(
                "actions.yml already exists — use force: true to overwrite",
            ));
        }

        let detection = detect_languages(&work_dir);
        let config = generate_template(&detection.languages);

        let yaml = serde_yaml::to_string(&config)
            .map_err(|e| ApiError::internal(&format!("YAML serialization error: {e}")))?;

        // Ensure .ovc directory exists.
        std::fs::create_dir_all(work_dir.join(".ovc"))
            .map_err(|e| ApiError::internal(&format!("failed to create .ovc dir: {e}")))?;

        std::fs::write(&config_path, yaml)
            .map_err(|e| ApiError::internal(&format!("failed to write actions.yml: {e}")))?;

        Ok(config)
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    let json = serde_json::to_value(&config)
        .map_err(|e| ApiError::internal(&format!("serialization error: {e}")))?;

    Ok((axum::http::StatusCode::CREATED, Json(json)))
}

/// `GET /api/v1/repos/:id/dependencies`
///
/// Runs the dependency update check directly and returns a structured JSON
/// report. Equivalent to running the `dependency_update_check` built-in
/// action but returns machine-readable data instead of human-readable text.
///
/// Query parameters (all optional):
/// - `check_dev` — include dev dependencies (default `true`)
/// - `level` — minimum update level to include: `patch`, `minor`, `major` (default `minor`)
/// - `timeout_secs` — per-registry HTTP timeout in seconds (default `30`)
pub async fn get_dependencies(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<DependencyQuery>,
) -> Result<Json<DependencyReportResponse>, ApiError> {
    validate_repo_exists(&app, &id)?;
    let work_dir = repo_working_dir(&app, &id);

    // Build a serde_yaml config value from query parameters so we can
    // reuse `depcheck::build_report` without introducing a separate API.
    let mut config_map = serde_yaml::Mapping::new();
    if let Some(check_dev) = query.check_dev {
        config_map.insert(
            serde_yaml::Value::String("check_dev".to_owned()),
            serde_yaml::Value::Bool(check_dev),
        );
    }
    if let Some(ref level) = query.level {
        config_map.insert(
            serde_yaml::Value::String("level".to_owned()),
            serde_yaml::Value::String(level.clone()),
        );
    }
    if let Some(timeout) = query.timeout_secs {
        config_map.insert(
            serde_yaml::Value::String("timeout_secs".to_owned()),
            serde_yaml::Value::Number(serde_yaml::Number::from(timeout)),
        );
    }
    let config_value = serde_yaml::Value::Mapping(config_map);

    let report = ovc_actions::depcheck::build_report(&work_dir, &config_value).await;

    let response = DependencyReportResponse {
        total_updates: report.total_updates,
        major_updates: report.major_updates,
        minor_updates: report.minor_updates,
        patch_updates: report.patch_updates,
        manifests: report
            .manifests
            .into_iter()
            .map(|m| ManifestReportResponse {
                file: m.file,
                package_manager: m.package_manager,
                dependencies: m
                    .dependencies
                    .into_iter()
                    .map(|d| DependencyStatusResponse {
                        name: d.name,
                        current_version: d.current_version,
                        latest_version: d.latest_version,
                        update_type: d.update_type.label().to_owned(),
                        dev: d.dev,
                    })
                    .collect(),
            })
            .collect(),
    };

    Ok(Json(response))
}

/// Query parameters for the dependency check endpoint.
#[derive(Debug, serde::Deserialize)]
pub struct DependencyQuery {
    /// Include dev dependencies (default `true`).
    pub check_dev: Option<bool>,
    /// Minimum update severity: `patch`, `minor`, or `major` (default `minor`).
    pub level: Option<String>,
    /// Per-registry HTTP timeout in seconds (default `30`).
    pub timeout_secs: Option<u64>,
}

// ── Dependency auto-update (Dependabot) ─────────────────────────────────────

/// Sanitise a string so it is safe to embed inside a branch name.
///
/// Replaces every character that is not alphanumeric, `-`, `.`, or `_` with
/// `-`, collapses consecutive `-`, and strips leading/trailing `-`.
fn sanitise_for_branch(s: &str) -> String {
    let raw: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '.' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();

    // Collapse consecutive dashes and strip leading/trailing.
    let mut result = String::with_capacity(raw.len());
    let mut last_dash = true; // treat start as "just had a dash" to strip leading
    for c in raw.chars() {
        if c == '-' {
            if !last_dash {
                result.push('-');
            }
            last_dash = true;
        } else {
            result.push(c);
            last_dash = false;
        }
    }
    // Strip trailing dash.
    if result.ends_with('-') {
        result.truncate(result.len() - 1);
    }
    result
}

/// Derive the `deps/{pm}/{name}-{version}` branch name for a dependency update.
fn deps_branch_name(file: &str, dep_name: &str, version: &str) -> String {
    let base = file.rsplit(['/', '\\']).next().unwrap_or(file);
    let pm = match base {
        "Cargo.toml" => "cargo",
        "package.json" => "npm",
        "composer.json" => "composer",
        "requirements.txt" | "pyproject.toml" => "pypi",
        "go.mod" => "go",
        "Gemfile" => "rubygems",
        "pom.xml" => "maven",
        "pubspec.yaml" => "pub",
        "mix.exs" => "hex",
        "Podfile" => "cocoapods",
        name if name.ends_with(".csproj") => "nuget",
        _ => "deps",
    };

    let safe_name = sanitise_for_branch(dep_name);
    let safe_ver = sanitise_for_branch(version);
    format!("deps/{pm}/{safe_name}-{safe_ver}")
}

/// Parse a `deps/{pm}/{name}-{version}` branch name into `(dependency, file)`.
///
/// Returns `(Some(dependency_name), Some(manifest_file))` on success, or
/// `(None, None)` when the branch name does not match the expected pattern.
fn parse_proposal_branch(branch: &str) -> (Option<String>, Option<String>) {
    // Expected format: `deps/{pm}/{name}-{version}`
    let segments: Vec<&str> = branch.splitn(3, '/').collect();
    if segments.len() < 3 {
        return (None, None);
    }

    let pm = segments[1];
    let name_ver = segments[2];

    // The dependency name is everything before the last `-` (the version part).
    let dependency = name_ver
        .rfind('-')
        .map(|i| name_ver[..i].to_owned())
        .or_else(|| Some(name_ver.to_owned()));

    // Reverse-map the package manager abbreviation to a default manifest file.
    let file = match pm {
        "cargo" => Some("Cargo.toml".to_owned()),
        "npm" => Some("package.json".to_owned()),
        "composer" => Some("composer.json".to_owned()),
        "pypi" => Some("pyproject.toml".to_owned()),
        "go" => Some("go.mod".to_owned()),
        "rubygems" => Some("Gemfile".to_owned()),
        "maven" => Some("pom.xml".to_owned()),
        "pub" => Some("pubspec.yaml".to_owned()),
        "hex" => Some("mix.exs".to_owned()),
        "cocoapods" => Some("Podfile".to_owned()),
        "nuget" => Some(format!("{pm}.csproj")),
        _ => None,
    };

    (dependency, file)
}

/// Check whether a branch can merge cleanly into the target branch without
/// actually committing the merge.
///
/// Returns `(mergeable, conflict_files)`.
fn check_merge_cleanly(
    repo: &ovc_core::repository::Repository,
    update_branch: &str,
    target_branch: &str,
) -> (bool, Vec<String>) {
    // Resolve both branches.
    let their_ref = format!("refs/heads/{update_branch}");
    let our_ref = format!("refs/heads/{target_branch}");

    let Ok(their_oid) = repo.ref_store().resolve(&their_ref) else {
        return (false, vec!["could not resolve update branch".to_owned()]);
    };
    let Ok(our_oid) = repo.ref_store().resolve(&our_ref) else {
        return (false, vec!["could not resolve target branch".to_owned()]);
    };

    if our_oid == their_oid {
        return (true, Vec::new());
    }

    // Get the commit trees.
    let Some(ovc_core::object::Object::Commit(our_commit)) =
        repo.get_object(&our_oid).ok().flatten()
    else {
        return (false, vec!["target branch HEAD is not a commit".to_owned()]);
    };
    let Some(ovc_core::object::Object::Commit(their_commit)) =
        repo.get_object(&their_oid).ok().flatten()
    else {
        return (false, vec!["update branch HEAD is not a commit".to_owned()]);
    };

    // Find merge base (reuse the same logic as branches.rs).
    let base_oid = {
        let mut ancestors_a = std::collections::HashSet::new();
        let mut current = Some(our_oid);
        while let Some(oid) = current {
            if !ancestors_a.insert(oid) {
                break;
            }
            current = repo
                .get_object(&oid)
                .ok()
                .flatten()
                .and_then(|obj| match obj {
                    ovc_core::object::Object::Commit(c) => c.parents.first().copied(),
                    _ => None,
                });
        }
        let mut visited_b = std::collections::HashSet::new();
        let mut current = Some(their_oid);
        loop {
            let Some(oid) = current else { break None };
            if ancestors_a.contains(&oid) {
                break Some(oid);
            }
            if !visited_b.insert(oid) {
                break None;
            }
            current = repo
                .get_object(&oid)
                .ok()
                .flatten()
                .and_then(|obj| match obj {
                    ovc_core::object::Object::Commit(c) => c.parents.first().copied(),
                    _ => None,
                });
        }
    };

    // Build the base tree OID — use an empty tree if there is no common ancestor.
    // All work happens on a scratch clone of the store so the repo is unmodified.
    let mut scratch = repo.object_store().clone();

    let base_tree = if let Some(base) = base_oid
        && let Some(ovc_core::object::Object::Commit(c)) = repo.get_object(&base).ok().flatten()
    {
        c.tree
    } else if base_oid.is_some() {
        // base_oid was Some but did not resolve to a commit — fall back to empty tree.
        let empty = ovc_core::object::Object::Tree(ovc_core::object::Tree {
            entries: Vec::new(),
        });
        match scratch.insert(&empty) {
            Ok(oid) => oid,
            Err(_) => return (false, vec!["failed to insert empty tree".to_owned()]),
        }
    } else {
        // No common ancestor.
        let empty_tree = ovc_core::object::Object::Tree(ovc_core::object::Tree {
            entries: Vec::new(),
        });
        match scratch.insert(&empty_tree) {
            Ok(oid) => oid,
            Err(_) => return (false, vec!["failed to insert empty tree".to_owned()]),
        }
    };
    match ovc_core::merge::merge_trees(
        &base_tree,
        &our_commit.tree,
        &their_commit.tree,
        &mut scratch,
    ) {
        Ok(result) => {
            let conflict_files: Vec<String> =
                result.conflicts.iter().map(|c| c.path.clone()).collect();
            (conflict_files.is_empty(), conflict_files)
        }
        Err(_) => (false, vec!["merge tree computation failed".to_owned()]),
    }
}

/// Apply a single update: create branch, modify file, stage, commit.
///
/// Returns `(branch_name, from_version, update_type_label, error)`.
/// On success `error` is `None`.
fn apply_single_update(
    repo: &mut ovc_core::repository::Repository,
    work_dir: &std::path::Path,
    original_branch: &str,
    item: &DependencyUpdateItem,
) -> Result<(String, String, String), String> {
    use ovc_core::object::{FileMode, Identity};

    let branch = deps_branch_name(&item.file, &item.name, &item.new_version);

    // Skip if the branch already exists.
    let branch_ref = format!("refs/heads/{branch}");
    if repo.ref_store().resolve(&branch_ref).is_ok() {
        return Err(format!("branch '{branch}' already exists"));
    }

    // Read the manifest file from the working directory.
    let manifest_path = work_dir.join(&item.file);
    let content = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("cannot read '{}': {e}", item.file))?;

    // Apply the version substitution.
    let updated_content = ovc_actions::depcheck::update_manifest_version(
        &content,
        &item.file,
        &item.name,
        &item.new_version,
    )
    .ok_or_else(|| format!("dependency '{}' not found in '{}'", item.name, item.file))?;

    // Detect the from_version and update_type from the original content before
    // we switch branches — the manifest is still at the original state.
    let from_version = ovc_actions::depcheck::extract_version_pub(&content, &item.file, &item.name)
        .unwrap_or_default();
    let update_type = ovc_actions::depcheck::classify_update_pub(&from_version, &item.new_version);

    // Create the update branch at the current HEAD of the original branch.
    let orig_ref = format!("refs/heads/{original_branch}");
    let orig_oid = repo
        .ref_store()
        .resolve(&orig_ref)
        .map_err(|e| format!("cannot resolve original branch: {e}"))?;
    repo.create_branch_at(&branch, orig_oid)
        .map_err(|e| format!("cannot create branch '{branch}': {e}"))?;

    // Switch HEAD to the update branch.
    let new_head_ref = format!("refs/heads/{branch}");
    repo.ref_store_mut()
        .set_head(ovc_core::refs::RefTarget::Symbolic(new_head_ref.clone()));
    repo.set_head_ref(new_head_ref);

    // Load the branch's tree into the index.
    {
        // First, resolve the commit tree OID from the store (immutable borrow).
        let orig_tree = {
            let obj = repo
                .object_store()
                .get(&orig_oid)
                .map_err(|e| e.to_string())?;
            match obj {
                Some(ovc_core::object::Object::Commit(c)) => c.tree,
                _ => return Err("original branch HEAD is not a commit".to_owned()),
            }
        };
        // Now take mutable access to index + store for read_tree.
        let (index, store) = repo.index_and_store_mut();
        if let Err(e) = index.read_tree(&orig_tree, store) {
            return Err(format!("cannot read tree into index: {e}"));
        }
    }

    // Stage the modified manifest content.
    let rel_path = item.file.replace('\\', "/");
    let content_bytes = updated_content.as_bytes();
    {
        let (index, store) = repo.index_and_store_mut();
        index
            .stage_file(&rel_path, content_bytes, FileMode::Regular, store)
            .map_err(|e| format!("cannot stage '{}': {e}", item.file))?;
    }

    // Write the updated file to the working directory.
    std::fs::write(&manifest_path, updated_content.as_bytes())
        .map_err(|e| format!("cannot write '{}': {e}", item.file))?;

    // Commit the change.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    let author = Identity {
        name: repo.config().user_name.clone(),
        email: repo.config().user_email.clone(),
        timestamp: now,
        tz_offset_minutes: 0,
    };
    let commit_msg = format!(
        "deps: update {} to {}\n\nAutomated dependency update by OVC Dependabot.",
        item.name, item.new_version
    );
    repo.create_commit(&commit_msg, &author)
        .map_err(|e| format!("cannot create commit: {e}"))?;

    Ok((branch, from_version, update_type))
}

// extract_version_from_content is provided by ovc_actions::depcheck::extract_version_pub.

/// `POST /api/v1/repos/:id/dependencies/update`
///
/// For each requested update (or all outdated deps when the list is empty):
/// 1. Create a `deps/{pm}/{name}-{version}` branch at the current HEAD.
/// 2. Modify the manifest and commit on that branch.
/// 3. Check whether the branch merges cleanly into the original branch.
/// 4. Switch back to the original branch.
/// 5. Return a proposal for each update.
#[allow(clippy::too_many_lines)]
pub async fn update_dependencies(
    claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    body: Option<Json<DependencyUpdateRequest>>,
) -> Result<Json<DependencyUpdateResponse>, ApiError> {
    validate_repo_exists(&app, &id)?;
    let work_dir = repo_working_dir(&app, &id);

    // If the caller sent an empty / missing list, run the dep checker first
    // to discover all outdated dependencies.
    let explicit_updates: Vec<DependencyUpdateItem> = body.map(|b| b.0.updates).unwrap_or_default();

    let updates: Vec<DependencyUpdateItem> = if explicit_updates.is_empty() {
        // Build the report with default config (no user-facing options here).
        let wd = work_dir.clone();
        let config_value = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        let report = ovc_actions::depcheck::build_report(&wd, &config_value).await;

        report
            .manifests
            .into_iter()
            .flat_map(|manifest| {
                manifest.dependencies.into_iter().filter_map(move |dep| {
                    if matches!(
                        dep.update_type,
                        ovc_actions::depcheck::UpdateType::Major
                            | ovc_actions::depcheck::UpdateType::Minor
                            | ovc_actions::depcheck::UpdateType::Patch
                    ) {
                        Some(DependencyUpdateItem {
                            name: dep.name,
                            file: manifest.file.clone(),
                            new_version: dep.latest_version,
                        })
                    } else {
                        None
                    }
                })
            })
            .collect()
    } else {
        explicit_updates
    };

    if updates.is_empty() {
        return Ok(Json(DependencyUpdateResponse {
            proposals: Vec::new(),
            created: 0,
            mergeable: 0,
            conflicting: 0,
        }));
    }

    let repo_mtx = app.repo_lock(&id);
    let guard = repo_mtx.lock().await;
    let (mut repo, _) = crate::routes::repos::open_repo_blocking(&app, &id).await?;

    let work_dir_clone = work_dir.clone();

    let response =
        tokio::task::spawn_blocking(move || -> Result<DependencyUpdateResponse, ApiError> {
            // Capture the original branch so we can restore it afterwards.
            let original_branch = match repo.ref_store().head() {
                ovc_core::refs::RefTarget::Symbolic(s) => {
                    s.strip_prefix("refs/heads/").unwrap_or(s).to_owned()
                }
                ovc_core::refs::RefTarget::Direct(oid) => oid.to_string(),
            };

            let mut proposals: Vec<DependencyProposal> = Vec::new();

            for item in &updates {
                let branch = deps_branch_name(&item.file, &item.name, &item.new_version);

                match apply_single_update(&mut repo, &work_dir_clone, &original_branch, item) {
                    Ok((branch_name, from_version, update_type)) => {
                        // Check mergeability against the original branch (read-only).
                        let (mergeable, conflict_files) =
                            check_merge_cleanly(&repo, &branch_name, &original_branch);

                        proposals.push(DependencyProposal {
                            branch: branch_name,
                            dependency: item.name.clone(),
                            file: item.file.clone(),
                            from_version,
                            to_version: item.new_version.clone(),
                            update_type,
                            mergeable,
                            conflict_files,
                            error: None,
                        });
                    }
                    Err(e) => {
                        proposals.push(DependencyProposal {
                            branch,
                            dependency: item.name.clone(),
                            file: item.file.clone(),
                            from_version: String::new(),
                            to_version: item.new_version.clone(),
                            update_type: "unknown".to_owned(),
                            mergeable: false,
                            conflict_files: Vec::new(),
                            error: Some(e),
                        });
                    }
                }

                // Always restore HEAD to the original branch between iterations
                // so the next update branches off the correct base.
                let orig_ref = format!("refs/heads/{original_branch}");
                repo.ref_store_mut()
                    .set_head(ovc_core::refs::RefTarget::Symbolic(orig_ref.clone()));
                repo.set_head_ref(orig_ref.clone());

                // Restore the working directory file if it exists.
                let orig_commit_opt = repo
                    .ref_store()
                    .resolve(&format!("refs/heads/{original_branch}"))
                    .ok()
                    .and_then(|oid| repo.get_object(&oid).ok().flatten())
                    .and_then(|obj| match obj {
                        ovc_core::object::Object::Commit(c) => Some(c),
                        _ => None,
                    });
                if let Some(orig_commit) = orig_commit_opt {
                    let mut idx = ovc_core::index::Index::new();
                    if idx
                        .read_tree(&orig_commit.tree, repo.object_store())
                        .is_ok()
                    {
                        let rel_path = item.file.replace('\\', "/");
                        if let Some(entry) = idx.entries().iter().find(|e| e.path == rel_path)
                            && let Ok(Some(ovc_core::object::Object::Blob(data))) =
                                repo.get_object(&entry.oid)
                        {
                            let _ = std::fs::write(work_dir_clone.join(&item.file), &data);
                        }
                        // Also reset the repo index to the original branch's tree.
                        let (index, store) = repo.index_and_store_mut();
                        let _ = index.read_tree(&orig_commit.tree, store);
                    }
                }
            }

            repo.save().map_err(ApiError::from_core)?;

            let created = proposals.iter().filter(|p| p.error.is_none()).count();
            let mergeable = proposals.iter().filter(|p| p.mergeable).count();
            let conflicting = proposals
                .iter()
                .filter(|p| p.error.is_none() && !p.mergeable)
                .count();

            Ok(DependencyUpdateResponse {
                proposals,
                created,
                mergeable,
                conflicting,
            })
        })
        .await
        .map_err(|e| {
            tracing::error!("task join error: {e}");
            ApiError::internal("internal task error")
        })??;

    // Auto-create PRs for each successfully created proposal.
    let successful_branches: Vec<String> = response
        .proposals
        .iter()
        .filter(|p| p.error.is_none())
        .map(|p| p.branch.clone())
        .collect();

    // Release the repo lock (drop guard) before creating PRs, since
    // PR creation needs its own repo access.
    drop(guard);

    for branch_name in &successful_branches {
        // Branch names only need `/` encoded for the path parameter.
        let encoded = branch_name.replace('/', "%2F");
        if let Err(e) = create_pr_from_proposal(
            claims.clone(),
            axum::extract::State(app.clone()),
            axum::extract::Path((id.clone(), encoded)),
        )
        .await
        {
            tracing::warn!("failed to auto-create PR for branch '{}': {e}", branch_name);
        }
    }

    Ok(Json(response))
}

/// `GET /api/v1/repos/:id/dependencies/proposals`
///
/// Lists all `deps/*` branches with their merge-readiness against the current
/// HEAD branch.
pub async fn list_proposals(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DependencyProposalsResponse>, ApiError> {
    validate_repo_exists(&app, &id)?;
    let (repo, _) = crate::routes::repos::open_repo_blocking(&app, &id).await?;

    let response =
        tokio::task::spawn_blocking(move || -> Result<DependencyProposalsResponse, ApiError> {
            let current_branch = match repo.ref_store().head() {
                ovc_core::refs::RefTarget::Symbolic(s) => {
                    s.strip_prefix("refs/heads/").unwrap_or(s).to_owned()
                }
                ovc_core::refs::RefTarget::Direct(oid) => oid.to_string(),
            };

            let mut proposals: Vec<ProposalInfo> = repo
                .ref_store()
                .list_branches()
                .into_iter()
                .filter(|(name, _)| name.starts_with("deps/"))
                .map(|(name, oid)| {
                    let (mergeable, conflict_files) =
                        check_merge_cleanly(&repo, name, &current_branch);

                    // Parse branch name to extract dependency and file.
                    // Branch names follow: `ovc-dep-update/{file}/{dependency}-{version}`
                    // or the shorter `deps/{manager}/{dependency}-{version}`.
                    let (dependency, file) = parse_proposal_branch(name);

                    ProposalInfo {
                        branch: name.to_owned(),
                        commit_id: oid.to_string(),
                        mergeable,
                        conflict_files,
                        dependency,
                        file,
                    }
                })
                .collect();

            proposals.sort_by(|a, b| a.branch.cmp(&b.branch));
            let total = proposals.len();

            Ok(DependencyProposalsResponse { proposals, total })
        })
        .await
        .map_err(|e| {
            tracing::error!("task join error: {e}");
            ApiError::internal("internal task error")
        })??;

    Ok(Json(response))
}

/// `DELETE /api/v1/repos/:id/dependencies/proposals/:branch`
///
/// Deletes a `deps/*` branch (rejects the update proposal).
pub async fn delete_proposal(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, branch)): Path<(String, String)>,
) -> Result<axum::http::StatusCode, ApiError> {
    // Ensure the branch is a deps/ branch to prevent accidental deletion of
    // important branches.
    let decoded = urlencoding_decode(&branch);
    if !decoded.starts_with("deps/") {
        return Err(ApiError::bad_request(
            "branch name must start with 'deps/' to use this endpoint",
        ));
    }

    validate_repo_exists(&app, &id)?;
    let repo_mtx = app.repo_lock(&id);
    let _guard = repo_mtx.lock().await;
    let (mut repo, _) = crate::routes::repos::open_repo_blocking(&app, &id).await?;

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        repo.delete_branch(&decoded).map_err(ApiError::from_core)?;
        repo.save().map_err(ApiError::from_core)?;
        Ok(())
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// `POST /api/v1/repos/:id/dependencies/proposals/:branch/merge`
///
/// Merges a `deps/*` branch into the current HEAD branch.
#[allow(clippy::too_many_lines)]
pub async fn merge_proposal(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, branch)): Path<(String, String)>,
) -> Result<Json<crate::models::MergeResponse>, ApiError> {
    let decoded = urlencoding_decode(&branch);
    if !decoded.starts_with("deps/") {
        return Err(ApiError::bad_request(
            "branch name must start with 'deps/' to use this endpoint",
        ));
    }

    validate_repo_exists(&app, &id)?;

    // Run pre-merge hooks and branch protection checks before acquiring the repo lock.
    {
        let work_dir = repo_working_dir(&app, &id);

        // Pre-merge hooks.
        let hook_results =
            ovc_actions::hooks::run_pre_merge_hooks(&work_dir, &[]).unwrap_or_default();
        if ovc_actions::hooks::has_blocking_failures(&hook_results) {
            let failures: Vec<String> = hook_results
                .iter()
                .filter(|r| {
                    !r.continue_on_error
                        && matches!(
                            r.status,
                            ovc_actions::runner::ActionStatus::Failed
                                | ovc_actions::runner::ActionStatus::TimedOut
                                | ovc_actions::runner::ActionStatus::Error
                        )
                })
                .map(|r| r.display_name.clone())
                .collect();
            return Err(ApiError::conflict(&format!(
                "Cannot merge dependency update: pre-merge checks failed: {}",
                failures.join(", ")
            )));
        }
    }

    let repo_mtx = app.repo_lock(&id);
    let _guard = repo_mtx.lock().await;
    let (mut repo, _) = crate::routes::repos::open_repo_blocking(&app, &id).await?;

    let response =
        tokio::task::spawn_blocking(move || -> Result<crate::models::MergeResponse, ApiError> {
            use ovc_core::object::{Identity, Object, Tree};

            let current_branch = match repo.ref_store().head() {
                ovc_core::refs::RefTarget::Symbolic(s) => {
                    s.strip_prefix("refs/heads/").unwrap_or(s).to_owned()
                }
                ovc_core::refs::RefTarget::Direct(_) => {
                    return Err(ApiError::bad_request(
                        "HEAD is detached — checkout a branch first",
                    ));
                }
            };

            let our_oid = repo
                .ref_store()
                .resolve_head()
                .map_err(ApiError::from_core)?;
            let their_ref = format!("refs/heads/{decoded}");
            let their_oid = repo
                .ref_store()
                .resolve(&their_ref)
                .map_err(|_| ApiError::not_found(&format!("branch '{decoded}' not found")))?;

            if our_oid == their_oid {
                return Ok(crate::models::MergeResponse {
                    status: "already_up_to_date".to_owned(),
                    commit_id: Some(our_oid.to_string()),
                    conflict_files: Vec::new(),
                    message: "Already up to date".to_owned(),
                });
            }

            let Some(Object::Commit(our_commit)) =
                repo.get_object(&our_oid).map_err(ApiError::from_core)?
            else {
                return Err(ApiError::internal("HEAD does not point to a commit"));
            };
            let Some(Object::Commit(their_commit)) =
                repo.get_object(&their_oid).map_err(ApiError::from_core)?
            else {
                return Err(ApiError::internal(
                    "update branch does not point to a commit",
                ));
            };

            // Find merge base.
            let base_tree = {
                let mut ancestors = std::collections::HashSet::new();
                let mut cur = Some(our_oid);
                while let Some(oid) = cur {
                    if !ancestors.insert(oid) {
                        break;
                    }
                    cur = repo.get_object(&oid).ok().flatten().and_then(|o| match o {
                        Object::Commit(c) => c.parents.first().copied(),
                        _ => None,
                    });
                }
                let mut visited = std::collections::HashSet::new();
                let mut cur = Some(their_oid);
                let base = loop {
                    let Some(oid) = cur else { break None };
                    if ancestors.contains(&oid) {
                        break Some(oid);
                    }
                    if !visited.insert(oid) {
                        break None;
                    }
                    cur = repo.get_object(&oid).ok().flatten().and_then(|o| match o {
                        Object::Commit(c) => c.parents.first().copied(),
                        _ => None,
                    });
                };

                if let Some(base_oid) = base {
                    match repo.get_object(&base_oid).map_err(ApiError::from_core)? {
                        Some(Object::Commit(c)) => c.tree,
                        _ => return Err(ApiError::internal("merge base is not a commit")),
                    }
                } else {
                    let empty = Object::Tree(Tree {
                        entries: Vec::new(),
                    });
                    repo.insert_object(&empty).map_err(ApiError::from_core)?
                }
            };

            let merge_result = ovc_core::merge::merge_trees(
                &base_tree,
                &our_commit.tree,
                &their_commit.tree,
                repo.object_store_mut(),
            )
            .map_err(ApiError::from_core)?;

            if !merge_result.conflicts.is_empty() {
                let conflict_paths: Vec<String> = merge_result
                    .conflicts
                    .iter()
                    .map(|c| c.path.clone())
                    .collect();
                let msg = format!(
                    "Merge conflict detected in {} file(s)",
                    conflict_paths.len()
                );
                return Ok(crate::models::MergeResponse {
                    status: "conflict".to_owned(),
                    commit_id: None,
                    conflict_files: conflict_paths,
                    message: msg,
                });
            }

            let merged_tree_oid = repo
                .insert_object(&Object::Tree(Tree {
                    entries: merge_result.entries,
                }))
                .map_err(ApiError::from_core)?;

            {
                let (index, store) = repo.index_and_store_mut();
                index
                    .read_tree(&merged_tree_oid, store)
                    .map_err(ApiError::from_core)?;
            }

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
            let author = Identity {
                name: repo.config().user_name.clone(),
                email: repo.config().user_email.clone(),
                timestamp: now,
                tz_offset_minutes: 0,
            };

            let msg = format!("Merge branch '{decoded}' into {current_branch}");
            let commit_oid = repo
                .create_commit(&msg, &author)
                .map_err(ApiError::from_core)?;

            repo.save().map_err(ApiError::from_core)?;

            Ok(crate::models::MergeResponse {
                status: "merged".to_owned(),
                commit_id: Some(commit_oid.to_string()),
                conflict_files: Vec::new(),
                message: "Merge completed successfully".to_owned(),
            })
        })
        .await
        .map_err(|e| {
            tracing::error!("task join error: {e}");
            ApiError::internal("internal task error")
        })??;

    Ok(Json(response))
}

/// `POST /api/v1/repos/:id/dependencies/proposals/:branch/create-pr`
///
/// Creates a pull request from a `deps/*` branch and runs CI checks.
pub async fn create_pr_from_proposal(
    claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, branch)): Path<(String, String)>,
) -> Result<(axum::http::StatusCode, Json<crate::models::PullRequest>), ApiError> {
    let decoded = urlencoding_decode(&branch);
    if !decoded.starts_with("deps/") {
        return Err(ApiError::bad_request(
            "branch name must start with 'deps/' to use this endpoint",
        ));
    }

    validate_repo_exists(&app, &id)?;

    // Extract dependency info from the branch name.
    let (dependency, file) = parse_proposal_branch(&decoded);
    let dep_name = dependency.unwrap_or_else(|| "unknown".to_owned());
    let file_name = file.unwrap_or_else(|| "unknown".to_owned());

    // Parse version from the branch name (last segment after final `-`).
    let version = decoded
        .rsplit('/')
        .next()
        .and_then(|seg| seg.rsplit_once('-').map(|(_, v)| v.to_owned()))
        .unwrap_or_else(|| "latest".to_owned());

    // Build a CreatePullRequestRequest and delegate to the PR create handler.
    let create_req = crate::models::CreatePullRequestRequest {
        title: format!("deps: update {dep_name} to {version}"),
        description: Some(format!(
            "Automated dependency update by OVC Dependabot.\n\nUpdates `{dep_name}` in `{file_name}`."
        )),
        source_branch: decoded.clone(),
        target_branch: None, // defaults to repo's default branch
        author: Some("ovc-dependabot".to_owned()),
    };

    // Reuse the full create_pull_request handler (includes CI checks).
    crate::routes::pulls::create_pull_request(
        claims,
        axum::extract::State(app),
        axum::extract::Path(id),
        axum::Json(create_req),
    )
    .await
}

// ── Secrets API ──────────────────────────────────────────────────────────

/// `GET /api/v1/repos/:id/actions/secrets`
///
/// Lists secret names (not values).
pub async fn list_secrets(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    validate_repo_exists(&app, &id)?;
    let work_dir = repo_working_dir(&app, &id);

    let result = tokio::task::spawn_blocking(move || -> Result<serde_json::Value, ApiError> {
        let vault = ovc_actions::secrets::SecretsVault::load(&work_dir)
            .map_err(|e| ApiError::internal(&format!("failed to load secrets: {e}")))?;
        let names: Vec<&str> = vault.list_names();
        Ok(serde_json::json!({ "names": names }))
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(result))
}

/// Request body for setting a secret.
#[derive(Debug, Clone, Deserialize)]
pub struct SetSecretRequest {
    /// The secret value.
    pub value: String,
}

/// `PUT /api/v1/repos/:id/actions/secrets/:name`
///
/// Sets a secret value.
pub async fn set_secret(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, name)): Path<(String, String)>,
    Json(body): Json<SetSecretRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    validate_repo_exists(&app, &id)?;

    // Validate secret name: alphanumeric + underscores only.
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(ApiError::bad_request(
            "secret name must be non-empty and contain only alphanumeric characters or underscores",
        ));
    }

    let work_dir = repo_working_dir(&app, &id);

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let mut vault = ovc_actions::secrets::SecretsVault::load(&work_dir)
            .map_err(|e| ApiError::internal(&format!("failed to load secrets: {e}")))?;
        vault.set(name, body.value);
        vault
            .save(&work_dir)
            .map_err(|e| ApiError::internal(&format!("failed to save secrets: {e}")))?;
        Ok(())
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `DELETE /api/v1/repos/:id/actions/secrets/:name`
///
/// Removes a secret.
pub async fn delete_secret(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    validate_repo_exists(&app, &id)?;
    let work_dir = repo_working_dir(&app, &id);

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let mut vault = ovc_actions::secrets::SecretsVault::load(&work_dir)
            .map_err(|e| ApiError::internal(&format!("failed to load secrets: {e}")))?;
        vault.remove(&name);
        vault
            .save(&work_dir)
            .map_err(|e| ApiError::internal(&format!("failed to save secrets: {e}")))?;
        Ok(())
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Docker status API ────────────────────────────────────────────────────

/// `GET /api/v1/repos/:id/actions/docker/status`
///
/// Probes Docker availability and returns status merged with config settings.
pub async fn docker_status(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    validate_repo_exists(&app, &id)?;
    let work_dir = repo_working_dir(&app, &id);

    // Load the config to get Docker settings (blocking FS read).
    // Fall back to defaults if no actions.yml exists — Docker status should
    // be queryable independently of the actions configuration.
    let docker_config = {
        let wd = work_dir.clone();
        tokio::task::spawn_blocking(
            move || -> Result<ovc_actions::config::DockerConfig, ApiError> {
                let config = ActionsConfig::load(&wd)
                    .map_err(|e| {
                        ApiError::internal(&format!("failed to load actions config: {e}"))
                    })?
                    .unwrap_or_default();
                Ok(config.defaults.docker)
            },
        )
        .await
        .map_err(|e| {
            tracing::error!("task join error: {e}");
            ApiError::internal("internal task error")
        })??
    };

    // Probe Docker (async network/process call).
    let avail = ovc_actions::docker::probe_docker().await;

    Ok(Json(serde_json::json!({
        "available": avail.available,
        "version": avail.version,
        "reason": avail.reason,
        "enabled": docker_config.enabled,
        "image": docker_config.image,
        "pull_policy": docker_config.pull_policy,
    })))
}

/// Percent-decode a URL path segment.
fn urlencoding_decode(s: &str) -> String {
    // A minimal percent-decoder: replace `%2F` → `/`, `%2f` → `/`.
    // Branch names in the path are already URL-encoded by the client for the
    // slash in `deps/cargo/name-version`.
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = bytes[i + 1];
            let lo = bytes[i + 2];
            if let (Some(h), Some(l)) = (hex_nibble(hi), hex_nibble(lo)) {
                out.push(char::from(h * 16 + l));
                i += 3;
                continue;
            }
        }
        out.push(char::from(bytes[i]));
        i += 1;
    }
    out
}

const fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ── Individual Action CRUD ──────────────────────────────────────────────

/// `GET /api/v1/repos/:id/actions/config/:name`
///
/// Returns the definition for a single named action from `.ovc/actions.yml`.
pub async fn get_action_config(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    validate_repo_exists(&app, &id)?;
    let work_dir = repo_working_dir(&app, &id);

    let result = tokio::task::spawn_blocking(move || -> Result<serde_json::Value, ApiError> {
        let config = load_config(&work_dir)?;
        let definition = config
            .actions
            .get(&name)
            .ok_or_else(|| ApiError::not_found(&format!("action '{name}' not found")))?;
        let value = serde_json::to_value(definition)
            .map_err(|e| ApiError::internal(&format!("serialization error: {e}")))?;
        Ok(serde_json::json!({ "name": name, "definition": value }))
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(result))
}

/// Request body for creating or updating a single action definition.
#[derive(Debug, Clone, Deserialize)]
pub struct PutActionDefinitionRequest {
    /// The action definition fields.
    #[serde(flatten)]
    pub definition: ovc_actions::config::ActionDefinition,
}

/// `PUT /api/v1/repos/:id/actions/config/:name`
///
/// Creates or updates a single action definition in `.ovc/actions.yml`.
/// The action name is taken from the URL path.
pub async fn put_action_config(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, name)): Path<(String, String)>,
    Json(body): Json<PutActionDefinitionRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    validate_repo_exists(&app, &id)?;

    // Validate action name: non-empty, reasonable length, safe characters.
    if name.is_empty() || name.len() > 128 {
        return Err(ApiError::bad_request(
            "action name must be 1-128 characters",
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(ApiError::bad_request(
            "action name must contain only alphanumeric characters, hyphens, or underscores",
        ));
    }

    let work_dir = repo_working_dir(&app, &id);

    let created = tokio::task::spawn_blocking(move || -> Result<bool, ApiError> {
        let mut config = match ActionsConfig::load(&work_dir) {
            Ok(Some(c)) => c,
            Ok(None) => ActionsConfig::default(),
            Err(e) => {
                return Err(ApiError::internal(&format!(
                    "failed to load actions config: {e}"
                )));
            }
        };

        let is_new = !config.actions.contains_key(&name);
        config.actions.insert(name, body.definition);

        let yaml = serde_yaml::to_string(&config)
            .map_err(|e| ApiError::internal(&format!("YAML serialization error: {e}")))?;

        let ovc_dir = work_dir.join(".ovc");
        std::fs::create_dir_all(&ovc_dir)
            .map_err(|e| ApiError::internal(&format!("failed to create .ovc dir: {e}")))?;
        std::fs::write(ovc_dir.join("actions.yml"), yaml)
            .map_err(|e| ApiError::internal(&format!("failed to write actions.yml: {e}")))?;

        Ok(is_new)
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(
        serde_json::json!({ "success": true, "created": created }),
    ))
}

/// `DELETE /api/v1/repos/:id/actions/config/:name`
///
/// Removes a single action definition from `.ovc/actions.yml`.
pub async fn delete_action_config(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    validate_repo_exists(&app, &id)?;
    let work_dir = repo_working_dir(&app, &id);

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let mut config = load_config(&work_dir)?;

        if config.actions.remove(&name).is_none() {
            return Err(ApiError::not_found(&format!("action '{name}' not found")));
        }

        let yaml = serde_yaml::to_string(&config)
            .map_err(|e| ApiError::internal(&format!("YAML serialization error: {e}")))?;

        let config_path = work_dir.join(".ovc").join("actions.yml");
        std::fs::write(&config_path, yaml)
            .map_err(|e| ApiError::internal(&format!("failed to write actions.yml: {e}")))?;

        Ok(())
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(serde_json::json!({ "success": true })))
}
