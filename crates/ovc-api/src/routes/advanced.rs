//! Advanced VCS operation endpoints: stash, rebase, cherry-pick, GC.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use axum::extract::Query;

use crate::auth::Claims;
use crate::error::ApiError;
use crate::models::{
    CherryPickRequest, CherryPickResponse, GcResponse, RebaseRequest, RebaseResponse,
    ReflogEntryResponse, ReflogQuery, ResetRequest, ResetResponse, RevertRequest, RevertResponse,
    StashEntryInfo, StashPushRequest,
};
use crate::routes::repos::open_repo_blocking;
use crate::routes::{resolve_commit_spec, validate_ref_name};
use crate::state::AppState;

// ── Stash ───────────────────────────────────────────────────────────────

/// Handler: `GET /api/v1/repos/:id/stash`
pub async fn list_stash(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<StashEntryInfo>>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let entries = tokio::task::spawn_blocking(move || -> Vec<StashEntryInfo> {
        repo.stash()
            .list()
            .iter()
            .enumerate()
            .map(|(i, entry)| StashEntryInfo {
                index: i,
                message: entry.message.clone(),
                commit_id: entry.commit_id.to_string(),
                base_commit_id: entry.base_commit_id.to_string(),
                timestamp: entry.timestamp,
            })
            .collect()
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })?;

    Ok(Json(entries))
}

/// Handler: `POST /api/v1/repos/:id/stash`
pub async fn push_stash(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<StashPushRequest>,
) -> Result<Json<StashEntryInfo>, ApiError> {
    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let entry = tokio::task::spawn_blocking(move || -> Result<StashEntryInfo, ApiError> {
        repo.stash_push(&req.message).map_err(ApiError::from_core)?;
        repo.save().map_err(ApiError::from_core)?;

        let stash_list = repo.stash().list();
        let stash_entry = stash_list.first().ok_or_else(|| {
            ApiError::internal("stash push succeeded but stash list is unexpectedly empty")
        })?;
        Ok(StashEntryInfo {
            index: 0,
            message: stash_entry.message.clone(),
            commit_id: stash_entry.commit_id.to_string(),
            base_commit_id: stash_entry.base_commit_id.to_string(),
            timestamp: stash_entry.timestamp,
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(entry))
}

/// Handler: `POST /api/v1/repos/:id/stash/:idx/pop`
pub async fn pop_stash(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, idx)): Path<(String, usize)>,
) -> Result<Json<StashEntryInfo>, ApiError> {
    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let entry = tokio::task::spawn_blocking(move || -> Result<StashEntryInfo, ApiError> {
        let stash_list = repo.stash().list();
        let info = stash_list
            .get(idx)
            .ok_or_else(|| ApiError::not_found(&format!("stash index {idx} not found")))?;
        let info = StashEntryInfo {
            index: idx,
            message: info.message.clone(),
            commit_id: info.commit_id.to_string(),
            base_commit_id: info.base_commit_id.to_string(),
            timestamp: info.timestamp,
        };
        repo.stash_pop(idx).map_err(ApiError::from_core)?;
        repo.save().map_err(ApiError::from_core)?;
        Ok(info)
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(entry))
}

/// Handler: `POST /api/v1/repos/:id/stash/:idx/apply`
pub async fn apply_stash(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, idx)): Path<(String, usize)>,
) -> Result<axum::http::StatusCode, ApiError> {
    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let store = repo.object_store();
        let mut index = repo.index().clone();
        repo.stash()
            .apply(idx, store, &mut index)
            .map_err(ApiError::from_core)?;
        *repo.index_mut() = index;
        repo.save().map_err(ApiError::from_core)?;
        Ok(())
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(axum::http::StatusCode::OK)
}

