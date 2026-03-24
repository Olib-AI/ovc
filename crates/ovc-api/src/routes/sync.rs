//! Cloud sync endpoints (push/pull/status).

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use crate::auth::Claims;
use crate::error::ApiError;
use crate::models::{PullResponse, PushResponse, SyncStatusResponse};
use crate::routes::repos::open_repo_blocking;
use crate::state::AppState;

/// Handler: `GET /api/v1/repos/:id/sync/status`
pub async fn sync_status(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<SyncStatusResponse>, ApiError> {
    let (repo, ovc_path) = open_repo_blocking(&app, &id).await?;

    // Check if a remote is configured.
    let remote = repo.config().remotes.keys().next().cloned();

    if remote.is_none() {
        return Ok(Json(SyncStatusResponse {
            status: "no_remote".to_owned(),
            remote: None,
            version: None,
        }));
    }

    let remote_name = remote.clone().unwrap_or_default();
    let remote_config = repo
        .config()
        .remotes
        .get(&remote_name)
        .cloned()
        .ok_or_else(|| ApiError::not_found("remote not configured"))?;

    let backend = build_local_backend(&remote_config.url)?;
    let engine = ovc_cloud::SyncEngine::new(backend, id.clone());

    let sync_result = engine
        .status(&ovc_path)
        .await
        .map_err(ApiError::from_cloud)?;

    let (status_str, version) = match sync_result {
        ovc_cloud::SyncStatus::InSync { version } => ("in_sync", Some(version)),
        ovc_cloud::SyncStatus::LocalAhead => ("local_ahead", None),
        ovc_cloud::SyncStatus::RemoteAhead { remote_version } => {
            ("remote_ahead", Some(remote_version))
        }
        ovc_cloud::SyncStatus::Diverged => ("diverged", None),
        ovc_cloud::SyncStatus::NoRemote => ("no_remote", None),
    };

    Ok(Json(SyncStatusResponse {
        status: status_str.to_owned(),
        remote,
        version,
    }))
}

/// Handler: `POST /api/v1/repos/:id/sync/push`
pub async fn sync_push(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<PushResponse>, ApiError> {
    let (repo, ovc_path) = open_repo_blocking(&app, &id).await?;

    let remote_config = repo
        .config()
        .remotes
        .values()
        .next()
        .cloned()
        .ok_or_else(|| ApiError::bad_request("no remote configured"))?;

    let backend = build_local_backend(&remote_config.url)?;
    let engine = ovc_cloud::SyncEngine::new(backend, id);

    let result = engine.push(&ovc_path).await.map_err(ApiError::from_cloud)?;

    Ok(Json(PushResponse {
        chunks_uploaded: result.chunks_uploaded,
        bytes_uploaded: result.bytes_uploaded,
    }))
}

/// Handler: `POST /api/v1/repos/:id/sync/pull`
pub async fn sync_pull(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<PullResponse>, ApiError> {
    let (repo, ovc_path) = open_repo_blocking(&app, &id).await?;

    let remote_config = repo
        .config()
        .remotes
        .values()
        .next()
        .cloned()
        .ok_or_else(|| ApiError::bad_request("no remote configured"))?;

    let backend = build_local_backend(&remote_config.url)?;
    let engine = ovc_cloud::SyncEngine::new(backend, id);

    let result = engine.pull(&ovc_path).await.map_err(ApiError::from_cloud)?;

    Ok(Json(PullResponse {
        chunks_downloaded: result.chunks_downloaded,
        bytes_downloaded: result.bytes_downloaded,
    }))
}

/// Builds a local storage backend from a URL/path.
fn build_local_backend(url: &str) -> Result<Box<dyn ovc_cloud::StorageBackend>, ApiError> {
    let path = std::path::Path::new(url);
    let backend = ovc_cloud::LocalBackend::new(path.to_path_buf())
        .map_err(|e| ApiError::internal(&format!("failed to create storage backend: {e}")))?;
    Ok(Box::new(backend))
}
