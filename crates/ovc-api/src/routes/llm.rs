//! LLM-powered feature endpoints: commit message generation, PR review,
//! diff explanation, and PR description generation.
//!
//! All streaming endpoints use Server-Sent Events (SSE) to deliver incremental
//! LLM output to the frontend.

use std::convert::Infallible;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::{Stream, StreamExt};
use std::fmt::Write;

use serde::{Deserialize, Serialize};

use ovc_llm::{ContextBuilder, FileDiffEntry, LlmClient, PassPlan, resolve_config};

use crate::auth::Claims;
use crate::error::ApiError;
use crate::models::{DiffLineKind, DiffResponse, FileDiff};
use crate::routes::repos::open_repo_blocking;
use crate::state::AppState;

// ── Request / Response types ────────────────────────────────────────────

/// Request body for the explain-diff endpoint.
#[derive(Debug, Deserialize)]
pub struct ExplainDiffRequest {
    /// The diff text to explain.
    pub diff: String,
}

/// Response for the LLM config endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfigResponse {
    /// Whether LLM is configured and available at the server level.
    pub server_enabled: bool,
    /// Per-repo config (may be absent if not yet configured).
    #[serde(flatten)]
    pub repo_config: Option<ovc_core::config::LlmRepoConfig>,
}

/// Request body for updating LLM config.
#[derive(Debug, Deserialize)]
pub struct UpdateLlmConfigRequest {
    /// Override base URL for the OpenAI-compatible API.
    pub base_url: Option<String>,
    /// Override model name.
    pub model: Option<String>,
    /// Maximum context tokens.
    pub max_context_tokens: Option<usize>,
    /// Temperature for LLM sampling (0.0–2.0).
    pub temperature: Option<f32>,
    /// Per-feature toggles.
    pub enabled_features: Option<ovc_core::config::LlmFeatureToggles>,
}

/// Response for the health check endpoint.
#[derive(Debug, Serialize)]
pub struct LlmHealthResponse {
    /// Whether the server has LLM configured.
    pub configured: bool,
    /// Whether the LLM server is reachable.
    pub reachable: bool,
    /// The configured model name.
    pub model: Option<String>,
    /// The configured base URL.
    pub base_url: Option<String>,
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Resolves the effective LLM config for a repo, merging server defaults
/// with per-repo overrides.
async fn resolve_repo_llm_config(
    app: &AppState,
    repo_id: &str,
) -> Result<ovc_llm::ResolvedLlmConfig, ApiError> {
    let (repo, _) = open_repo_blocking(app, repo_id).await?;
    let repo_config = tokio::task::spawn_blocking(move || repo.config().llm.clone())
        .await
        .map_err(|e| {
            tracing::error!("task join error: {e}");
            ApiError::internal("internal task error")
        })?;

    resolve_config(&app.llm_server_config, repo_config.as_ref())
        .map_err(|e| ApiError::service_unavailable(&e.to_string()))
}

/// Converts a `DiffResponse` into a unified diff text string suitable for
/// sending to an LLM.
fn diff_response_to_text(diff: &DiffResponse) -> String {
    diff.files.iter().map(file_diff_to_text).collect()
}

/// Converts a single `FileDiff` into unified diff text.
fn file_diff_to_text(file: &FileDiff) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "--- a/{}\n+++ b/{}", file.path, file.path);
    for hunk in &file.hunks {
        let _ = writeln!(
            out,
            "@@ -{},{} +{},{} @@",
            hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
        );
        for line in &hunk.lines {
            let prefix = match line.kind {
                DiffLineKind::Context => ' ',
                DiffLineKind::Addition => '+',
                DiffLineKind::Deletion => '-',
            };
            out.push(prefix);
            out.push_str(&line.content);
            out.push('\n');
        }
    }
    out
}

/// Converts a `DiffResponse` into structured `FileDiffEntry` items for
/// the context builder's two-tier packing strategy.
fn diff_to_entries(diff: &DiffResponse) -> Vec<FileDiffEntry> {
    diff.files
        .iter()
        .map(|f| FileDiffEntry {
            path: f.path.clone(),
            status: f.status.clone(),
            additions: f.additions,
            deletions: f.deletions,
            diff_text: file_diff_to_text(f),
        })
        .collect()
}

