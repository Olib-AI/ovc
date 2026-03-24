//! Pull-request / branch-comparison endpoint **and** PR lifecycle endpoints.
//!
//! `GET /api/v1/repos/:id/pulls/:branch` returns a [`PullRequestView`]
//! describing how `:branch` compares against the repository's default branch.
//!
//! The lifecycle endpoints (`list`, `create`, `get`, `update`, `merge`) persist
//! PR metadata inside the encrypted superblock via
//! [`ovc_core::pulls::PullRequestStore`].

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as AxPath, Query, State};

use ovc_actions::config::{ActionsConfig, Trigger};
use ovc_actions::runner::{ActionRunner, ActionStatus};
use ovc_core::pulls::{PrCheckResult, PrChecks, PrComment, PrState, PullRequest, Review};

use crate::auth::Claims;
use crate::error::ApiError;
use crate::models::{
    CommitAuthor, CommitInfo, CreateCommentRequest, CreatePullRequestRequest, CreateReviewRequest,
    DiffResponse, ListPullRequestsQuery, MergePullRequestRequest, MergePullRequestResponse,
    PullRequestSummary, PullRequestView, UpdateCommentRequest, UpdatePullRequestRequest,
};
use crate::routes::commits::format_timestamp;
use crate::routes::repos::open_repo_blocking;
use crate::state::AppState;

