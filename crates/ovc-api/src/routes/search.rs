//! Code search (grep) endpoint.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};

use crate::auth::Claims;
use crate::error::ApiError;
use crate::models::{SearchMatch, SearchQuery, SearchResponse};
use crate::routes::repos::open_repo_blocking;
use crate::state::AppState;

/// Handler: `GET /api/v1/repos/:id/search`
///
/// Searches all blobs in the current HEAD tree for lines matching the given
/// regular expression pattern. Returns matching file paths, line numbers,
/// and line contents.
pub async fn search(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, ApiError> {
    if query.q.is_empty() {
        return Err(ApiError::bad_request("search query must not be empty"));
    }
    if query.q.len() > 1024 {
        return Err(ApiError::bad_request(
            "search query must not exceed 1024 characters",
        ));
    }
    if query.max_results == 0 || query.max_results > 10_000 {
        return Err(ApiError::bad_request(
            "max_results must be between 1 and 10000",
        ));
    }

    let (repo, _) = open_repo_blocking(&app, &id).await?;
    let pattern = query.q.clone();
    let case_insensitive = query.case_insensitive;
    let is_regex = query.is_regex;
    let file_pattern = query.file_pattern.clone();
    let max_results = query.max_results;

    let response = tokio::task::spawn_blocking(move || -> Result<SearchResponse, ApiError> {
        let head_oid = repo
            .ref_store()
            .resolve_head()
            .map_err(|_| ApiError::bad_request("no HEAD commit found"))?;

        let commit_obj = repo
            .get_object(&head_oid)
            .map_err(ApiError::from_core)?
            .ok_or_else(|| ApiError::not_found("HEAD commit not found"))?;

        let ovc_core::object::Object::Commit(commit) = commit_obj else {
            return Err(ApiError::internal("HEAD is not a commit object"));
        };

        let matches = ovc_core::grep::grep_tree_filtered(
            &pattern,
            &commit.tree,
            repo.object_store(),
            case_insensitive,
            is_regex,
            file_pattern.as_deref(),
        )
        .map_err(ApiError::from_core)?;

        let all_results: Vec<SearchMatch> = matches
            .into_iter()
            .map(|m| SearchMatch {
                path: m.path,
                line_number: m.line_number,
                line: m.line,
            })
            .collect();

        let truncated = all_results.len() > max_results;
        let results = if truncated {
            all_results.into_iter().take(max_results).collect()
        } else {
            all_results
        };
        let total_matches = results.len();

        Ok(SearchResponse {
            query: pattern,
            total_matches,
            results,
            truncated,
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(response))
}
