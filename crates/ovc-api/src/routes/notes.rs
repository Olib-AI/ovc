//! Commit notes CRUD endpoints.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use crate::auth::Claims;
use crate::error::ApiError;
use crate::models::{NoteResponse, SetNoteRequest};
use crate::routes::repos::open_repo_blocking;
use crate::state::AppState;

/// Handler: `GET /api/v1/repos/:id/notes`
///
/// Lists all notes across all commits.
pub async fn list_notes(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<NoteResponse>>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let notes = tokio::task::spawn_blocking(move || -> Vec<NoteResponse> {
        repo.notes()
            .iter()
            .map(|(oid, message)| NoteResponse {
                commit_id: oid.to_string(),
                message: message.clone(),
            })
            .collect()
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })?;

    Ok(Json(notes))
}

/// Handler: `GET /api/v1/repos/:id/notes/:commit_id`
///
/// Retrieves the note attached to a specific commit.
pub async fn get_note(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, commit_id)): Path<(String, String)>,
) -> Result<Json<NoteResponse>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let response = tokio::task::spawn_blocking(move || -> Result<NoteResponse, ApiError> {
        let oid: ovc_core::id::ObjectId = commit_id
            .parse()
            .map_err(|_| ApiError::bad_request("invalid commit id format"))?;

        let message = ovc_core::notes::get_note(repo.notes(), &oid)
            .ok_or_else(|| ApiError::not_found("no note found for this commit"))?;

        Ok(NoteResponse {
            commit_id: oid.to_string(),
            message: message.clone(),
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(response))
}

/// Handler: `PUT /api/v1/repos/:id/notes/:commit_id`
///
/// Creates or updates a note on a commit.
pub async fn set_note(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, commit_id)): Path<(String, String)>,
    Json(req): Json<SetNoteRequest>,
) -> Result<Json<NoteResponse>, ApiError> {
    if req.message.is_empty() {
        return Err(ApiError::bad_request("note message must not be empty"));
    }
    // Notes are stored inline in the superblock, which is serialized on
    // every save. Cap the size to prevent a single API call from inflating
    // the superblock with multi-megabyte note content (64 KiB limit).
    if req.message.len() > 65_536 {
        return Err(ApiError::bad_request(
            "note message exceeds maximum size of 65536 bytes",
        ));
    }

    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let response = tokio::task::spawn_blocking(move || -> Result<NoteResponse, ApiError> {
        let oid: ovc_core::id::ObjectId = commit_id
            .parse()
            .map_err(|_| ApiError::bad_request("invalid commit id format"))?;

        ovc_core::notes::set_note(repo.notes_mut(), oid, req.message.clone());
        repo.save().map_err(ApiError::from_core)?;

        Ok(NoteResponse {
            commit_id: oid.to_string(),
            message: req.message,
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(response))
}

/// Handler: `DELETE /api/v1/repos/:id/notes/:commit_id`
///
/// Removes the note from a commit.
pub async fn remove_note(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, commit_id)): Path<(String, String)>,
) -> Result<axum::http::StatusCode, ApiError> {
    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let oid: ovc_core::id::ObjectId = commit_id
            .parse()
            .map_err(|_| ApiError::bad_request("invalid commit id format"))?;

        ovc_core::notes::remove_note(repo.notes_mut(), &oid).map_err(ApiError::from_core)?;
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