/// Handler: `DELETE /api/v1/repos/:id/stash`
///
/// Clears all stash entries. Returns `204 No Content` on success.
pub async fn clear_stash(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        repo.stash_mut().clear();
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

/// Handler: `DELETE /api/v1/repos/:id/stash/:idx`
pub async fn drop_stash(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, idx)): Path<(String, usize)>,
) -> Result<axum::http::StatusCode, ApiError> {
    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        repo.stash_mut()
            .drop_entry(idx)
            .map_err(ApiError::from_core)?;
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

// ── Rebase ──────────────────────────────────────────────────────────────

/// Handler: `POST /api/v1/repos/:id/rebase`
pub async fn rebase_branch(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<RebaseRequest>,
) -> Result<Json<RebaseResponse>, ApiError> {
    validate_ref_name(&req.onto)?;
    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let response = tokio::task::spawn_blocking(move || -> Result<RebaseResponse, ApiError> {
        let current_branch = match repo.ref_store().head() {
            ovc_core::refs::RefTarget::Symbolic(s) => {
                s.strip_prefix("refs/heads/").unwrap_or(s).to_owned()
            }
            ovc_core::refs::RefTarget::Direct(_) => {
                return Err(ApiError::bad_request("cannot rebase from detached HEAD"));
            }
        };

        match repo.rebase_branch(&current_branch, &req.onto) {
            Ok(result) => {
                repo.save().map_err(ApiError::from_core)?;
                Ok(RebaseResponse {
                    status: "success".to_owned(),
                    new_tip: Some(result.new_tip.to_string()),
                    replayed_count: result.replayed.len(),
                    replayed: result
                        .replayed
                        .iter()
                        .map(|(old, new)| (old.to_string(), new.to_string()))
                        .collect(),
                    conflicts: Vec::new(),
                })
            }
            Err(ovc_core::rebase::RebaseError::Conflict {
                conflicts,
                completed,
                ..
            }) => Ok(RebaseResponse {
                status: "conflict".to_owned(),
                new_tip: None,
                replayed_count: completed.len(),
                replayed: completed
                    .iter()
                    .map(|(old, new)| (old.to_string(), new.to_string()))
                    .collect(),
                conflicts,
            }),
            Err(ovc_core::rebase::RebaseError::NoCommonAncestor) => {
                Err(ApiError::bad_request("no common ancestor found"))
            }
            Err(ovc_core::rebase::RebaseError::MergeCommitInChain { commit }) => {
                Err(ApiError::bad_request(&format!(
                    "cannot rebase: commit {commit} is a merge commit; use merge instead"
                )))
            }
            Err(ovc_core::rebase::RebaseError::BaseNotReachable) => Err(ApiError::bad_request(
                "cannot rebase: base commit is not reachable from branch tip",
            )),
            Err(ovc_core::rebase::RebaseError::Core(e)) => Err(ApiError::from_core(e)),
        }
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(response))
}

// ── Cherry-pick ─────────────────────────────────────────────────────────

/// Handler: `POST /api/v1/repos/:id/cherry-pick`
pub async fn cherry_pick(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<CherryPickRequest>,
) -> Result<Json<CherryPickResponse>, ApiError> {
    let commit_oid: ovc_core::id::ObjectId = req
        .commit_id
        .parse()
        .map_err(|e| ApiError::bad_request(&format!("invalid commit id: {e}")))?;

    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;
    let source = req.commit_id.clone();

    let response = tokio::task::spawn_blocking(move || -> Result<CherryPickResponse, ApiError> {
        let new_oid = repo
            .cherry_pick_commit(&commit_oid)
            .map_err(ApiError::from_core)?;
        repo.save().map_err(ApiError::from_core)?;

        Ok(CherryPickResponse {
            new_commit_id: new_oid.to_string(),
            source_commit_id: source,
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(response))
}

// ── Revert ──────────────────────────────────────────────────────────────

/// Handler: `POST /api/v1/repos/:id/revert`
pub async fn revert_commit(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<RevertRequest>,
) -> Result<Json<RevertResponse>, ApiError> {
    let commit_oid: ovc_core::id::ObjectId = req
        .commit_id
        .parse()
        .map_err(|e| ApiError::bad_request(&format!("invalid commit id: {e}")))?;

    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;
    let source = req.commit_id.clone();

    let response = tokio::task::spawn_blocking(move || -> Result<RevertResponse, ApiError> {
        let new_oid = repo
            .revert_commit(&commit_oid)
            .map_err(ApiError::from_core)?;
        repo.save().map_err(ApiError::from_core)?;

        Ok(RevertResponse {
            new_commit_id: new_oid.to_string(),
            source_commit_id: source,
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(response))
}

// ── Garbage Collection ──────────────────────────────────────────────────

/// Handler: `POST /api/v1/repos/:id/gc`
pub async fn garbage_collect(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<GcResponse>, ApiError> {
    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let response = tokio::task::spawn_blocking(move || -> Result<GcResponse, ApiError> {
        let result = repo.gc().map_err(ApiError::from_core)?;
        repo.save().map_err(ApiError::from_core)?;

        Ok(GcResponse {
            objects_before: result.objects_before,
            objects_after: result.objects_after,
            objects_removed: result.objects_before.saturating_sub(result.objects_after),
            bytes_before: result.bytes_before,
            bytes_after: result.bytes_after,
            bytes_freed: result.bytes_before.saturating_sub(result.bytes_after),
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(response))
}

// ── Reset ───────────────────────────────────────────────────────────────

/// Handler: `POST /api/v1/repos/:id/reset`
///
/// Supports three modes:
/// - `"soft"`: move HEAD only
/// - `"mixed"` (default): move HEAD and reset the index
/// - `"hard"`: move HEAD, reset the index, and reset the working tree
pub async fn reset(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<ResetRequest>,
) -> Result<Json<ResetResponse>, ApiError> {
    let mode = req.mode.as_str();
    if !matches!(mode, "soft" | "mixed" | "hard") {
        return Err(ApiError::bad_request(
            "mode must be one of: soft, mixed, hard",
        ));
    }

    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (repo, ovc_path) = open_repo_blocking(&app, &id).await?;
    // The working directory is the parent of the `.ovc` file.
    let workdir_path = ovc_path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();

    let response = tokio::task::spawn_blocking(move || execute_reset(repo, req, &workdir_path))
        .await
        .map_err(|e| {
            tracing::error!("task join error: {e}");
            ApiError::internal("internal task error")
        })??;

    Ok(Json(response))
}

/// Blocking implementation of the reset operation.
fn execute_reset(
    mut repo: ovc_core::repository::Repository,
    req: ResetRequest,
    workdir_path: &std::path::Path,
) -> Result<ResetResponse, ApiError> {
    let is_soft = req.mode == "soft";
    let is_hard = req.mode == "hard";

    let target_oid = resolve_reset_target(&req, &repo)?;

    // Verify the target is a commit and retrieve it.
    let target_obj = repo
        .get_object(&target_oid)
        .map_err(ApiError::from_core)?
        .ok_or_else(|| ApiError::not_found(&format!("target commit not found: {target_oid}")))?;
    let ovc_core::object::Object::Commit(commit) = target_obj else {
        return Err(ApiError::bad_request(&format!(
            "target is not a commit: {target_oid}"
        )));
    };

    // Determine the current branch name.
    let branch = match repo.ref_store().head() {
        ovc_core::refs::RefTarget::Symbolic(ref_name) => ref_name
            .strip_prefix("refs/heads/")
            .unwrap_or(ref_name)
            .to_owned(),
        ovc_core::refs::RefTarget::Direct(_) => {
            return Err(ApiError::bad_request("HEAD is detached; cannot reset"));
        }
    };

    // Build identity for the reflog entry.
    // Priority: repo config → OVC_AUTHOR_* env vars → hardcoded "API" fallback.
    let identity_name = {
        let cfg = repo.config().user_name.clone();
        if cfg.is_empty() {
            std::env::var("OVC_AUTHOR_NAME").unwrap_or_else(|_| "API".to_owned())
        } else {
            cfg
        }
    };
    let identity_email = {
        let cfg = repo.config().user_email.clone();
        if cfg.is_empty() {
            std::env::var("OVC_AUTHOR_EMAIL").unwrap_or_else(|_| "api@ovc".to_owned())
        } else {
            cfg
        }
    };
    let identity = ovc_core::object::Identity {
        name: identity_name,
        email: identity_email,
        timestamp: chrono::Utc::now().timestamp(),
        tz_offset_minutes: 0,
    };

    repo.ref_store_mut()
        .set_branch(
            &branch,
            target_oid,
            &identity,
            &format!("reset: moving to {}", &target_oid.to_string()[..12]),
        )
        .map_err(ApiError::from_core)?;

    if !is_soft {
        let tree_oid = commit.tree;
        let (index, store) = repo.index_and_store_mut();
        index
            .read_tree(&tree_oid, store)
            .map_err(ApiError::from_core)?;
    }

    if is_hard {
        reset_working_tree(&repo, &commit, workdir_path)?;
    }

    repo.save().map_err(ApiError::from_core)?;

    Ok(ResetResponse {
        commit_id: target_oid.to_string(),
        mode: req.mode,
    })
}

/// Resolves the target commit OID for a reset operation.
fn resolve_reset_target(
    req: &ResetRequest,
    repo: &ovc_core::repository::Repository,
) -> Result<ovc_core::id::ObjectId, ApiError> {
    if let Some(ref spec) = req.commit_id {
        return resolve_commit_spec(spec, repo);
    }
    // Default: HEAD~1 (parent of current HEAD).
    let head = repo
        .ref_store()
        .resolve_head()
        .map_err(ApiError::from_core)?;
    let head_obj = repo
        .get_object(&head)
        .map_err(ApiError::from_core)?
        .ok_or_else(|| ApiError::not_found("HEAD commit not found"))?;
    match head_obj {
        ovc_core::object::Object::Commit(c) => {
            if c.parents.is_empty() {
                return Err(ApiError::bad_request(
                    "HEAD has no parent; specify a target commit explicitly",
                ));
            }
            Ok(c.parents[0])
        }
        _ => Err(ApiError::bad_request("HEAD does not point to a commit")),
    }
}

/// Resets the working directory to match the target commit tree.
fn reset_working_tree(
    repo: &ovc_core::repository::Repository,
    commit: &ovc_core::object::Commit,
    workdir_path: &std::path::Path,
) -> Result<(), ApiError> {
    let workdir = ovc_core::workdir::WorkDir::new(workdir_path.to_path_buf());
    let ignore = ovc_core::ignore::IgnoreRules::default();
    let workdir_files = workdir
        .scan_files(&ignore)
        .map_err(|e| ApiError::internal(&format!("Failed to scan working directory: {e}")))?;

    let mut target_index = ovc_core::index::Index::new();
    target_index
        .read_tree(&commit.tree, repo.object_store())
        .map_err(ApiError::from_core)?;
    let target_paths: std::collections::BTreeSet<&str> = target_index
        .entries()
        .iter()
        .map(|e| e.path.as_str())
        .collect();

    for wf in &workdir_files {
        if !target_paths.contains(wf.path.as_str()) {
            let _ = workdir.delete_file(&wf.path);
        }
    }

    for entry in target_index.entries() {
        if let Ok(Some(ovc_core::object::Object::Blob(data))) = repo.get_object(&entry.oid) {
            let _ = workdir.write_file(&entry.path, &data, entry.mode);
        }
    }

    Ok(())
}

// ── Reflog ──────────────────────────────────────────────────────────────

/// Handler: `GET /api/v1/repos/:id/reflog`
///
/// Returns the reference log for the current HEAD branch, ordered newest-first.
pub async fn get_reflog(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<ReflogQuery>,
) -> Result<Json<Vec<ReflogEntryResponse>>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;
    let limit = query.limit.min(1000);

    let entries = tokio::task::spawn_blocking(move || -> Vec<ReflogEntryResponse> {
        // Determine the current HEAD reference name to query the reflog.
        let ref_name = match repo.ref_store().head() {
            ovc_core::refs::RefTarget::Symbolic(s) => s.clone(),
            ovc_core::refs::RefTarget::Direct(_) => "HEAD".to_owned(),
        };

        repo.ref_store()
            .get_reflog(&ref_name)
            .into_iter()
            .rev()
            .take(limit)
            .map(|entry| ReflogEntryResponse {
                ref_name: entry.ref_name.clone(),
                old_value: entry.old_value.map(|oid| oid.to_string()),
                new_value: entry.new_value.to_string(),
                identity_name: entry.identity.name.clone(),
                identity_email: entry.identity.email.clone(),
                timestamp: entry.identity.timestamp,
                message: entry.message.clone(),
            })
            .collect()
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })?;

    Ok(Json(entries))
}
