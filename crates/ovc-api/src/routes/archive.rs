//! Archive download endpoint.

use std::io::Write;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;

use crate::auth::Claims;
use crate::error::ApiError;
use crate::models::ArchiveQuery;
use crate::routes::repos::open_repo_blocking;
use crate::routes::resolve_commit_spec;
use crate::state::AppState;

/// Handler: `GET /api/v1/repos/:id/archive`
///
/// Downloads the repository tree at a given commit as a tar archive.
/// The `format` query parameter controls the output format (currently only
/// `"tar"` is supported). The `commit` parameter defaults to HEAD.
pub async fn get_archive(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<ArchiveQuery>,
) -> Result<Response, ApiError> {
    if query.format != "tar" {
        return Err(ApiError::bad_request(
            "unsupported archive format; only 'tar' is supported",
        ));
    }

    let (repo, _) = open_repo_blocking(&app, &id).await?;
    let repo_id = id.clone();

    let archive_data = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, ApiError> {
        let commit_oid = if let Some(ref commit_ref) = query.commit {
            resolve_commit_spec(commit_ref, &repo)?
        } else {
            repo.ref_store()
                .resolve_head()
                .map_err(|_| ApiError::bad_request("no HEAD commit found"))?
        };

        let commit_obj = repo
            .get_object(&commit_oid)
            .map_err(ApiError::from_core)?
            .ok_or_else(|| ApiError::not_found("commit not found"))?;

        let ovc_core::object::Object::Commit(commit) = commit_obj else {
            return Err(ApiError::bad_request("object is not a commit"));
        };

        // Build index from tree to get all file paths and OIDs.
        let mut index = ovc_core::index::Index::new();
        index
            .read_tree(&commit.tree, repo.object_store())
            .map_err(ApiError::from_core)?;

        let mut tar_buf = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_buf);

            for entry in index.entries() {
                let blob = repo
                    .get_object(&entry.oid)
                    .map_err(ApiError::from_core)?
                    .ok_or_else(|| ApiError::not_found("blob not found during archive"))?;

                let ovc_core::object::Object::Blob(data) = blob else {
                    continue;
                };

                let mut tar_header = tar::Header::new_gnu();
                tar_header.set_size(data.len() as u64);
                tar_header.set_mode(if entry.mode == ovc_core::object::FileMode::Executable {
                    0o755
                } else {
                    0o644
                });
                tar_header.set_cksum();

                builder
                    .append_data(&mut tar_header, &entry.path, data.as_slice())
                    .map_err(|e| ApiError::internal(&format!("tar write error: {e}")))?;
            }

            builder
                .finish()
                .map_err(|e| ApiError::internal(&format!("tar finalize error: {e}")))?;
        }

        // Compress with gzip.
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(&tar_buf)
            .map_err(|e| ApiError::internal(&format!("gzip write error: {e}")))?;
        encoder
            .finish()
            .map_err(|e| ApiError::internal(&format!("gzip finish error: {e}")))
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    let filename = format!("{repo_id}.tar.gz");
    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/gzip")
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
                .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
        )
        .body(Body::from(archive_data))
        .map_err(|e| ApiError::internal(&format!("response build error: {e}")))?;

    Ok(response)
}