/// Parses raw unified diff text into `FileDiffEntry` items by splitting on
/// file headers (`--- a/` / `+++ b/`).  Used by `explain_diff` which receives
/// raw diff text from the frontend rather than a structured `DiffResponse`.
fn parse_raw_diff_to_entries(diff: &str) -> Vec<FileDiffEntry> {
    let mut entries = Vec::new();
    let mut current_path = String::new();
    let mut current_text = String::new();
    let mut additions: u64 = 0;
    let mut deletions: u64 = 0;

    for line in diff.lines() {
        if line.starts_with("+++ b/") || line.starts_with("+++ ") {
            // Flush previous file.
            if !current_path.is_empty() {
                entries.push(FileDiffEntry {
                    path: current_path.clone(),
                    status: "modified".into(),
                    additions,
                    deletions,
                    diff_text: std::mem::take(&mut current_text),
                });
                additions = 0;
                deletions = 0;
            }
            line.strip_prefix("+++ b/")
                .or_else(|| line.strip_prefix("+++ "))
                .unwrap_or(line)
                .clone_into(&mut current_path);
        }
        if line.starts_with('+') && !line.starts_with("+++") {
            additions += 1;
        }
        if line.starts_with('-') && !line.starts_with("---") {
            deletions += 1;
        }
        current_text.push_str(line);
        current_text.push('\n');
    }

    // Flush last file.
    if !current_path.is_empty() {
        entries.push(FileDiffEntry {
            path: current_path,
            status: "modified".into(),
            additions,
            deletions,
            diff_text: current_text,
        });
    }

    // If no file headers found, treat entire diff as one entry.
    if entries.is_empty() && !diff.trim().is_empty() {
        entries.push(FileDiffEntry {
            path: "(diff)".into(),
            status: "modified".into(),
            additions: 0,
            deletions: 0,
            diff_text: diff.to_owned(),
        });
    }

    entries
}

/// Detects project languages from the working directory (if available).
fn detect_languages(app: &AppState, repo_id: &str) -> Vec<String> {
    let Some(workdir) = app.workdir_for(repo_id) else {
        return Vec::new();
    };
    ovc_actions::detect::detect_languages(&workdir)
        .languages
        .into_iter()
        .map(|l| l.language)
        .collect()
}

/// Creates an SSE stream that executes a multi-pass pipeline for any
/// LLM feature (commit msg, PR review, diff explanation).
///
///   - `reduce_fn` builds the final prompt from summaries + file manifest.
fn multipass_stream(
    client: LlmClient,
    plan: PassPlan,
    reduce_fn: impl FnOnce(&ContextBuilder, &[String], &[FileDiffEntry]) -> Vec<ovc_llm::ChatMessage>
    + Send
    + 'static,
    context: ContextBuilder,
) -> impl Stream<Item = Result<Event, Infallible>> + Send {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(16);

    tokio::spawn(async move {
        match plan {
            PassPlan::SinglePass(messages) => {
                run_single_pass(&client, messages, &tx).await;
            }
            PassPlan::MultiPass {
                batches,
                file_manifest,
            } => {
                run_multi_pass(&client, batches, &file_manifest, reduce_fn, &context, &tx).await;
            }
        }
    });

    futures::stream::unfold(rx, |mut rx| async {
        rx.recv().await.map(|item| (item, rx))
    })
}

/// Streams a single LLM request through the channel.
async fn run_single_pass(
    client: &LlmClient,
    messages: Vec<ovc_llm::ChatMessage>,
    tx: &tokio::sync::mpsc::Sender<Result<Event, Infallible>>,
) {
    match client.complete_streaming(messages).await {
        Ok(mut stream) => {
            while let Some(chunk_result) = stream.next().await {
                let event = match chunk_result {
                    Ok(chunk) if chunk.done => Event::default().event("done").data(""),
                    Ok(chunk) => Event::default().event("delta").data(chunk.delta),
                    Err(e) => {
                        tracing::warn!("LLM stream error: {e}");
                        let _ = tx
                            .send(Ok(Event::default().event("error").data(e.to_string())))
                            .await;
                        return;
                    }
                };
                if tx.send(Ok(event)).await.is_err() {
                    return; // Client disconnected.
                }
            }
            // Stream ended without explicit done — send one.
            let _ = tx.send(Ok(Event::default().event("done").data(""))).await;
        }
        Err(e) => {
            tracing::warn!("LLM stream start error: {e}");
            let _ = tx
                .send(Ok(Event::default().event("error").data(e.to_string())))
                .await;
        }
    }
}

