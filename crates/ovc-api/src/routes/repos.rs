//! Repository CRUD endpoints.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use crate::auth::Claims;
use crate::error::ApiError;
use crate::models::{CreateRepoRequest, RepoInfo, RepoStats, UnlockRepoRequest};
use crate::state::AppState;

/// Handler: `GET /api/v1/repos`
///
/// Lists all `.ovc` files in the configured repos directory.
///
/// Stats are served from a 30-second in-memory cache to avoid running the
/// Argon2 KDF on every request. Cache misses (first call or stale entry) run
/// the full open-and-stat path on a blocking thread.
pub async fn list_repos(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
) -> Result<Json<Vec<RepoInfo>>, ApiError> {
    let repos_dir = app.repos_dir.clone();

    // Enumerate repo IDs on a blocking thread (directory read is sync I/O).
    let entries: Vec<(String, std::path::PathBuf)> =
        tokio::task::spawn_blocking(move || -> Result<_, ApiError> {
            let mut out = Vec::new();
            let dir_entries = std::fs::read_dir(&repos_dir).map_err(|e| {
                tracing::error!("failed to read repos directory: {e}");
                ApiError::internal("failed to read repos directory")
            })?;
            for entry in dir_entries {
                let entry = entry.map_err(|e| {
                    tracing::error!("failed to read directory entry: {e}");
                    ApiError::internal("failed to read directory entry")
                })?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("ovc") {
                    let id = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default()
                        .to_owned();
                    out.push((id, path));
                }
            }
            Ok(out)
        })
        .await
        .map_err(|e| {
            tracing::error!("task join error: {e}");
            ApiError::internal("internal task error")
        })??;

    // Separate cache hits from misses. Cache hits never touch the KDF.
    let mut result: Vec<RepoInfo> = Vec::with_capacity(entries.len());
    let mut misses: Vec<(String, std::path::PathBuf)> = Vec::new();

    for (id, path) in entries {
        if let Some((head, repo_stats)) = app.get_cached_repo_stats(&id) {
            result.push(RepoInfo {
                name: id.clone(),
                path: path.display().to_string(),
                id,
                head,
                repo_stats,
            });
        } else {
            misses.push((id, path));
        }
    }

    // For cache misses, open repos (KDF-heavy) on a blocking thread.
    if !misses.is_empty() {
        let passwords_snapshot = app
            .passwords
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default();

        let fresh: Vec<(String, String, String, RepoStats)> =
            tokio::task::spawn_blocking(move || -> Result<_, ApiError> {
                let mut out = Vec::with_capacity(misses.len());
                for (id, path) in misses {
                    let (head, repo_stats) = try_read_repo_stats(&path, &passwords_snapshot, &id);
                    let path_str = path.display().to_string();
                    out.push((id, path_str, head, repo_stats));
                }
                Ok(out)
            })
            .await
            .map_err(|e| {
                tracing::error!("task join error: {e}");
                ApiError::internal("internal task error")
            })??;

        for (id, path_str, head, repo_stats) in fresh {
            // Populate cache for next call.
            app.set_cached_repo_stats(&id, head.clone(), repo_stats.clone());
            result.push(RepoInfo {
                name: id.clone(),
                path: path_str,
                id,
                head,
                repo_stats,
            });
        }
    }

    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Json(result))
}