/// Handler: `GET /api/v1/repos/:id/pulls/:branch`
///
/// Compares `:branch` against the default branch (`repo.config().default_branch`).
/// Returns commits unique to the branch, a file diff, mergeability, and
/// ahead/behind counts.
pub async fn get_pull_request(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath((id, branch)): AxPath<(String, String)>,
) -> Result<Json<PullRequestView>, ApiError> {
    crate::routes::validate_ref_name(&branch)?;

    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let view = tokio::task::spawn_blocking(move || -> Result<PullRequestView, ApiError> {
        let base_name = repo.config().default_branch.clone();

        // Resolve branch tip.
        let branch_ref = if branch.starts_with("refs/heads/") {
            branch.clone()
        } else {
            format!("refs/heads/{branch}")
        };
        let branch_oid = repo
            .ref_store()
            .resolve(&branch_ref)
            .map_err(|_| ApiError::not_found(&format!("branch '{branch}' not found")))?;

        // Resolve base tip.
        let base_ref = format!("refs/heads/{base_name}");
        let base_oid = repo
            .ref_store()
            .resolve(&base_ref)
            .map_err(|_| ApiError::not_found(&format!("default branch '{base_name}' not found")))?;

        // ── Compute ancestor sets once — reused for merge base, ahead, and behind ──
        //
        // Previously, find_merge_base ran collect_ancestors internally, and
        // then collect_ancestors was called again for ahead and behind.  That
        // produced 3-4 full BFS walks over the same graph.  We now compute
        // both sets once and pass them to a cheaper merge-base helper that
        // avoids any redundant work.
        let base_ancestors = collect_ancestors(&repo, base_oid);
        let branch_ancestors = collect_ancestors(&repo, branch_oid);

        // ── Find merge base using pre-computed ancestor sets ─────────────
        let merge_base = find_merge_base_from_sets(&repo, branch_oid, &base_ancestors);

        // ── Ahead commits: in branch but not reachable from base ─────────
        let mut ahead_commits: Vec<CommitInfo> = Vec::new();
        let mut visited: HashSet<ovc_core::id::ObjectId> = HashSet::new();
        let mut stack = vec![branch_oid];

        while let Some(oid) = stack.pop() {
            if !visited.insert(oid) {
                continue;
            }
            if base_ancestors.contains(&oid) {
                // This commit is already reachable from base — stop this path.
                continue;
            }

            let obj = repo.get_object(&oid).map_err(ApiError::from_core)?;
            let Some(ovc_core::object::Object::Commit(commit)) = obj else {
                continue;
            };

            let hex = oid.to_string();
            ahead_commits.push(CommitInfo {
                short_id: hex[..12.min(hex.len())].to_owned(),
                id: hex,
                message: commit.message.clone(),
                author: CommitAuthor {
                    name: commit.author.name.clone(),
                    email: commit.author.email.clone(),
                },
                authored_at: format_timestamp(commit.author.timestamp),
                parent_ids: commit.parents.iter().map(ToString::to_string).collect(),
                signature_status: "unsigned".to_owned(),
                signer_fingerprint: None,
                signer_identity: None,
            });

            for &parent in &commit.parents {
                stack.push(parent);
            }
        }
        let ahead_by = ahead_commits.len();

        // Sort ahead commits by timestamp descending (newest first).
        ahead_commits.sort_unstable_by(|a, b| b.authored_at.cmp(&a.authored_at));

        // ── Behind count: commits in base not reachable from branch ──────
        // Both ancestor sets were already computed above — no extra BFS needed.
        let behind_by = base_ancestors
            .iter()
            .filter(|oid| !branch_ancestors.contains(oid))
            .count();

        // ── Tree diff: base head vs branch head (full line-level hunks) ──
        let diff = compute_full_diff(base_oid, branch_oid, &repo)?;

        // ── Mergeability dry-run ─────────────────────────────────────────
        let (mergeable, conflict_files) =
            check_merge_cleanly(base_oid, branch_oid, merge_base, &repo)?;

        Ok(PullRequestView {
            branch: branch
                .strip_prefix("refs/heads/")
                .unwrap_or(&branch)
                .to_owned(),
            base: base_name,
            commits: ahead_commits,
            diff,
            mergeable,
            conflict_files,
            ahead_by,
            behind_by,
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(view))
}

// ── Private helpers ──────────────────────────────────────────────────────

/// Collects all commit OIDs reachable from `start` via depth-first walk.
fn collect_ancestors(
    repo: &ovc_core::repository::Repository,
    start: ovc_core::id::ObjectId,
) -> HashSet<ovc_core::id::ObjectId> {
    let mut visited: HashSet<ovc_core::id::ObjectId> = HashSet::new();
    let mut stack = vec![start];
    while let Some(oid) = stack.pop() {
        if !visited.insert(oid) {
            continue;
        }
        if let Ok(Some(ovc_core::object::Object::Commit(c))) = repo.get_object(&oid) {
            for &parent in &c.parents {
                stack.push(parent);
            }
        }
    }
    visited
}

/// Returns the most recent common ancestor of `oid_a` and `oid_b`.
///
/// Convenience wrapper that computes the ancestor set for `oid_a` internally.
/// Prefer [`find_merge_base_from_sets`] when the ancestor set is already
/// available to avoid a redundant BFS walk.
fn find_merge_base(
    repo: &ovc_core::repository::Repository,
    oid_a: ovc_core::id::ObjectId,
    oid_b: ovc_core::id::ObjectId,
) -> Option<ovc_core::id::ObjectId> {
    let ancestors_a = collect_ancestors(repo, oid_a);
    find_merge_base_from_sets(repo, oid_b, &ancestors_a)
}

/// Returns the most recent common ancestor of `oid_a` and `oid_b`.
///
/// Accepts a pre-computed ancestor set for `oid_a` so callers that already
/// have that set (e.g. `get_pull_request`) avoid a redundant BFS walk.
fn find_merge_base_from_sets(
    repo: &ovc_core::repository::Repository,
    oid_b: ovc_core::id::ObjectId,
    ancestors_a: &HashSet<ovc_core::id::ObjectId>,
) -> Option<ovc_core::id::ObjectId> {
    let mut visited: HashSet<ovc_core::id::ObjectId> = HashSet::new();
    let mut stack = vec![oid_b];
    while let Some(oid) = stack.pop() {
        if ancestors_a.contains(&oid) {
            return Some(oid);
        }
        if !visited.insert(oid) {
            continue;
        }
        if let Ok(Some(ovc_core::object::Object::Commit(c))) = repo.get_object(&oid) {
            for &parent in &c.parents {
                stack.push(parent);
            }
        }
    }
    None
}

/// Computes the full file diff (with line-level hunks) between two commit trees.
fn compute_full_diff(
    base_oid: ovc_core::id::ObjectId,
    head_oid: ovc_core::id::ObjectId,
    repo: &ovc_core::repository::Repository,
) -> Result<DiffResponse, ApiError> {
    let base_tree = resolve_commit_tree_oid(base_oid, repo)?;
    let head_tree = resolve_commit_tree_oid(head_oid, repo)?;

    let mut base_index = ovc_core::index::Index::new();
    base_index
        .read_tree(&base_tree, repo.object_store())
        .map_err(ApiError::from_core)?;

    let mut head_index = ovc_core::index::Index::new();
    head_index
        .read_tree(&head_tree, repo.object_store())
        .map_err(ApiError::from_core)?;

    // Reuse the full diff computation from commits.rs (includes hunks).
    crate::routes::commits::compute_diff_between_indices(&base_index, &head_index, repo)
}

/// Resolves a commit OID to its tree OID.
fn resolve_commit_tree_oid(
    commit_oid: ovc_core::id::ObjectId,
    repo: &ovc_core::repository::Repository,
) -> Result<ovc_core::id::ObjectId, ApiError> {
    let obj = repo
        .get_object(&commit_oid)
        .map_err(ApiError::from_core)?
        .ok_or_else(|| ApiError::not_found("commit not found"))?;
    let ovc_core::object::Object::Commit(c) = obj else {
        return Err(ApiError::bad_request("expected commit object"));
    };
    Ok(c.tree)
}

/// Performs a dry-run three-way merge to determine whether `branch_oid` can be
/// merged into `base_oid` without conflicts.
///
/// Uses a cloned scratch `ObjectStore` to avoid mutating the live repository.
fn check_merge_cleanly(
    base_oid: ovc_core::id::ObjectId,
    branch_oid: ovc_core::id::ObjectId,
    merge_base: Option<ovc_core::id::ObjectId>,
    repo: &ovc_core::repository::Repository,
) -> Result<(bool, Vec<String>), ApiError> {
    let base_obj = repo
        .get_object(&base_oid)
        .map_err(ApiError::from_core)?
        .ok_or_else(|| ApiError::not_found("base commit not found"))?;
    let ovc_core::object::Object::Commit(base_commit) = base_obj else {
        return Err(ApiError::bad_request("base is not a commit"));
    };

    let branch_obj = repo
        .get_object(&branch_oid)
        .map_err(ApiError::from_core)?
        .ok_or_else(|| ApiError::not_found("branch commit not found"))?;
    let ovc_core::object::Object::Commit(branch_commit) = branch_obj else {
        return Err(ApiError::bad_request("branch is not a commit"));
    };

    // Clone the store so the dry-run does not mutate live state.
    let mut scratch_store = repo.object_store().clone();

    let ancestor_tree_oid = if let Some(base) = merge_base {
        let anc_obj = repo
            .get_object(&base)
            .map_err(ApiError::from_core)?
            .ok_or_else(|| ApiError::not_found("merge base commit not found"))?;
        let ovc_core::object::Object::Commit(c) = anc_obj else {
            return Err(ApiError::bad_request("merge base is not a commit"));
        };
        c.tree
    } else {
        // No common ancestor: insert an empty tree into the scratch store.
        let empty_tree = ovc_core::object::Object::Tree(ovc_core::object::Tree {
            entries: Vec::new(),
        });
        scratch_store
            .insert(&empty_tree)
            .map_err(ApiError::from_core)?
    };

    let merge_result = ovc_core::merge::merge_trees(
        &ancestor_tree_oid,
        &base_commit.tree,
        &branch_commit.tree,
        &mut scratch_store,
    )
    .map_err(ApiError::from_core)?;

    if merge_result.conflicts.is_empty() {
        Ok((true, Vec::new()))
    } else {
        let paths = merge_result
            .conflicts
            .iter()
            .map(|c| c.path.clone())
            .collect();
        Ok((false, paths))
    }
}

// ── PR helpers ──────────────────────────────────────────────────────────

fn now_iso8601() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Maps a core `PullRequest` reference to the lightweight API summary type.
fn pr_to_summary(pr: &PullRequest) -> PullRequestSummary {
    PullRequestSummary {
        number: pr.number,
        title: pr.title.clone(),
        state: pr.state,
        source_branch: pr.source_branch.clone(),
        target_branch: pr.target_branch.clone(),
        author: pr.author.clone(),
        created_at: pr.created_at.clone(),
        updated_at: pr.updated_at.clone(),
    }
}

// ── PR Lifecycle Handlers ───────────────────────────────────────────────

/// Handler: `GET /api/v1/repos/:id/pulls`
///
/// Lists all pull requests, optionally filtered by state.
pub async fn list_pull_requests(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath(id): AxPath<String>,
    Query(query): Query<ListPullRequestsQuery>,
) -> Result<Json<Vec<PullRequestSummary>>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let summaries =
        tokio::task::spawn_blocking(move || -> Result<Vec<PullRequestSummary>, ApiError> {
            let state_filter = query.state.to_lowercase();
            let filter = match state_filter.as_str() {
                "all" => None,
                "closed" => Some(PrState::Closed),
                "merged" => Some(PrState::Merged),
                // "open" and any unrecognised value default to showing open PRs.
                _ => Some(PrState::Open),
            };

            let summaries: Vec<PullRequestSummary> = repo
                .pull_request_store()
                .list(filter)
                .iter()
                .map(|pr| pr_to_summary(pr))
                .collect();

            Ok(summaries)
        })
        .await
        .map_err(|e| {
            tracing::error!("task join error: {e}");
            ApiError::internal("internal task error")
        })??;

    Ok(Json(summaries))
}

/// Handler: `POST /api/v1/repos/:id/pulls`
///
/// Creates a new pull request with auto-assigned number.
pub async fn create_pull_request(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath(id): AxPath<String>,
    Json(req): Json<CreatePullRequestRequest>,
) -> Result<(axum::http::StatusCode, Json<PullRequest>), ApiError> {
    // Validate title: non-empty and bounded length.
    if req.title.trim().is_empty() {
        return Err(ApiError::bad_request("title must not be empty"));
    }
    if req.title.len() > 500 {
        return Err(ApiError::bad_request(
            "title must not exceed 500 characters",
        ));
    }

    // Validate description length.
    if let Some(ref desc) = req.description
        && desc.len() > 65_536
    {
        return Err(ApiError::bad_request(
            "description must not exceed 65536 characters",
        ));
    }

    // Validate branch name lengths.
    if req.source_branch.trim().is_empty() || req.source_branch.len() > 256 {
        return Err(ApiError::bad_request(
            "source_branch must be non-empty and at most 256 characters",
        ));
    }
    if let Some(ref target) = req.target_branch
        && (target.trim().is_empty() || target.len() > 256)
    {
        return Err(ApiError::bad_request(
            "target_branch must be non-empty and at most 256 characters",
        ));
    }

    // Validate source branch name.
    crate::routes::validate_ref_name(&req.source_branch)?;
    if let Some(ref target) = req.target_branch {
        crate::routes::validate_ref_name(target)?;
    }

    // Hold the per-repo mutex around the entire create operation to prevent
    // concurrent requests from allocating the same PR number.
    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;

    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let pr = tokio::task::spawn_blocking(move || -> Result<PullRequest, ApiError> {
        // Determine the target branch (default to repo's default branch).
        let target_branch = req
            .target_branch
            .unwrap_or_else(|| repo.config().default_branch.clone());

        // Verify source branch exists.
        let source_ref = format!("refs/heads/{}", req.source_branch);
        repo.ref_store().resolve(&source_ref).map_err(|_| {
            ApiError::not_found(&format!("source branch '{}' not found", req.source_branch))
        })?;

        // Verify target branch exists.
        let target_ref = format!("refs/heads/{target_branch}");
        repo.ref_store().resolve(&target_ref).map_err(|_| {
            ApiError::not_found(&format!("target branch '{target_branch}' not found"))
        })?;

        let number = repo
            .pull_request_store_mut()
            .next_pr_number()
            .map_err(ApiError::from_core)?;
        let now = now_iso8601();

        let pr = PullRequest {
            number,
            title: req.title,
            description: req.description.unwrap_or_default(),
            state: PrState::Open,
            source_branch: req.source_branch,
            target_branch,
            author: req
                .author
                .unwrap_or_else(|| repo.config().user_name.clone()),
            created_at: now.clone(),
            updated_at: now,
            merged_at: None,
            merge_commit: None,
            checks: None,
            reviews: Vec::new(),
            comments: Vec::new(),
            required_approvals: 0,
        };

        repo.pull_request_store_mut().save(pr.clone());
        repo.save().map_err(ApiError::from_core)?;
        Ok(pr)
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    // Run CI checks if actions config has pull-request triggered actions.
    let pr = run_checks_for_pr(&app, &id, pr).await?;

    Ok((axum::http::StatusCode::CREATED, Json(pr)))
}

/// Handler: `GET /api/v1/repos/:id/pulls/by-number/:number`
///
/// Returns a single pull request by its number.
pub async fn get_pull_request_by_number(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath((id, number)): AxPath<(String, u64)>,
) -> Result<Json<PullRequest>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let pr = tokio::task::spawn_blocking(move || -> Result<PullRequest, ApiError> {
        repo.pull_request_store()
            .get(number)
            .cloned()
            .ok_or_else(|| ApiError::not_found(&format!("pull request #{number} not found")))
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(pr))
}

/// Handler: `PATCH /api/v1/repos/:id/pulls/by-number/:number`
///
/// Updates a pull request's title, description, or state (open/closed).
pub async fn update_pull_request(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath((id, number)): AxPath<(String, u64)>,
    Json(req): Json<UpdatePullRequestRequest>,
) -> Result<Json<PullRequest>, ApiError> {
    // Validate field lengths before entering the blocking task.
    if let Some(ref title) = req.title {
        if title.trim().is_empty() {
            return Err(ApiError::bad_request("title must not be empty"));
        }
        if title.len() > 500 {
            return Err(ApiError::bad_request(
                "title must not exceed 500 characters",
            ));
        }
    }
    if let Some(ref description) = req.description
        && description.len() > 65_536
    {
        return Err(ApiError::bad_request(
            "description must not exceed 65536 characters",
        ));
    }

    let lock = app.repo_lock(&id);
    let _guard = lock.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let pr = tokio::task::spawn_blocking(move || -> Result<PullRequest, ApiError> {
        let pr = repo
            .pull_request_store_mut()
            .get_mut(number)
            .ok_or_else(|| ApiError::not_found(&format!("pull request #{number} not found")))?;

        // Cannot update a merged PR.
        if pr.state == PrState::Merged {
            return Err(ApiError::conflict("cannot update a merged pull request"));
        }

        if let Some(title) = req.title {
            // Length already validated above; re-check emptiness for safety.
            if title.trim().is_empty() {
                return Err(ApiError::bad_request("title must not be empty"));
            }
            pr.title = title;
        }

        if let Some(description) = req.description {
            pr.description = description;
        }

        if let Some(state_str) = req.state {
            match state_str.to_lowercase().as_str() {
                "open" => pr.state = PrState::Open,
                "closed" => pr.state = PrState::Closed,
                _ => {
                    return Err(ApiError::bad_request(
                        "state must be 'open' or 'closed' (use the merge endpoint to merge)",
                    ));
                }
            }
        }

        pr.updated_at = now_iso8601();
        let result = pr.clone();
        repo.save().map_err(ApiError::from_core)?;
        Ok(result)
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(pr))
}

/// Handler: `POST /api/v1/repos/:id/pulls/by-number/:number/merge`
///
/// Merges the source branch into the target branch and marks the PR as merged.
/// Reuses the three-way merge logic from `branches::merge_branch`.
#[allow(clippy::too_many_lines)]
pub async fn merge_pull_request(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath((id, number)): AxPath<(String, u64)>,
    Json(req): Json<MergePullRequestRequest>,
) -> Result<Json<MergePullRequestResponse>, ApiError> {
    let strategy = req.strategy.unwrap_or_else(|| "merge".to_owned());
    if !matches!(strategy.as_str(), "merge" | "squash" | "rebase") {
        return Err(ApiError::bad_request(
            "strategy must be 'merge', 'squash', or 'rebase'",
        ));
    }

    // Read the PR first (outside the repo lock) to fail fast.
    let pr_snapshot = {
        let (repo, _) = open_repo_blocking(&app, &id).await?;
        tokio::task::spawn_blocking(move || -> Result<PullRequest, ApiError> {
            repo.pull_request_store()
                .get(number)
                .cloned()
                .ok_or_else(|| ApiError::not_found(&format!("pull request #{number} not found")))
        })
        .await
        .map_err(|e| {
            tracing::error!("task join error: {e}");
            ApiError::internal("internal task error")
        })??
    };

    if pr_snapshot.state == PrState::Merged {
        return Err(ApiError::conflict("pull request is already merged"));
    }
    if pr_snapshot.state == PrState::Closed {
        return Err(ApiError::conflict(
            "cannot merge a closed pull request — reopen it first",
        ));
    }

    // Block merge if CI checks are failing, unless force is set.
    if !req.force
        && pr_snapshot
            .checks
            .as_ref()
            .is_some_and(|c| c.status == "failing")
    {
        return Err(ApiError::conflict(
            "Cannot merge: CI checks are failing. Run checks or fix failures first.",
        ));
    }

    // Enforce branch protection: required approvals and CI pass.
    if !req.force {
        let (repo_bp, _) = open_repo_blocking(&app, &id).await?;
        let protection = tokio::task::spawn_blocking({
            let target = pr_snapshot.target_branch.clone();
            move || -> Option<ovc_core::access::BranchProtection> {
                repo_bp
                    .access_control()
                    .branch_protection
                    .get(&target)
                    .cloned()
            }
        })
        .await
        .map_err(|e| {
            tracing::error!("task join error: {e}");
            ApiError::internal("internal task error")
        })?;

        if let Some(bp) = protection {
            // Check required approvals.
            if bp.required_approvals > 0 {
                let approval_count = u32::try_from(
                    pr_snapshot
                        .reviews
                        .iter()
                        .filter(|r| r.state == ovc_core::pulls::ReviewState::Approved)
                        .count(),
                )
                .unwrap_or(u32::MAX);
                if approval_count < bp.required_approvals {
                    return Err(ApiError::conflict(&format!(
                        "Cannot merge: branch '{}' requires {} approved review(s), but only {} found",
                        pr_snapshot.target_branch, bp.required_approvals, approval_count
                    )));
                }
            }

            // Check CI pass requirement.
            if bp.require_ci_pass {
                let ci_passing = pr_snapshot
                    .checks
                    .as_ref()
                    .is_some_and(|c| c.status == "passing");
                if !ci_passing {
                    return Err(ApiError::conflict(&format!(
                        "Cannot merge: branch '{}' requires CI checks to pass before merging",
                        pr_snapshot.target_branch
                    )));
                }
            }
        }
    }

    // Run pre-merge hooks unless force is set.
    if !req.force {
        let work_dir = repo_working_dir(&app, &id);
        let hook_results =
            ovc_actions::hooks::run_pre_merge_hooks(&work_dir, &[]).unwrap_or_default();
        if ovc_actions::hooks::has_blocking_failures(&hook_results) {
            let failures: Vec<String> = hook_results
                .iter()
                .filter(|r| {
                    !r.continue_on_error
                        && matches!(
                            r.status,
                            ActionStatus::Failed | ActionStatus::TimedOut | ActionStatus::Error
                        )
                })
                .map(|r| r.display_name.clone())
                .collect();
            return Err(ApiError::conflict(&format!(
                "Cannot merge: pre-merge checks failed: {}",
                failures.join(", ")
            )));
        }

        // Check branch protection rules on the target branch.
        let violations =
            ovc_actions::hooks::check_branch_protection(&work_dir, &pr_snapshot.target_branch)
                .unwrap_or_default();
        // Filter out the "require_pull_request" violation since we ARE merging via PR.
        let violations: Vec<String> = violations
            .into_iter()
            .filter(|v| !v.contains("requires merging via pull request"))
            .collect();
        if !violations.is_empty() {
            return Err(ApiError::conflict(&format!(
                "Cannot merge: branch protection violations on '{}': {}",
                pr_snapshot.target_branch,
                violations.join("; ")
            )));
        }
    }

    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let response = tokio::task::spawn_blocking(move || -> Result<MergePullRequestResponse, ApiError> {
        // Re-read the PR under the lock to avoid TOCTOU races.
        let pr = repo
            .pull_request_store()
            .get(number)
            .cloned()
            .ok_or_else(|| ApiError::not_found(&format!("pull request #{number} not found")))?;

        if pr.state == PrState::Merged {
            return Err(ApiError::conflict("pull request is already merged"));
        }
        if pr.state == PrState::Closed {
            return Err(ApiError::conflict("cannot merge a closed pull request"));
        }

        // Validate author identity.
        if repo.config().user_name.is_empty() || repo.config().user_email.is_empty() {
            return Err(ApiError::bad_request(
                "user.name and user.email must be configured for merge commits",
            ));
        }

        // Resolve target branch (ours).
        let target_ref = format!("refs/heads/{}", pr.target_branch);
        let our_oid = repo
            .ref_store()
            .resolve(&target_ref)
            .map_err(|_| ApiError::not_found(&format!(
                "target branch '{}' not found", pr.target_branch
            )))?;
        let our_obj = repo
            .get_object(&our_oid)
            .map_err(ApiError::from_core)?
            .ok_or_else(|| ApiError::internal("target branch does not point to a commit"))?;
        let ovc_core::object::Object::Commit(our_commit) = our_obj else {
            return Err(ApiError::internal("target branch does not point to a commit"));
        };

        // Resolve source branch (theirs).
        let source_ref = format!("refs/heads/{}", pr.source_branch);
        let their_oid = repo
            .ref_store()
            .resolve(&source_ref)
            .map_err(|_| ApiError::not_found(&format!(
                "source branch '{}' not found", pr.source_branch
            )))?;

        if our_oid == their_oid {
            return Ok(MergePullRequestResponse {
                status: "already_up_to_date".to_owned(),
                commit_id: Some(our_oid.to_string()),
                conflicts: Vec::new(),
                pull_request: pr,
            });
        }

        let their_obj = repo
            .get_object(&their_oid)
            .map_err(ApiError::from_core)?
            .ok_or_else(|| ApiError::internal("source branch does not point to a commit"))?;
        let ovc_core::object::Object::Commit(their_commit) = their_obj else {
            return Err(ApiError::internal("source branch does not point to a commit"));
        };

        // Find merge base.
        let base_oid = find_merge_base(&repo, our_oid, their_oid);

        let base_tree = if let Some(base) = base_oid {
            let base_obj = repo
                .get_object(&base)
                .map_err(ApiError::from_core)?
                .ok_or_else(|| ApiError::internal("merge base commit not found"))?;
            let ovc_core::object::Object::Commit(c) = base_obj else {
                return Err(ApiError::internal("merge base is not a commit"));
            };
            c.tree
        } else {
            // No common ancestor: use empty tree.
            let empty_tree = ovc_core::object::Object::Tree(ovc_core::object::Tree {
                entries: Vec::new(),
            });
            repo.insert_object(&empty_tree).map_err(ApiError::from_core)?
        };

        // Perform three-way tree merge.
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
            return Ok(MergePullRequestResponse {
                status: "conflict".to_owned(),
                commit_id: None,
                conflicts: conflict_paths,
                pull_request: pr,
            });
        }

        // Build merged tree.
        let merged_tree = ovc_core::object::Object::Tree(ovc_core::object::Tree {
            entries: merge_result.entries,
        });
        let merged_tree_oid = repo
            .insert_object(&merged_tree)
            .map_err(ApiError::from_core)?;

        // To create a merge commit on the target branch, we need HEAD to
        // point there. Save the original HEAD so we can restore it.
        let original_head = repo.ref_store().head().clone();
        let target_symbolic = format!("refs/heads/{}", pr.target_branch);

        // Temporarily switch HEAD to the target branch.
        repo.ref_store_mut()
            .set_head(ovc_core::refs::RefTarget::Symbolic(target_symbolic.clone()));

        // Read merged tree into index.
        {
            let (index, object_store) = repo.index_and_store_mut();
            index
                .read_tree(&merged_tree_oid, object_store)
                .map_err(ApiError::from_core)?;
        }

        // Create merge commit.
        let author = ovc_core::object::Identity {
            name: repo.config().user_name.clone(),
            email: repo.config().user_email.clone(),
            timestamp: chrono::Utc::now().timestamp(),
            tz_offset_minutes: 0,
        };

        let merge_message = format!(
            "Merge pull request #{} from {}\n\n{}",
            pr.number, pr.source_branch, pr.title
        );
        let commit_oid = repo
            .create_commit(&merge_message, &author)
            .map_err(ApiError::from_core)?;

        // Restore original HEAD if it was different.
        if !matches!(&original_head, ovc_core::refs::RefTarget::Symbolic(s) if *s == target_symbolic)
        {
            repo.ref_store_mut().set_head(original_head);
        }

        // Update PR metadata in the store and save everything in one call.
        let now = now_iso8601();
        let mut updated_pr = pr;
        updated_pr.state = PrState::Merged;
        updated_pr.merged_at = Some(now.clone());
        updated_pr.merge_commit = Some(commit_oid.to_string());
        updated_pr.updated_at = now;
        repo.pull_request_store_mut().save(updated_pr.clone());

        repo.save().map_err(ApiError::from_core)?;

        Ok(MergePullRequestResponse {
            status: "merged".to_owned(),
            commit_id: Some(commit_oid.to_string()),
            conflicts: Vec::new(),
            pull_request: updated_pr,
        })
    })
    .await
    .map_err(|e| { tracing::error!("task join error: {e}"); ApiError::internal("internal task error") })??;

    Ok(Json(response))
}

// ── CI Check Helpers ────────────────────────────────────────────────────

/// Marker files used to identify a project working directory.
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

/// Derives the working directory for a repository. Mirrors the logic in
/// `actions.rs::repo_working_dir` so that the pulls module can load
/// actions config independently.
fn repo_working_dir(app: &AppState, repo_id: &str) -> PathBuf {
    if let Some(workdir) = app.workdir_for(repo_id)
        && workdir.is_dir()
    {
        return workdir;
    }
    let ovc_path = app.repos_dir.join(format!("{repo_id}.ovc"));
    if let Some(parent) = ovc_path.parent() {
        for marker in PROJECT_MARKERS {
            if parent.join(marker).exists() {
                return parent.to_path_buf();
            }
        }
    }
    app.repos_dir.join(format!("{repo_id}.ovc.d"))
}

/// Collects changed file paths between two branches (source vs target).
fn collect_changed_paths(
    repo: &ovc_core::repository::Repository,
    source_branch: &str,
    target_branch: &str,
) -> Result<Vec<String>, ApiError> {
    let source_ref = format!("refs/heads/{source_branch}");
    let target_ref = format!("refs/heads/{target_branch}");

    let source_oid = repo
        .ref_store()
        .resolve(&source_ref)
        .map_err(|_| ApiError::not_found(&format!("source branch '{source_branch}' not found")))?;
    let target_oid = repo
        .ref_store()
        .resolve(&target_ref)
        .map_err(|_| ApiError::not_found(&format!("target branch '{target_branch}' not found")))?;

    let diff = compute_full_diff(target_oid, source_oid, repo)?;
    let paths: Vec<String> = diff.files.iter().map(|f| f.path.clone()).collect();
    Ok(paths)
}

/// Converts action runner results into a `PrChecks` structure.
fn results_to_pr_checks(results: &[ovc_actions::runner::ActionResult]) -> PrChecks {
    let status = if results
        .iter()
        .all(|r| r.status == ActionStatus::Passed || r.status == ActionStatus::Skipped)
    {
        "passing".to_owned()
    } else {
        "failing".to_owned()
    };

    let check_results: Vec<PrCheckResult> = results
        .iter()
        .map(|r| PrCheckResult {
            name: r.name.clone(),
            display_name: r.display_name.clone(),
            category: r.category.clone(),
            status: r.status.to_string(),
            duration_ms: r.duration_ms,
            docker_used: r.docker_used,
        })
        .collect();

    PrChecks {
        status,
        results: check_results,
        ran_at: now_iso8601(),
    }
}

/// Runs CI checks for a pull request if actions config has pull-request
/// triggered actions. Updates and saves the PR if checks are run.
async fn run_checks_for_pr(
    app: &AppState,
    repo_id: &str,
    mut pr: PullRequest,
) -> Result<PullRequest, ApiError> {
    let work_dir = repo_working_dir(app, repo_id);

    let config = {
        let wd = work_dir.clone();
        tokio::task::spawn_blocking(move || ActionsConfig::load(&wd))
            .await
            .map_err(|e| {
                tracing::error!("task join error: {e}");
                ApiError::internal("internal task error")
            })?
            .map_err(|e| ApiError::internal(&format!("failed to load actions config: {e}")))?
    };

    let Some(config) = config else {
        return Ok(pr);
    };

    // Check if there are any pull-request triggered actions.
    let pr_actions = config.actions_for_trigger(Trigger::PullRequest);
    if pr_actions.is_empty() {
        return Ok(pr);
    }

    // Get changed files between source and target.
    let (repo, _) = open_repo_blocking(app, repo_id).await?;
    let source = pr.source_branch.clone();
    let target = pr.target_branch.clone();
    let changed_paths =
        tokio::task::spawn_blocking(move || collect_changed_paths(&repo, &source, &target))
            .await
            .map_err(|e| {
                tracing::error!("task join error: {e}");
                ApiError::internal("internal task error")
            })??;

    // Run actions.
    let runner = ActionRunner::new_with_docker_probe(&work_dir, config).await;
    let results = runner
        .run_trigger(Trigger::PullRequest, &changed_paths)
        .await;

    // Convert to PrChecks and update PR.
    let checks = results_to_pr_checks(&results);
    pr.checks = Some(checks);
    pr.updated_at = now_iso8601();

    // Save updated PR into the superblock.
    let lock = app.repo_lock(repo_id);
    let _guard = lock.lock().await;
    let (mut repo, _) = open_repo_blocking(app, repo_id).await?;
    let pr_clone = pr.clone();
    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        repo.pull_request_store_mut().save(pr_clone);
        repo.save().map_err(ApiError::from_core)?;
        Ok(())
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(pr)
}

/// Handler: `POST /api/v1/repos/:id/pulls/by-number/:number/checks`
///
/// Runs CI checks for a pull request and updates the PR's checks field.
pub async fn run_pr_checks(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath((id, number)): AxPath<(String, u64)>,
) -> Result<Json<PullRequest>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let pr = tokio::task::spawn_blocking(move || -> Result<PullRequest, ApiError> {
        repo.pull_request_store()
            .get(number)
            .cloned()
            .ok_or_else(|| ApiError::not_found(&format!("pull request #{number} not found")))
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    if pr.state != PrState::Open {
        return Err(ApiError::bad_request(
            "can only run checks on open pull requests",
        ));
    }

    let pr = run_checks_for_pr(&app, &id, pr).await?;
    Ok(Json(pr))
}

// ── Reviews ─────────────────────────────────────────────────────────────

/// Handler: `POST /api/v1/repos/:id/pulls/by-number/:number/reviews`
///
/// Creates a new review on a pull request.
pub async fn create_review(
    claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath((id, number)): AxPath<(String, u64)>,
    Json(req): Json<CreateReviewRequest>,
) -> Result<Json<Review>, ApiError> {
    let author = claims.sub.clone();

    let lock = app.repo_lock(&id);
    let _guard = lock.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let review = tokio::task::spawn_blocking(move || -> Result<Review, ApiError> {
        let pr = repo
            .pull_request_store_mut()
            .get_mut(number)
            .ok_or_else(|| ApiError::not_found(&format!("pull request #{number} not found")))?;

        if pr.state != PrState::Open {
            return Err(ApiError::bad_request("can only review open pull requests"));
        }

        let review_id = pr.reviews.len() as u64 + 1;
        let now = chrono::Utc::now().to_rfc3339();

        let review = Review {
            id: review_id,
            author,
            author_identity: None,
            state: req.state,
            body: req.body,
            created_at: now.clone(),
            signature: req.signature,
            verified: false,
        };

        pr.reviews.push(review.clone());
        pr.updated_at = now;

        repo.save().map_err(ApiError::from_core)?;
        Ok(review)
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(review))
}

/// Handler: `GET /api/v1/repos/:id/pulls/by-number/:number/reviews`
///
/// Lists all reviews on a pull request.
pub async fn list_reviews(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath((id, number)): AxPath<(String, u64)>,
) -> Result<Json<Vec<Review>>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let reviews = tokio::task::spawn_blocking(move || -> Result<Vec<Review>, ApiError> {
        let pr = repo
            .pull_request_store()
            .get(number)
            .ok_or_else(|| ApiError::not_found(&format!("pull request #{number} not found")))?;
        Ok(pr.reviews.clone())
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(reviews))
}

// ── Comments ────────────────────────────────────────────────────────────

/// Handler: `POST /api/v1/repos/:id/pulls/by-number/:number/comments`
///
/// Creates a new comment on a pull request.
pub async fn create_comment(
    claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath((id, number)): AxPath<(String, u64)>,
    Json(req): Json<CreateCommentRequest>,
) -> Result<Json<PrComment>, ApiError> {
    let author = claims.sub.clone();

    let lock = app.repo_lock(&id);
    let _guard = lock.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let comment = tokio::task::spawn_blocking(move || -> Result<PrComment, ApiError> {
        let pr = repo
            .pull_request_store_mut()
            .get_mut(number)
            .ok_or_else(|| ApiError::not_found(&format!("pull request #{number} not found")))?;

        let comment_id = pr.comments.len() as u64 + 1;
        let now = chrono::Utc::now().to_rfc3339();

        let comment = PrComment {
            id: comment_id,
            author,
            author_identity: None,
            body: req.body,
            file_path: req.file_path,
            line_number: req.line_number,
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        pr.comments.push(comment.clone());
        pr.updated_at = now;

        repo.save().map_err(ApiError::from_core)?;
        Ok(comment)
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(comment))
}

/// Handler: `GET /api/v1/repos/:id/pulls/by-number/:number/comments`
///
/// Lists all comments on a pull request.
pub async fn list_comments(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath((id, number)): AxPath<(String, u64)>,
) -> Result<Json<Vec<PrComment>>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let comments = tokio::task::spawn_blocking(move || -> Result<Vec<PrComment>, ApiError> {
        let pr = repo
            .pull_request_store()
            .get(number)
            .ok_or_else(|| ApiError::not_found(&format!("pull request #{number} not found")))?;
        Ok(pr.comments.clone())
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(comments))
}

/// Handler: `PATCH /api/v1/repos/:id/pulls/by-number/:number/comments/:comment_id`
///
/// Updates a comment's body.
pub async fn update_comment(
    claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath((id, number, comment_id)): AxPath<(String, u64, u64)>,
    Json(req): Json<UpdateCommentRequest>,
) -> Result<Json<PrComment>, ApiError> {
    let author = claims.sub.clone();

    let lock = app.repo_lock(&id);
    let _guard = lock.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let comment = tokio::task::spawn_blocking(move || -> Result<PrComment, ApiError> {
        let pr = repo
            .pull_request_store_mut()
            .get_mut(number)
            .ok_or_else(|| ApiError::not_found(&format!("pull request #{number} not found")))?;

        let comment = pr
            .comments
            .iter_mut()
            .find(|c| c.id == comment_id)
            .ok_or_else(|| ApiError::not_found(&format!("comment #{comment_id} not found")))?;

        // Only the author (or a local admin) can edit.
        if comment.author != author && author != "local-user" {
            return Err(ApiError::forbidden("you can only edit your own comments"));
        }

        comment.body = req.body;
        comment.updated_at = chrono::Utc::now().to_rfc3339();

        let updated = comment.clone();
        repo.save().map_err(ApiError::from_core)?;
        Ok(updated)
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(comment))
}

/// Handler: `DELETE /api/v1/repos/:id/pulls/by-number/:number/comments/:comment_id`
///
/// Deletes a comment.
pub async fn delete_comment(
    claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath((id, number, comment_id)): AxPath<(String, u64, u64)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let author = claims.sub.clone();

    let lock = app.repo_lock(&id);
    let _guard = lock.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let pr = repo
            .pull_request_store_mut()
            .get_mut(number)
            .ok_or_else(|| ApiError::not_found(&format!("pull request #{number} not found")))?;

        let idx = pr
            .comments
            .iter()
            .position(|c| c.id == comment_id)
            .ok_or_else(|| ApiError::not_found(&format!("comment #{comment_id} not found")))?;

        // Only the author (or a local admin) can delete.
        if pr.comments[idx].author != author && author != "local-user" {
            return Err(ApiError::forbidden("you can only delete your own comments"));
        }

        pr.comments.remove(idx);
        pr.updated_at = chrono::Utc::now().to_rfc3339();

        repo.save().map_err(ApiError::from_core)?;
        Ok(())
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(serde_json::json!({ "deleted": true })))
}
