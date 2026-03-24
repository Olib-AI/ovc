//! Remote management endpoints: list, add, remove.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use ovc_core::config::RemoteConfig;

use crate::auth::Claims;
use crate::error::ApiError;
use crate::models::{AddRemoteRequest, RemoteInfo};
use crate::routes::repos::open_repo_blocking;
use crate::state::AppState;

/// Handler: `GET /api/v1/repos/:id/remotes`
///
/// Returns all configured remotes for the repository.
pub async fn list_remotes(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<RemoteInfo>>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let remotes: Vec<RemoteInfo> = repo
        .config()
        .remotes
        .iter()
        .map(|(name, config)| RemoteInfo {
            name: name.clone(),
            url: config.url.clone(),
            backend_type: config.backend_type.clone(),
        })
        .collect();

    Ok(Json(remotes))
}

/// Handler: `POST /api/v1/repos/:id/remotes`
///
/// Adds a new remote to the repository configuration.
pub async fn add_remote(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<AddRemoteRequest>,
) -> Result<Json<RemoteInfo>, ApiError> {
    if req.name.is_empty() {
        return Err(ApiError::bad_request("remote name must not be empty"));
    }
    if req.url.is_empty() {
        return Err(ApiError::bad_request("remote URL must not be empty"));
    }

    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let response = tokio::task::spawn_blocking(move || -> Result<RemoteInfo, ApiError> {
        let config = repo.config_mut();
        if config.remotes.contains_key(&req.name) {
            return Err(ApiError::conflict(&format!(
                "remote '{}' already exists",
                req.name
            )));
        }

        config.remotes.insert(
            req.name.clone(),
            RemoteConfig {
                url: req.url.clone(),
                backend_type: req.backend_type.clone(),
            },
        );

        repo.save().map_err(ApiError::from_core)?;

        Ok(RemoteInfo {
            name: req.name,
            url: req.url,
            backend_type: req.backend_type,
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(response))
}

/// Handler: `DELETE /api/v1/repos/:id/remotes/:name`
///
/// Removes a remote from the repository configuration.
pub async fn remove_remote(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, name)): Path<(String, String)>,
) -> Result<axum::http::StatusCode, ApiError> {
    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let config = repo.config_mut();
        if config.remotes.remove(&name).is_none() {
            return Err(ApiError::not_found(&format!("remote '{name}' not found")));
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
