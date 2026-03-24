//! Branch CRUD and merge endpoints.

use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use crate::auth::Claims;
use crate::error::ApiError;
use crate::models::{BranchInfo, CreateBranchRequest, MergeRequest, MergeResponse};
use crate::routes::repos::open_repo_blocking;
use crate::routes::{resolve_commit_spec, validate_ref_name};
use crate::state::AppState;

/// Handler: `GET /api/v1/repos/:id/branches`
pub async fn list_branches(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<BranchInfo>>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let branches = tokio::task::spawn_blocking(move || -> Vec<BranchInfo> {
        let current_branch = match repo.ref_store().head() {
            ovc_core::refs::RefTarget::Symbolic(s) => {
                s.strip_prefix("refs/heads/").unwrap_or(s).to_owned()
            }
            ovc_core::refs::RefTarget::Direct(_) => String::new(),
        };

        repo.ref_store()
            .list_branches()
            .into_iter()
            .map(|(name, oid)| BranchInfo {
                name: name.to_owned(),
                commit_id: oid.to_string(),
                is_current: name == current_branch,
            })
            .collect()
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })?;

    Ok(Json(branches))
}

/// Handler: `POST /api/v1/repos/:id/branches`
pub async fn create_branch(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<CreateBranchRequest>,
) -> Result<Json<BranchInfo>, ApiError> {
    validate_ref_name(&req.name)?;

    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let info = tokio::task::spawn_blocking(move || -> Result<BranchInfo, ApiError> {
        // Guard: HEAD must resolve to create a branch (requires at least one commit).
        if repo.ref_store().resolve_head().is_err() {
            return Err(ApiError::bad_request(
                "cannot create branch: repository has no commits yet — make an initial commit first",
            ));
        }

        if let Some(ref spec) = req.start_point {
            // Resolve the start_point to a concrete commit OID.
            let target_oid = resolve_commit_spec(spec, &repo)?;

            // Verify the resolved OID actually points to a commit.
            let obj = repo
                .get_object(&target_oid)
                .map_err(ApiError::from_core)?
                .ok_or_else(|| {
                    ApiError::not_found(&format!("object not found: {target_oid}"))
                })?;
            if !matches!(obj, ovc_core::object::Object::Commit(_)) {
                return Err(ApiError::bad_request(&format!(
                    "start_point '{spec}' does not resolve to a commit"
                )));
            }

            // Build identity for the reflog entry.
            let identity = ovc_core::object::Identity {
                name: repo.config().user_name.clone(),
                email: repo.config().user_email.clone(),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX)),
                tz_offset_minutes: 0,
            };

            repo.ref_store_mut()
                .set_branch(
                    &req.name,
                    target_oid,
                    &identity,
                    &format!("branch: created {} from {}", req.name, spec),
                )
                .map_err(ApiError::from_core)?;
        } else {
            repo.create_branch(&req.name).map_err(ApiError::from_core)?;
        }

        repo.save().map_err(ApiError::from_core)?;

        let oid = repo
            .ref_store()
            .resolve(&format!("refs/heads/{}", req.name))
            .map_err(ApiError::from_core)?;

        Ok(BranchInfo {
            name: req.name,
            commit_id: oid.to_string(),
            is_current: false,
        })
    })
    .await
    .map_err(|e| { tracing::error!("task join error: {e}"); ApiError::internal("internal task error") })??;

    // Invalidate cached stats: branch count changed.
    app.invalidate_repo_stats(&id);

    Ok(Json(info))
}