/// Handler: `POST /api/v1/repos`
///
/// Creates a new repository, initializing a `.ovc` file.
pub async fn create_repo(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Json(req): Json<CreateRepoRequest>,
) -> Result<(axum::http::StatusCode, Json<RepoInfo>), ApiError> {
    if req.name.is_empty() {
        return Err(ApiError::bad_request("repository name must not be empty"));
    }

    // Validate name contains only safe characters.
    if !req
        .name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(ApiError::bad_request(
            "repository name may only contain alphanumeric characters, hyphens, underscores, and dots",
        ));
    }

    let repos_dir = app.repos_dir.clone();
    let name = req.name.clone();
    let password = req.password.clone();

    let info = tokio::task::spawn_blocking(move || -> Result<RepoInfo, ApiError> {
        let ovc_path = repos_dir.join(format!("{name}.ovc"));
        let repo = ovc_core::repository::Repository::init(&ovc_path, password.as_bytes())
            .map_err(ApiError::from_core)?;

        let head = repo
            .head_ref()
            .strip_prefix("refs/heads/")
            .unwrap_or_else(|| repo.head_ref())
            .to_owned();
        let repo_stats = build_repo_stats(&repo);

        Ok(RepoInfo {
            id: name.clone(),
            name,
            path: ovc_path.display().to_string(),
            head,
            repo_stats,
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    // Cache the password for the session (zeroized on eviction/drop).
    app.passwords
        .write()
        .map_err(|_| ApiError::internal("lock poisoned"))?
        .insert(info.id.clone(), zeroize::Zeroizing::new(req.password));

    Ok((axum::http::StatusCode::CREATED, Json(info)))
}

/// Validates that a repository ID contains only safe characters.
///
/// Rejects IDs containing path separators, parent-directory references, or any
/// character outside the `[a-zA-Z0-9._-]` set. This prevents path-traversal
/// attacks when the ID is used to construct filesystem paths.
fn validate_repo_id(id: &str) -> Result<(), ApiError> {
    if id.is_empty() {
        return Err(ApiError::bad_request("repository id must not be empty"));
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(ApiError::bad_request("invalid repository id"));
    }
    if !id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(ApiError::bad_request("invalid repository id"));
    }
    Ok(())
}

/// Handler: `GET /api/v1/repos/:id`
///
/// Returns detailed information about a repository. Requires the repo to be unlocked.
pub async fn get_repo(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<RepoInfo>, ApiError> {
    validate_repo_id(&id)?;
    let (repo, ovc_path) = open_repo_blocking(&app, &id).await?;

    let info = tokio::task::spawn_blocking(move || -> RepoInfo {
        let head = repo
            .head_ref()
            .strip_prefix("refs/heads/")
            .unwrap_or_else(|| repo.head_ref())
            .to_owned();
        let repo_stats = build_repo_stats(&repo);
        RepoInfo {
            id: id.clone(),
            name: id,
            path: ovc_path.display().to_string(),
            head,
            repo_stats,
        }
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })?;

    Ok(Json(info))
}

/// Handler: `DELETE /api/v1/repos/:id`
///
/// Deletes a repository file.
pub async fn delete_repo(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    validate_repo_id(&id)?;
    let ovc_path = app.repos_dir.join(format!("{id}.ovc"));

    let delete_result = tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        match std::fs::remove_file(&ovc_path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(ApiError::not_found("repository not found"))
            }
            Err(e) => {
                tracing::error!("failed to delete repository: {e}");
                Err(ApiError::internal("failed to delete repository"))
            }
        }
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })?;

    // Always evict the password cache entry regardless of deletion outcome.
    if let Ok(mut passwords) = app.passwords.write() {
        passwords.remove(&id);
    }

    // Evict the per-repo mutex to prevent unbounded growth of the repo_locks
    // map. Without this, every unique repo id that was ever accessed would
    // retain an `Arc<Mutex>` for the lifetime of the server process.
    if let Ok(mut locks) = app.repo_locks.write() {
        locks.remove(&id);
    }

    delete_result?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Handler: `GET /api/v1/repos/:id/config`
///
/// Returns the repository's editable configuration (author identity, default branch).
pub async fn get_repo_config(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<crate::models::RepoConfigResponse>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let resp = tokio::task::spawn_blocking(move || {
        let config = repo.config();
        // When the stored config has empty user fields, fall back to
        // environment variables so the Settings page shows what the CLI
        // would actually use for commits.
        let user_name = if config.user_name.is_empty() {
            std::env::var("OVC_AUTHOR_NAME").unwrap_or_default()
        } else {
            config.user_name.clone()
        };
        let user_email = if config.user_email.is_empty() {
            std::env::var("OVC_AUTHOR_EMAIL").unwrap_or_default()
        } else {
            config.user_email.clone()
        };
        crate::models::RepoConfigResponse {
            user_name,
            user_email,
            default_branch: config.default_branch.clone(),
        }
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })?;

    Ok(Json(resp))
}

/// Handler: `PUT /api/v1/repos/:id/config`
///
/// Updates the repository's configuration (author identity, default branch).
pub async fn update_repo_config(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<crate::models::UpdateRepoConfigRequest>,
) -> Result<Json<crate::models::RepoConfigResponse>, ApiError> {
    // Validate inputs.
    if let Some(ref name) = req.user_name
        && name.len() > 256
    {
        return Err(ApiError::bad_request(
            "user_name must not exceed 256 characters",
        ));
    }
    if let Some(ref email) = req.user_email
        && email.len() > 256
    {
        return Err(ApiError::bad_request(
            "user_email must not exceed 256 characters",
        ));
    }
    if let Some(ref branch) = req.default_branch {
        crate::routes::validate_ref_name(branch)?;
    }

    let lock = app.repo_lock(&id);
    let _guard = lock.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let resp = tokio::task::spawn_blocking(
        move || -> Result<crate::models::RepoConfigResponse, ApiError> {
            let config = repo.config_mut();
            if let Some(name) = req.user_name {
                config.user_name = name;
            }
            if let Some(email) = req.user_email {
                config.user_email = email;
            }
            if let Some(branch) = req.default_branch {
                config.default_branch = branch;
            }
            let resp = crate::models::RepoConfigResponse {
                user_name: config.user_name.clone(),
                user_email: config.user_email.clone(),
                default_branch: config.default_branch.clone(),
            };
            repo.save().map_err(ApiError::from_core)?;
            Ok(resp)
        },
    )
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(resp))
}