/// Runs the multi-pass map-reduce pipeline through the channel.
///
/// `reduce_fn` receives (context, summaries, `file_manifest`) and returns
/// the final chat messages for the streaming reduce phase.
async fn run_multi_pass(
    client: &LlmClient,
    batches: Vec<ovc_llm::DiffBatch>,
    file_manifest: &[FileDiffEntry],
    reduce_fn: impl FnOnce(&ContextBuilder, &[String], &[FileDiffEntry]) -> Vec<ovc_llm::ChatMessage>,
    context: &ContextBuilder,
    tx: &tokio::sync::mpsc::Sender<Result<Event, Infallible>>,
) {
    let total_batches = batches.len();
    let mut summaries = Vec::with_capacity(total_batches);

    for (i, batch) in batches.into_iter().enumerate() {
        // Emit progress event.
        let progress = serde_json::json!({
            "phase": "analyzing",
            "batch": i + 1,
            "total": total_batches,
            "files": batch.paths,
        });
        if tx
            .send(Ok(Event::default()
                .event("progress")
                .data(progress.to_string())))
            .await
            .is_err()
        {
            return;
        }

        match client.complete(batch.messages).await {
            Ok(summary) => {
                tracing::info!(
                    "batch {}/{}: summarised {} files",
                    i + 1,
                    total_batches,
                    batch.file_count
                );
                summaries.push(summary);
            }
            Err(e) => {
                tracing::warn!("batch {}/{} failed: {e}", i + 1, total_batches);
                let _ = tx
                    .send(Ok(Event::default()
                        .event("error")
                        .data(format!("Batch {}/{total_batches} failed: {e}", i + 1))))
                    .await;
                return;
            }
        }
    }

    // Emit progress for final generation.
    let progress = serde_json::json!({ "phase": "generating" });
    let _ = tx
        .send(Ok(Event::default()
            .event("progress")
            .data(progress.to_string())))
        .await;

    // Build reduce-phase messages from summaries.
    let final_messages = reduce_fn(context, &summaries, file_manifest);

    // Stream the final response.
    run_single_pass(client, final_messages, tx).await;
}

// ── Handlers ────────────────────────────────────────────────────────────