/// Handler: `DELETE /api/v1/repos/:id/branches/:name`
pub async fn delete_branch(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, name)): Path<(String, String)>,
) -> Result<axum::http::StatusCode, ApiError> {
    validate_ref_name(&name)?;
    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        // The core layer also performs this check, but we produce a clearer
        // 409 Conflict here rather than the generic 400 from CoreError::Config.
        let full_ref = if name.starts_with("refs/heads/") {
            name.clone()
        } else {
            format!("refs/heads/{name}")
        };
        if let ovc_core::refs::RefTarget::Symbolic(head_target) = repo.ref_store().head()
            && *head_target == full_ref
        {
            return Err(ApiError::conflict(
                "cannot delete the currently checked-out branch",
            ));
        }

        // Prevent deletion of the repository's default branch regardless of
        // what HEAD currently points to.
        let default_branch_ref = format!("refs/heads/{}", repo.config().default_branch);
        if full_ref == default_branch_ref {
            return Err(ApiError::conflict("cannot delete the default branch"));
        }

        repo.delete_branch(&name).map_err(ApiError::from_core)?;
        repo.save().map_err(ApiError::from_core)?;
        Ok(())
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    // Invalidate cached stats: branch count changed.
    app.invalidate_repo_stats(&id);

    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Checks whether the repository has uncommitted changes (staged or unstaged).
///
/// Staged changes are detected by comparing current index entries against the
/// HEAD tree. Unstaged changes are detected by hashing each on-disk file and
/// comparing the result against the stored blob OID in the index.
///
/// Returns `Err(ApiError::conflict(...))` when dirty state is found, `Ok(())`
/// when the working tree is clean.
fn check_for_dirty_state(
    repo: &ovc_core::repository::Repository,
    workdir_path: Option<&PathBuf>,
) -> Result<(), ApiError> {
    // ── 1. Staged changes: index vs HEAD tree ────────────────────────────
    let head_tree_oid = repo.ref_store().resolve_head().ok().and_then(|commit_oid| {
        repo.get_object(&commit_oid)
            .ok()
            .flatten()
            .and_then(|obj| match obj {
                ovc_core::object::Object::Commit(c) => Some(c.tree),
                _ => None,
            })
    });

    let mut head_index = ovc_core::index::Index::new();
    if let Some(tree_oid) = head_tree_oid {
        head_index
            .read_tree(&tree_oid, repo.object_store())
            .map_err(ApiError::from_core)?;
    }

    // Build a (path -> oid) map for both HEAD and the current index.
    let head_map: std::collections::BTreeMap<&str, ovc_core::id::ObjectId> = head_index
        .entries()
        .iter()
        .map(|e| (e.path.as_str(), e.oid))
        .collect();
    let index_map: std::collections::BTreeMap<&str, ovc_core::id::ObjectId> = repo
        .index()
        .entries()
        .iter()
        .map(|e| (e.path.as_str(), e.oid))
        .collect();

    let all_paths: std::collections::BTreeSet<&str> =
        head_map.keys().chain(index_map.keys()).copied().collect();

    for path in &all_paths {
        if head_map.get(path) != index_map.get(path) {
            return Err(ApiError::conflict(
                "Cannot switch branches: you have uncommitted changes. \
                 Commit or stash them first.",
            ));
        }
    }

    // ── 2. Unstaged changes: on-disk files vs index ──────────────────────
    let Some(wd_path) = workdir_path else {
        // No working directory available — skip unstaged check.
        return Ok(());
    };

    for entry in repo.index().entries() {
        let disk_path = wd_path.join(&entry.path);
        let Ok(bytes) = std::fs::read(&disk_path) else {
            // File deleted on disk but still in index — that is a change.
            return Err(ApiError::conflict(
                "Cannot switch branches: you have uncommitted changes. \
                 Commit or stash them first.",
            ));
        };
        let disk_oid = ovc_core::id::hash_blob(&bytes);
        if disk_oid != entry.oid {
            return Err(ApiError::conflict(
                "Cannot switch branches: you have uncommitted changes. \
                 Commit or stash them first.",
            ));
        }
    }

    Ok(())
}

/// Handler: `POST /api/v1/repos/:id/branches/:name/checkout`
pub async fn checkout_branch(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, name)): Path<(String, String)>,
) -> Result<axum::http::StatusCode, ApiError> {
    validate_ref_name(&name)?;

    // Resolve the working directory before entering the blocking task — this
    // requires `&AppState` which is not `Send` across the spawn boundary.
    let workdir_path = crate::routes::files::find_workdir_for_repo(&app, &id);

    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let full_name = if name.starts_with("refs/heads/") {
            name
        } else {
            format!("refs/heads/{name}")
        };

        // Verify branch exists.
        repo.ref_store()
            .resolve(&full_name)
            .map_err(ApiError::from_core)?;

        // Guard: refuse to switch branches when the working tree is dirty.
        check_for_dirty_state(&repo, workdir_path.as_ref())?;

        // Update HEAD symbolic ref in the ref store.
        repo.ref_store_mut()
            .set_head(ovc_core::refs::RefTarget::Symbolic(full_name.clone()));

        // Keep superblock.head_ref in sync with the ref store's HEAD target.
        // This mirrors what checkout_branch() does in the core layer.
        repo.set_head_ref(full_name);

        // Read the target tree into the index and capture the commit for
        // the working-directory update below.
        let commit_oid = repo
            .ref_store()
            .resolve_head()
            .map_err(ApiError::from_core)?;
        let target_commit = repo
            .get_object(&commit_oid)
            .map_err(ApiError::from_core)?
            .and_then(|obj| {
                if let ovc_core::object::Object::Commit(c) = obj {
                    Some(c)
                } else {
                    None
                }
            });

        if let Some(ref commit) = target_commit {
            let (index, object_store) = repo.index_and_store_mut();
            index
                .read_tree(&commit.tree, object_store)
                .map_err(ApiError::from_core)?;
        }

        // Update the working directory when one is available.
        // On bare repos or repos-dir mode, `workdir_path` is `None` and we
        // skip this step silently — the index update above is still correct.
        if let (Some(ref commit), Some(ref wd_path)) = (target_commit, workdir_path) {
            let workdir = ovc_core::workdir::WorkDir::new(wd_path.clone());
            let ignore = ovc_core::ignore::IgnoreRules::default();
            let workdir_files = workdir.scan_files(&ignore).map_err(|e| {
                ApiError::internal(&format!("failed to scan working directory: {e}"))
            })?;

            // Build the target index to know which paths should exist after checkout.
            let mut target_index = ovc_core::index::Index::new();
            target_index
                .read_tree(&commit.tree, repo.object_store())
                .map_err(ApiError::from_core)?;
            let target_paths: std::collections::BTreeSet<&str> = target_index
                .entries()
                .iter()
                .map(|e| e.path.as_str())
                .collect();

            // Remove files that belong to the previous branch but not the new one.
            for wf in &workdir_files {
                if !target_paths.contains(wf.path.as_str()) {
                    let _ = workdir.delete_file(&wf.path);
                }
            }

            // Write (or overwrite) files from the target branch tree.
            for entry in target_index.entries() {
                if let Ok(Some(ovc_core::object::Object::Blob(data))) = repo.get_object(&entry.oid)
                {
                    workdir
                        .write_file(&entry.path, &data, entry.mode)
                        .map_err(|e| {
                            ApiError::internal(&format!(
                                "failed to write '{}' to working directory: {e}",
                                entry.path
                            ))
                        })?;
                }
            }
        }

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

/// Handler: `POST /api/v1/repos/:id/branches/:name/merge`
#[allow(clippy::too_many_lines)]
pub async fn merge_branch(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, name)): Path<(String, String)>,
    Json(req): Json<MergeRequest>,
) -> Result<Json<MergeResponse>, ApiError> {
    validate_ref_name(&req.source_branch)?;
    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let response = tokio::task::spawn_blocking(move || -> Result<MergeResponse, ApiError> {
        // Validate that the URL branch name matches the current HEAD branch.
        let current_branch = match repo.ref_store().head() {
            ovc_core::refs::RefTarget::Symbolic(s) => {
                s.strip_prefix("refs/heads/").unwrap_or(s).to_owned()
            }
            ovc_core::refs::RefTarget::Direct(_) => String::new(),
        };
        if current_branch != name {
            return Err(ApiError::conflict(&format!(
                "HEAD is on branch '{current_branch}', not '{name}' — \
                 checkout '{name}' first before merging into it"
            )));
        }

        // Validate author identity before proceeding with the merge.
        if repo.config().user_name.is_empty() || repo.config().user_email.is_empty() {
            return Err(ApiError::bad_request(
                "user.name and user.email must be configured for merge commits",
            ));
        }

        // Resolve HEAD (ours).
        let our_oid = repo
            .ref_store()
            .resolve_head()
            .map_err(ApiError::from_core)?;
        let Some(ovc_core::object::Object::Commit(our_commit)) =
            repo.get_object(&our_oid).map_err(ApiError::from_core)?
        else {
            return Err(ApiError::internal("HEAD does not point to a commit"));
        };

        // Resolve source branch (theirs).
        let source_ref = if req.source_branch.starts_with("refs/heads/") {
            req.source_branch.clone()
        } else {
            format!("refs/heads/{}", req.source_branch)
        };
        let their_oid = repo
            .ref_store()
            .resolve(&source_ref)
            .map_err(ApiError::from_core)?;

        if our_oid == their_oid {
            return Ok(MergeResponse {
                status: "already_up_to_date".to_owned(),
                commit_id: Some(our_oid.to_string()),
                conflict_files: Vec::new(),
                message: "Already up to date".to_owned(),
            });
        }

        let Some(ovc_core::object::Object::Commit(their_commit)) =
            repo.get_object(&their_oid).map_err(ApiError::from_core)?
        else {
            return Err(ApiError::internal(
                "source branch does not point to a commit",
            ));
        };

        // Find merge base.
        let base_oid = find_merge_base(&repo, our_oid, their_oid);

        let base_tree = if let Some(base) = base_oid {
            let Some(ovc_core::object::Object::Commit(c)) =
                repo.get_object(&base).map_err(ApiError::from_core)?
            else {
                return Err(ApiError::internal("merge base is not a commit"));
            };
            c.tree
        } else {
            // No common ancestor: use empty tree.
            let empty_tree = ovc_core::object::Object::Tree(ovc_core::object::Tree {
                entries: Vec::new(),
            });
            repo.insert_object(&empty_tree)
                .map_err(ApiError::from_core)?
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
            let msg = format!(
                "Merge conflict detected in {} file(s)",
                conflict_paths.len()
            );
            return Ok(MergeResponse {
                status: "conflict".to_owned(),
                commit_id: None,
                conflict_files: conflict_paths,
                message: msg,
            });
        }

        // Build merged tree.
        let merged_tree = ovc_core::object::Object::Tree(ovc_core::object::Tree {
            entries: merge_result.entries,
        });
        let merged_tree_oid = repo
            .insert_object(&merged_tree)
            .map_err(ApiError::from_core)?;

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

        let merge_message = format!("Merge branch '{}'", req.source_branch);
        let commit_oid = repo
            .create_commit(&merge_message, &author)
            .map_err(ApiError::from_core)?;

        repo.save().map_err(ApiError::from_core)?;

        Ok(MergeResponse {
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

/// Simple merge base finder: walks both ancestor chains and returns the first common commit.
///
/// Uses visited sets for both chains to prevent infinite loops on corrupt
/// commit graphs that contain cycles.
fn find_merge_base(
    repo: &ovc_core::repository::Repository,
    oid_a: ovc_core::id::ObjectId,
    oid_b: ovc_core::id::ObjectId,
) -> Option<ovc_core::id::ObjectId> {
    // Collect ancestors of A with cycle detection.
    let mut ancestors_a = std::collections::HashSet::new();
    let mut current = Some(oid_a);
    while let Some(oid) = current {
        if !ancestors_a.insert(oid) {
            break; // Cycle detected in chain A.
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

    // Walk ancestors of B looking for intersection, with cycle detection.
    let mut visited_b = std::collections::HashSet::new();
    let mut current = Some(oid_b);
    while let Some(oid) = current {
        if ancestors_a.contains(&oid) {
            return Some(oid);
        }
        if !visited_b.insert(oid) {
            break; // Cycle detected in chain B.
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

    None
}
