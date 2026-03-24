//! Submodule management endpoints.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use crate::auth::Claims;
use crate::error::ApiError;
use crate::models::{AddSubmoduleRequest, SubmoduleInfo};
use crate::routes::repos::open_repo_blocking;
use crate::state::AppState;

/// Handler: `GET /api/v1/repos/:id/submodules`
///
/// Lists all configured submodules.
pub async fn list_submodules(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<SubmoduleInfo>>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let submodules = tokio::task::spawn_blocking(move || -> Vec<SubmoduleInfo> {
        repo.submodules()
            .iter()
            .map(|(name, config)| SubmoduleInfo {
                name: name.clone(),
                path: config.path.clone(),
                url: config.url.clone(),
                ovc_file: config.ovc_file.clone(),
                pinned_sequence: config.pinned_sequence,
                status: config.status.as_str().to_owned(),
            })
            .collect()
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })?;

    Ok(Json(submodules))
}

/// Handler: `POST /api/v1/repos/:id/submodules`
///
/// Adds a new submodule configuration.
pub async fn add_submodule(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<AddSubmoduleRequest>,
) -> Result<Json<SubmoduleInfo>, ApiError> {
    if req.name.is_empty() {
        return Err(ApiError::bad_request("submodule name must not be empty"));
    }

    // Derive ovc_file from the submodule name when the frontend omits it.
    let ovc_file = match req.ovc_file {
        Some(ref f) if !f.is_empty() => f.clone(),
        _ => format!("{}.ovc", req.name),
    };

    // Validate path and URL via the core validation logic before touching
    // persistent state. This rejects path traversal, absolute paths, and
    // obviously invalid URLs at the API boundary.
    {
        use ovc_core::submodule::{SubmoduleConfig, SubmoduleStatus};
        let probe = SubmoduleConfig {
            path: req.path.clone(),
            url: req.url.clone(),
            ovc_file: ovc_file.clone(),
            pinned_sequence: 0,
            status: SubmoduleStatus::Configured,
        };
        probe
            .validate()
            .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    }

    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let response = tokio::task::spawn_blocking(move || -> Result<SubmoduleInfo, ApiError> {
        if repo.submodules().contains_key(&req.name) {
            return Err(ApiError::conflict(&format!(
                "submodule '{}' already exists",
                req.name
            )));
        }

        let config = ovc_core::submodule::SubmoduleConfig {
            path: req.path.clone(),
            url: req.url.clone(),
            ovc_file: ovc_file.clone(),
            pinned_sequence: 0,
            status: ovc_core::submodule::SubmoduleStatus::Configured,
        };

        repo.submodules_mut().insert(req.name.clone(), config);
        repo.save().map_err(ApiError::from_core)?;

        Ok(SubmoduleInfo {
            name: req.name,
            path: req.path,
            url: req.url,
            ovc_file,
            pinned_sequence: 0,
            status: ovc_core::submodule::SubmoduleStatus::Configured
                .as_str()
                .to_owned(),
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(response))
}

/// Handler: `DELETE /api/v1/repos/:id/submodules/:name`
///
/// Removes a submodule configuration.
pub async fn remove_submodule(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, name)): Path<(String, String)>,
) -> Result<axum::http::StatusCode, ApiError> {
    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        if repo.submodules_mut().remove(&name).is_none() {
            return Err(ApiError::not_found(&format!(
                "submodule '{name}' not found"
            )));
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
