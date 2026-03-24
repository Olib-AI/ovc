//! Tag CRUD endpoints.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use crate::auth::Claims;
use crate::error::ApiError;
use crate::models::{CreateTagRequest, TagInfo};
use crate::routes::repos::open_repo_blocking;
use crate::routes::validate_ref_name;
use crate::state::AppState;

/// Handler: `GET /api/v1/repos/:id/tags`
pub async fn list_tags(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<TagInfo>>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let tags = tokio::task::spawn_blocking(move || -> Vec<TagInfo> {
        repo.ref_store()
            .list_tags()
            .into_iter()
            .map(|(name, oid, msg)| TagInfo {
                name: name.to_owned(),
                commit_id: oid.to_string(),
                message: msg.map(str::to_owned),
            })
            .collect()
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })?;

    Ok(Json(tags))
}

/// Handler: `POST /api/v1/repos/:id/tags`
pub async fn create_tag(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<CreateTagRequest>,
) -> Result<Json<TagInfo>, ApiError> {
    validate_ref_name(&req.name)?;

    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let info = tokio::task::spawn_blocking(move || -> Result<TagInfo, ApiError> {
        let target_oid = if let Some(ref spec) = req.commit_id {
            crate::routes::resolve_commit_spec(spec, &repo)?
        } else {
            repo.ref_store()
                .resolve_head()
                .map_err(ApiError::from_core)?
        };

        repo.ref_store_mut()
            .create_tag(&req.name, target_oid, req.message.as_deref())
            .map_err(ApiError::from_core)?;

        repo.save().map_err(ApiError::from_core)?;

        Ok(TagInfo {
            name: req.name,
            commit_id: target_oid.to_string(),
            message: req.message,
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    // Invalidate cached stats: tag count changed.
    app.invalidate_repo_stats(&id);

    Ok(Json(info))
}

/// Handler: `DELETE /api/v1/repos/:id/tags/:name`
pub async fn delete_tag(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, name)): Path<(String, String)>,
) -> Result<axum::http::StatusCode, ApiError> {
    validate_ref_name(&name)?;
    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        repo.ref_store_mut()
            .delete_tag(&name)
            .map_err(ApiError::from_core)?;
        repo.save().map_err(ApiError::from_core)?;
        Ok(())
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    // Invalidate cached stats: tag count changed.
    app.invalidate_repo_stats(&id);

    Ok(axum::http::StatusCode::NO_CONTENT)
}