/// Handler: `POST /api/v1/repos/:id/unlock`
///
/// Provides a password to unlock a repository for the current session.
/// Validates the password by attempting to open the repository.
pub async fn unlock_repo(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UnlockRepoRequest>,
) -> Result<axum::http::StatusCode, ApiError> {
    validate_repo_id(&id)?;
    let ovc_path = app.repos_dir.join(format!("{id}.ovc"));
    let password = req.password.clone();

    // Validate the password by attempting to open.
    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let _repo = ovc_core::repository::Repository::open(&ovc_path, password.as_bytes())
            .map_err(ApiError::from_core)?;
        Ok(())
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    app.passwords
        .write()
        .map_err(|_| ApiError::internal("lock poisoned"))?
        .insert(id, zeroize::Zeroizing::new(req.password));

    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Opens a repository using key-based auth (from env) or cached password,
/// running the CPU-bound crypto on a blocking thread.
///
/// Priority:
/// 1. `OVC_KEY` env var → key-based auth (no unlock needed)
/// 2. Cached password from `/repos/:id/unlock` → password-based auth
pub async fn open_repo_blocking(
    app: &AppState,
    repo_id: &str,
) -> Result<(ovc_core::repository::Repository, std::path::PathBuf), ApiError> {
    validate_repo_id(repo_id)?;

    let ovc_path = app.repos_dir.join(format!("{repo_id}.ovc"));

    // Try key-based auth first (from environment variables).
    //
    // If the key is found and valid but the repository has no key slot for it
    // (password-only repo), fall through to password-based auth rather than
    // failing hard. This allows the server to open password repos when `OVC_KEY`
    // is set in the environment (common in developer setups where the env var is
    // always present but not all repos have been re-keyed).
    // Try key-based auth. If the key doesn't apply to this repo (password-only
    // repo, or no matching key slot), fall through to password-based auth.
    // This handles the common case where OVC_KEY is set globally but some repos
    // were created with passwords and haven't been re-keyed yet.
    if let Ok(key_query) = std::env::var("OVC_KEY") {
        let path_for_key = ovc_path.clone();
        let key_result = tokio::task::spawn_blocking(move || -> Result<_, ApiError> {
            let pub_path = match ovc_core::keys::find_key(&key_query) {
                Ok(Some(p)) => p,
                Ok(None) => {
                    // Key not found — signal caller to fall through to password auth.
                    return Err(ApiError::unauthorized("__key_not_applicable__"));
                }
                Err(e) => {
                    tracing::error!("key search failed: {e}");
                    return Err(ApiError::internal("key search failed"));
                }
            };

            let priv_path = ovc_core::keys::private_key_path_for(&pub_path);
            if !priv_path.exists() {
                // Private key file absent — signal caller to fall through to password auth.
                return Err(ApiError::unauthorized("__key_not_applicable__"));
            }

            let passphrase = std::env::var("OVC_KEY_PASSPHRASE").unwrap_or_default();

            let keypair =
                ovc_core::keys::OvcKeyPair::load_private(&priv_path, passphrase.as_bytes())
                    .map_err(|e| {
                        tracing::error!("failed to load private key: {e}");
                        ApiError::internal("failed to load private key")
                    })?;

            ovc_core::repository::Repository::open_with_key(&path_for_key, &keypair).map_err(|e| {
                use ovc_core::error::CoreError;
                match &e {
                    // Repo has no key slots or this key is not in any slot —
                    // signal caller to fall through to password auth.
                    CoreError::DecryptionFailed { .. } => {
                        ApiError::unauthorized("__key_not_applicable__")
                    }
                    _ => ApiError::from_core(e),
                }
            })
        })
        .await
        .map_err(|e| {
            tracing::error!("task join error: {e}");
            ApiError::internal("internal task error")
        })?;

        match key_result {
            Ok(repo) => return Ok((repo, ovc_path)),
            Err(ref api_err) if api_err.message == "__key_not_applicable__" => {
                // Key-based auth not applicable for this repo — continue to
                // password-based auth below.
            }
            Err(e) => return Err(e),
        }
    }

    // Fall back to password-based auth from cache.
    let password = app
        .passwords
        .read()
        .map_err(|_| ApiError::internal("lock poisoned"))?
        .get(repo_id)
        .cloned()
        .ok_or_else(|| {
            ApiError::unauthorized("repository not unlocked -- call POST /repos/:id/unlock first, or set OVC_KEY env var")
        })?;

    let ovc_path_for_pw = ovc_path.clone();
    let repo = tokio::task::spawn_blocking(move || -> Result<_, ApiError> {
        ovc_core::repository::Repository::open(&ovc_path_for_pw, password.as_bytes())
            .map_err(ApiError::from_core)
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok((repo, ovc_path))
}

/// Builds `RepoStats` from a repository handle.
fn build_repo_stats(repo: &ovc_core::repository::Repository) -> RepoStats {
    let branches = repo.ref_store().list_branches();
    let tags = repo.ref_store().list_tags();
    let tracked = repo.index().entries().len();
    let total_commits = count_commits(repo);

    RepoStats {
        total_commits,
        total_branches: branches.len() as u64,
        total_tags: tags.len() as u64,
        tracked_files: tracked as u64,
    }
}

/// Counts all commits reachable from HEAD by walking all parent links (BFS).
///
/// Unlike the log endpoint (which follows first-parent only, matching `git log`
/// default behavior), this uses BFS with a visited set to ensure merge parents
/// are counted accurately.
fn count_commits(repo: &ovc_core::repository::Repository) -> u64 {
    let Some(head_oid) = repo.ref_store().resolve_head().ok() else {
        return 0;
    };

    let mut visited = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(head_oid);
    visited.insert(head_oid);

    let mut count = 0u64;

    while let Some(oid) = queue.pop_front() {
        if let Ok(Some(ovc_core::object::Object::Commit(commit))) = repo.get_object(&oid) {
            count += 1;
            for &parent_oid in &commit.parents {
                if visited.insert(parent_oid) {
                    queue.push_back(parent_oid);
                }
            }
        }
    }

    count
}

/// Attempts to open a repo and read stats. Falls back to empty stats on failure.
///
/// Tries key-based auth from env vars first, then password-based auth from the
/// cached passwords map.
fn try_read_repo_stats(
    path: &std::path::Path,
    passwords: &std::collections::HashMap<String, zeroize::Zeroizing<String>>,
    repo_id: &str,
) -> (String, RepoStats) {
    let empty = (
        String::new(),
        RepoStats {
            total_commits: 0,
            total_branches: 0,
            total_tags: 0,
            tracked_files: 0,
        },
    );

    // Try key-based auth from environment.
    if let Ok(key_query) = std::env::var("OVC_KEY") {
        let Ok(Some(pub_path)) = ovc_core::keys::find_key(&key_query) else {
            return try_password_auth(path, passwords, repo_id, &empty);
        };
        let priv_path = ovc_core::keys::private_key_path_for(&pub_path);
        if !priv_path.exists() {
            return try_password_auth(path, passwords, repo_id, &empty);
        }
        let passphrase = std::env::var("OVC_KEY_PASSPHRASE").unwrap_or_default();
        let Ok(keypair) =
            ovc_core::keys::OvcKeyPair::load_private(&priv_path, passphrase.as_bytes())
        else {
            return try_password_auth(path, passwords, repo_id, &empty);
        };
        if let Ok(repo) = ovc_core::repository::Repository::open_with_key(path, &keypair) {
            let raw = repo.head_ref();
            let head = raw.strip_prefix("refs/heads/").unwrap_or(raw).to_owned();
            let stats = build_repo_stats(&repo);
            return (head, stats);
        }
    }

    try_password_auth(path, passwords, repo_id, &empty)
}

/// Attempts password-based auth from the cached passwords map.
fn try_password_auth(
    path: &std::path::Path,
    passwords: &std::collections::HashMap<String, zeroize::Zeroizing<String>>,
    repo_id: &str,
    empty: &(String, RepoStats),
) -> (String, RepoStats) {
    if let Some(password) = passwords.get(repo_id)
        && let Ok(repo) = ovc_core::repository::Repository::open(path, password.as_bytes())
    {
        let raw = repo.head_ref();
        let head = raw.strip_prefix("refs/heads/").unwrap_or(raw).to_owned();
        let stats = build_repo_stats(&repo);
        return (head, stats);
    }
    empty.clone()
}