/// Handler: `POST /api/v1/repos/:id/llm/generate-commit-msg`
///
/// Streams a suggested commit message based on the staged diff.
/// For large diffs, uses a multi-pass map-reduce pipeline: batches of
/// files are summarised first, then a final request generates the commit
/// message from the combined summaries.
pub async fn generate_commit_message(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let config = resolve_repo_llm_config(&app, &id).await?;

    if !config.features.commit_message {
        return Err(ApiError::service_unavailable(
            "commit message generation is disabled",
        ));
    }

    // Compute staged diff.
    let (repo, _) = open_repo_blocking(&app, &id).await?;
    let diff = tokio::task::spawn_blocking(move || -> Result<DiffResponse, ApiError> {
        crate::routes::commits::compute_index_diff(&repo)
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    if diff.files.is_empty() {
        return Err(ApiError::bad_request(
            "no staged changes to generate a commit message for",
        ));
    }

    let entries = diff_to_entries(&diff);
    let languages = detect_languages(&app, &id);
    let context = ContextBuilder::new(config.max_context_tokens);
    let plan = context.plan_commit_message(&entries, &languages);

    let client =
        LlmClient::new(config).map_err(|e| ApiError::service_unavailable(&e.to_string()))?;

    let stream = multipass_stream(
        client,
        plan,
        move |ctx, summaries, manifest| {
            ctx.for_commit_message_from_summaries(summaries, manifest, &languages)
        },
        context,
    );

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Handler: `POST /api/v1/repos/:id/llm/review-pr/:pr_number`
///
/// Streams an AI code review for a pull request's diff.
pub async fn review_pr(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, pr_number)): Path<(String, u64)>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let config = resolve_repo_llm_config(&app, &id).await?;

    if !config.features.pr_review {
        return Err(ApiError::service_unavailable("PR review is disabled"));
    }

    // Load the PR to get title, description, and branches.
    let (repo, _) = open_repo_blocking(&app, &id).await?;
    let (pr_title, pr_desc, diff) = tokio::task::spawn_blocking(
        move || -> Result<(String, String, DiffResponse), ApiError> {
            let pr_store = repo.pull_request_store();
            let pr = pr_store
                .get(pr_number)
                .ok_or_else(|| ApiError::not_found("pull request not found"))?;

            // Compute diff between source and target branches.
            let source_oid = repo
                .ref_store()
                .resolve(&format!("refs/heads/{}", pr.source_branch))
                .map_err(ApiError::from_core)?;
            let target_oid = repo
                .ref_store()
                .resolve(&format!("refs/heads/{}", pr.target_branch))
                .map_err(ApiError::from_core)?;

            let source_tree = match repo.get_object(&source_oid).map_err(ApiError::from_core)? {
                Some(ovc_core::object::Object::Commit(c)) => c.tree,
                _ => return Err(ApiError::internal("source branch has no commit")),
            };
            let target_tree = match repo.get_object(&target_oid).map_err(ApiError::from_core)? {
                Some(ovc_core::object::Object::Commit(c)) => c.tree,
                _ => return Err(ApiError::internal("target branch has no commit")),
            };

            let mut source_index = ovc_core::index::Index::new();
            source_index
                .read_tree(&source_tree, repo.object_store())
                .map_err(ApiError::from_core)?;
            let mut target_index = ovc_core::index::Index::new();
            target_index
                .read_tree(&target_tree, repo.object_store())
                .map_err(ApiError::from_core)?;

            let diff = crate::routes::commits::compute_diff_between_indices(
                &target_index,
                &source_index,
                &repo,
            )?;

            Ok((pr.title.clone(), pr.description.clone(), diff))
        },
    )
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    let entries = diff_to_entries(&diff);
    let context = ContextBuilder::new(config.max_context_tokens);
    let plan = context.plan_pr_review(&entries, &pr_title, &pr_desc);

    let client =
        LlmClient::new(config).map_err(|e| ApiError::service_unavailable(&e.to_string()))?;

    let stream = multipass_stream(
        client,
        plan,
        move |ctx, summaries, manifest| {
            ctx.reduce_from_summaries(
                summaries,
                manifest,
                &[],
                ovc_llm::prompts::PR_REVIEW_SYSTEM,
                &format!(
                    "Review these changes for PR \"{pr_title}\".\n\
                     PR Description: {pr_desc}"
                ),
            )
        },
        context,
    );

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Handler: `POST /api/v1/repos/:id/llm/explain-diff`
///
/// Streams an explanation of the provided diff text.
pub async fn explain_diff(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<ExplainDiffRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let config = resolve_repo_llm_config(&app, &id).await?;

    if !config.features.explain_diff {
        return Err(ApiError::service_unavailable(
            "diff explanation is disabled",
        ));
    }

    if req.diff.trim().is_empty() {
        return Err(ApiError::bad_request("diff text is empty"));
    }

    // Parse raw diff text into file entries for multi-pass support.
    let entries = parse_raw_diff_to_entries(&req.diff);
    let languages = detect_languages(&app, &id);
    let context = ContextBuilder::new(config.max_context_tokens);
    let plan = context.plan_explain_diff(&entries, &languages);

    let client =
        LlmClient::new(config).map_err(|e| ApiError::service_unavailable(&e.to_string()))?;

    let stream = multipass_stream(
        client,
        plan,
        move |ctx, summaries, manifest| {
            ctx.reduce_from_summaries(
                summaries,
                manifest,
                &languages,
                ovc_llm::prompts::EXPLAIN_DIFF_SYSTEM,
                "Explain all these changes in plain English.",
            )
        },
        context,
    );

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Completes a PR description using either a single-pass or multi-pass
/// pipeline depending on the `PassPlan`.
async fn complete_pr_description(
    client: &LlmClient,
    context: &ContextBuilder,
    plan: PassPlan,
    diff: &DiffResponse,
    commit_messages: &[String],
    languages: &[String],
) -> Result<String, ApiError> {
    match plan {
        PassPlan::SinglePass(_) => {
            // Small diff — build PR description prompt directly.
            let diff_summary = diff_response_to_text(diff);
            let chat_messages =
                context.for_pr_description(commit_messages, &diff_summary, languages);
            client
                .complete(chat_messages)
                .await
                .map_err(|e| ApiError::service_unavailable(&e.to_string()))
        }
        PassPlan::MultiPass {
            batches,
            file_manifest,
        } => {
            // Large diff — summarise batches first, then generate description.
            let batch_messages: Vec<Vec<ovc_llm::ChatMessage>> =
                batches.into_iter().map(|b| b.messages).collect();
            let summaries = client
                .complete_batches(batch_messages)
                .await
                .map_err(|e| ApiError::service_unavailable(&e.to_string()))?;

            let commits_text = commit_messages
                .iter()
                .map(|m| format!("- {m}"))
                .collect::<Vec<_>>()
                .join("\n");

            let final_instruction = format!(
                "Commits:\n{commits_text}\n\n\
                 Generate a PR description with: a summary paragraph, a bullet list of changes, \
                 and any notable caveats."
            );

            let final_messages = context.reduce_from_summaries(
                &summaries,
                &file_manifest,
                languages,
                ovc_llm::prompts::PR_DESC_SYSTEM,
                &final_instruction,
            );
            client
                .complete(final_messages)
                .await
                .map_err(|e| ApiError::service_unavailable(&e.to_string()))
        }
    }
}

/// Handler: `POST /api/v1/repos/:id/llm/generate-pr-description/:pr_number`
///
/// Generates a PR description from the PR's commits and diff summary.
/// Non-streaming — returns the full text as JSON.
pub async fn generate_pr_description(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, pr_number)): Path<(String, u64)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = resolve_repo_llm_config(&app, &id).await?;

    if !config.features.pr_description {
        return Err(ApiError::service_unavailable(
            "PR description generation is disabled",
        ));
    }

    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let (commit_messages, diff) =
        tokio::task::spawn_blocking(move || -> Result<(Vec<String>, DiffResponse), ApiError> {
            let pr_store = repo.pull_request_store();
            let pr = pr_store
                .get(pr_number)
                .ok_or_else(|| ApiError::not_found("pull request not found"))?;

            // Collect commit messages from source branch.
            let source_oid = repo
                .ref_store()
                .resolve(&format!("refs/heads/{}", pr.source_branch))
                .map_err(ApiError::from_core)?;
            let target_oid = repo
                .ref_store()
                .resolve(&format!("refs/heads/{}", pr.target_branch))
                .map_err(ApiError::from_core)?;

            let mut messages = Vec::new();
            let mut current = source_oid;
            for _ in 0..100 {
                if current == target_oid {
                    break;
                }
                match repo.get_object(&current).map_err(ApiError::from_core)? {
                    Some(ovc_core::object::Object::Commit(c)) => {
                        messages.push(c.message.clone());
                        if let Some(parent) = c.parents.first() {
                            current = *parent;
                        } else {
                            break;
                        }
                    }
                    _ => break,
                }
            }

            // Compute diff summary.
            let source_tree = match repo.get_object(&source_oid).map_err(ApiError::from_core)? {
                Some(ovc_core::object::Object::Commit(c)) => c.tree,
                _ => return Err(ApiError::internal("source branch has no commit")),
            };
            let target_tree = match repo.get_object(&target_oid).map_err(ApiError::from_core)? {
                Some(ovc_core::object::Object::Commit(c)) => c.tree,
                _ => return Err(ApiError::internal("target branch has no commit")),
            };
            let mut source_index = ovc_core::index::Index::new();
            source_index
                .read_tree(&source_tree, repo.object_store())
                .map_err(ApiError::from_core)?;
            let mut target_index = ovc_core::index::Index::new();
            target_index
                .read_tree(&target_tree, repo.object_store())
                .map_err(ApiError::from_core)?;
            let diff_obj = crate::routes::commits::compute_diff_between_indices(
                &target_index,
                &source_index,
                &repo,
            )?;

            Ok((messages, diff_obj))
        })
        .await
        .map_err(|e| {
            tracing::error!("task join error: {e}");
            ApiError::internal("internal task error")
        })??;

    let entries = diff_to_entries(&diff);
    let languages = detect_languages(&app, &id);
    let context = ContextBuilder::new(config.max_context_tokens);

    let client =
        LlmClient::new(config).map_err(|e| ApiError::service_unavailable(&e.to_string()))?;

    // Use the multi-pass plan to handle large PR diffs.
    let plan = context.plan_explain_diff(&entries, &[]);
    let description =
        complete_pr_description(&client, &context, plan, &diff, &commit_messages, &languages)
            .await?;

    Ok(Json(serde_json::json!({ "description": description })))
}

/// Handler: `GET /api/v1/repos/:id/llm/config`
///
/// Returns the per-repo LLM configuration merged with server-level status.
pub async fn get_llm_config(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<LlmConfigResponse>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;
    let repo_config = tokio::task::spawn_blocking(move || repo.config().llm.clone())
        .await
        .map_err(|e| {
            tracing::error!("task join error: {e}");
            ApiError::internal("internal task error")
        })?;

    Ok(Json(LlmConfigResponse {
        server_enabled: app.llm_server_config.enabled,
        repo_config,
    }))
}

/// Handler: `PUT /api/v1/repos/:id/llm/config`
///
/// Updates the per-repo LLM configuration.
pub async fn put_llm_config(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateLlmConfigRequest>,
) -> Result<Json<LlmConfigResponse>, ApiError> {
    let lock = app.repo_lock(&id);
    let _guard = lock.lock().await;

    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let server_enabled = app.llm_server_config.enabled;
    let resp = tokio::task::spawn_blocking(move || -> Result<LlmConfigResponse, ApiError> {
        let config = repo.config_mut();
        let llm = config
            .llm
            .get_or_insert_with(|| ovc_core::config::LlmRepoConfig {
                base_url: None,
                model: None,
                max_context_tokens: None,
                temperature: None,
                enabled_features: ovc_core::config::LlmFeatureToggles::default(),
            });

        if let Some(url) = req.base_url {
            llm.base_url = if url.is_empty() { None } else { Some(url) };
        }
        if let Some(model) = req.model {
            llm.model = if model.is_empty() { None } else { Some(model) };
        }
        if let Some(tokens) = req.max_context_tokens {
            llm.max_context_tokens = if tokens == 0 { None } else { Some(tokens) };
        }
        if let Some(temp) = req.temperature {
            llm.temperature = Some(temp);
        }
        if let Some(features) = req.enabled_features {
            llm.enabled_features = features;
        }

        let repo_config = config.llm.clone();
        repo.save().map_err(ApiError::from_core)?;

        Ok(LlmConfigResponse {
            server_enabled,
            repo_config,
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(resp))
}

/// Query parameters for the health endpoint.
#[derive(Debug, Deserialize)]
pub struct HealthQuery {
    /// Optional repo ID to include per-repo LLM config in the check.
    pub repo_id: Option<String>,
}

/// Handler: `GET /api/v1/llm/health`
///
/// Checks whether the LLM server is reachable. Accepts an optional `repo_id`
/// query parameter to also consider per-repo config.
pub async fn llm_health(
    State(app): State<Arc<AppState>>,
    Query(query): Query<HealthQuery>,
) -> Result<Json<LlmHealthResponse>, ApiError> {
    // Try to load per-repo config if a repo_id was provided.
    let repo_config = if let Some(ref repo_id) = query.repo_id {
        match open_repo_blocking(&app, repo_id).await {
            Ok((repo, _)) => tokio::task::spawn_blocking(move || repo.config().llm.clone())
                .await
                .ok()
                .flatten(),
            Err(_) => None,
        }
    } else {
        None
    };

    let Ok(resolved) = resolve_config(&app.llm_server_config, repo_config.as_ref()) else {
        return Ok(Json(LlmHealthResponse {
            configured: false,
            reachable: false,
            model: None,
            base_url: None,
        }));
    };

    let base_url = resolved.base_url.clone();
    let model = resolved.model.clone();

    let client =
        LlmClient::new(resolved).map_err(|e| ApiError::service_unavailable(&e.to_string()))?;

    let reachable = client.health_check().await.unwrap_or(false);

    Ok(Json(LlmHealthResponse {
        configured: true,
        reachable,
        model: Some(model),
        base_url: Some(base_url),
    }))
}
