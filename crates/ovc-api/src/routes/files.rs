//! File tree, blob content, and status endpoints.

use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Multipart, Path, Query, State};
use base64::Engine as _;
use serde::Deserialize;

use crate::auth::Claims;
use crate::error::ApiError;
use crate::models::{
    BlameLineResponse, BlameResponse, CleanRequest, CleanResponse, DeleteBlobRequest,
    DeleteBlobResponse, FileContent, FileStatusEntry, FileTreeEntry, MkdirRequest, MkdirResponse,
    MoveFileRequest, MoveFileResponse, PutBlobRequest, PutBlobResponse, RestoreRequest,
    StageRequest, StatusResponse, UnstageRequest, UploadResponse, UploadedFile,
};
use crate::routes::repos::open_repo_blocking;
use crate::routes::resolve_commit_spec;
use crate::state::AppState;

/// Query parameters for the blame endpoint.
#[derive(Debug, Deserialize)]
pub struct BlameQuery {
    /// Optional commit ref to blame at (HEAD, HEAD~N, branch name, tag, or full OID).
    /// When absent, blames HEAD (current behavior).
    #[serde(default, rename = "ref")]
    pub commit_ref: Option<String>,
}

/// Marker files that indicate the parent directory of a `.ovc` file is a
/// project working directory (i.e. source files live alongside the `.ovc` file).
const PROJECT_MARKER_FILES: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "go.mod",
    "pyproject.toml",
    "setup.py",
    "Makefile",
    "CMakeLists.txt",
    "build.gradle",
    "pom.xml",
    ".gitignore",
    "README.md",
    "README",
    "src",
    "lib",
];

/// Attempts to find a working directory for the given repository.
///
/// Strategies (in order):
/// 1. Check `AppState::workdir_for()` (populated from `OVC_WORKDIR_MAP`,
///    `OVC_WORKDIR_SCAN`, or `--workdir` flags at startup).
/// 2. Check if the parent directory of the `.ovc` file contains recognizable
///    project files (Cargo.toml, package.json, etc.), indicating it is the workdir.
/// 3. Return `None` if no working directory is accessible.
pub(crate) fn find_workdir_for_repo(app: &AppState, repo_id: &str) -> Option<PathBuf> {
    // Strategy 1: Pre-configured workdir mapping.
    if let Some(workdir) = app.workdir_for(repo_id)
        && workdir.is_dir()
    {
        return Some(workdir);
    }

    // Strategy 2: Parent of the .ovc file has project marker files.
    let ovc_path = app.repos_dir.join(format!("{repo_id}.ovc"));
    if let Some(parent) = ovc_path.parent() {
        for marker in PROJECT_MARKER_FILES {
            if parent.join(marker).exists() {
                return Some(parent.to_path_buf());
            }
        }
    }

    None
}

/// Query parameters for the tree endpoint.
#[derive(Debug, Deserialize)]
pub struct TreeQuery {
    /// Path within the repository tree (default: root).
    #[serde(default)]
    pub path: Option<String>,
    /// Optional commit ref to browse (HEAD, HEAD~N, branch name, tag, or full OID).
    /// When absent the current index is used.
    #[serde(default, rename = "ref")]
    pub commit_ref: Option<String>,
}

/// Query parameters for the blob endpoint.
#[derive(Debug, Deserialize)]
pub struct BlobQuery {
    /// Path of the file to retrieve.
    pub path: String,
    /// Optional commit ref to browse (HEAD, HEAD~N, branch name, tag, or full OID).
    /// When absent the current index is used.
    #[serde(default, rename = "ref")]
    pub commit_ref: Option<String>,
}

/// Scans the working directory and populates unstaged/untracked lists.
fn collect_workdir_changes(
    repo: &ovc_core::repository::Repository,
    workdir_path: &std::path::Path,
    head_tree_oid: Option<&ovc_core::id::ObjectId>,
    unstaged: &mut Vec<FileStatusEntry>,
    untracked: &mut Vec<String>,
) -> Result<(), ApiError> {
    let workdir = ovc_core::workdir::WorkDir::new(workdir_path.to_path_buf());
    let ignore = ovc_core::ignore::IgnoreRules::load(workdir_path);
    let status_entries = workdir
        .compute_status(repo.index(), head_tree_oid, repo.object_store(), &ignore)
        .map_err(ApiError::from_core)?;

    for entry in &status_entries {
        match entry.unstaged {
            ovc_core::workdir::FileStatus::Modified => {
                unstaged.push(FileStatusEntry {
                    path: entry.path.clone(),
                    status: "modified".to_owned(),
                });
            }
            ovc_core::workdir::FileStatus::Deleted => {
                unstaged.push(FileStatusEntry {
                    path: entry.path.clone(),
                    status: "deleted".to_owned(),
                });
            }
            _ => {}
        }
        if entry.staged == ovc_core::workdir::FileStatus::Untracked {
            untracked.push(entry.path.clone());
        }
    }
    Ok(())
}

/// Handler: `GET /api/v1/repos/:id/status`
pub async fn get_status(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<StatusResponse>, ApiError> {
    let workdir_path = find_workdir_for_repo(&app, &id);
    let has_workdir = workdir_path.is_some();
    let (repo, _ovc_path) = open_repo_blocking(&app, &id).await?;

    let response = tokio::task::spawn_blocking(move || -> Result<StatusResponse, ApiError> {
        // Determine current branch name.
        let branch = match repo.ref_store().head() {
            ovc_core::refs::RefTarget::Symbolic(s) => {
                s.strip_prefix("refs/heads/").unwrap_or(s).to_owned()
            }
            ovc_core::refs::RefTarget::Direct(oid) => {
                let hex = oid.to_string();
                hex[..12.min(hex.len())].to_owned()
            }
        };

        // Compute HEAD tree for comparison.
        let head_tree_oid = repo.ref_store().resolve_head().ok().and_then(|commit_oid| {
            repo.get_object(&commit_oid).ok().flatten().and_then(|obj| {
                if let ovc_core::object::Object::Commit(c) = obj {
                    Some(c.tree)
                } else {
                    None
                }
            })
        });

        // Compare index vs HEAD for staged changes.
        let mut staged = Vec::new();
        let mut unstaged = Vec::new();
        let mut untracked = Vec::new();

        if let Some(tree_oid) = head_tree_oid {
            // Build head index to compare against current index.
            let mut head_index = ovc_core::index::Index::new();
            if head_index.read_tree(&tree_oid, repo.object_store()).is_ok() {
                // Find staged changes: entries in current index that differ from HEAD.
                for entry in repo.index().entries() {
                    let file_status =
                        head_index
                            .get_entry(&entry.path)
                            .map_or("added", |head_entry| {
                                if head_entry.oid == entry.oid {
                                    "unmodified"
                                } else {
                                    "modified"
                                }
                            });
                    if file_status != "unmodified" {
                        staged.push(FileStatusEntry {
                            path: entry.path.clone(),
                            status: file_status.to_owned(),
                        });
                    }
                }

                // Find staged deletions: entries in HEAD but not in current index.
                for head_entry in head_index.entries() {
                    if repo.index().get_entry(&head_entry.path).is_none() {
                        staged.push(FileStatusEntry {
                            path: head_entry.path.clone(),
                            status: "deleted".to_owned(),
                        });
                    }
                }
            }
        } else {
            // No HEAD commit: all entries are "added".
            for entry in repo.index().entries() {
                staged.push(FileStatusEntry {
                    path: entry.path.clone(),
                    status: "added".to_owned(),
                });
            }
        }

        // Scan working directory for unstaged and untracked files.
        if let Some(ref wd_path) = workdir_path {
            collect_workdir_changes(
                &repo,
                wd_path,
                head_tree_oid.as_ref(),
                &mut unstaged,
                &mut untracked,
            )?;
        }

        Ok(StatusResponse {
            branch,
            staged,
            unstaged,
            untracked,
            has_workdir,
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(response))
}

/// Builds a `Vec<FileTreeEntry>` from a slice of index entries at the given prefix.
fn build_tree_listing(
    entries: &[ovc_core::index::IndexEntry],
    prefix_normalized: &str,
) -> Vec<FileTreeEntry> {
    let mut result = Vec::new();
    let mut dirs_seen = std::collections::BTreeSet::new();

    for entry in entries {
        let rel = if prefix_normalized.is_empty() {
            entry.path.as_str()
        } else if let Some(rest) = entry.path.strip_prefix(prefix_normalized) {
            rest.strip_prefix('/').unwrap_or(rest)
        } else {
            continue;
        };

        if rel.is_empty() {
            continue;
        }

        if let Some(slash_pos) = rel.find('/') {
            // This is inside a subdirectory at the current level.
            let dir_name = &rel[..slash_pos];
            let dir_path = if prefix_normalized.is_empty() {
                dir_name.to_owned()
            } else {
                format!("{prefix_normalized}/{dir_name}")
            };

            if dirs_seen.insert(dir_path.clone()) {
                result.push(FileTreeEntry {
                    name: dir_name.to_owned(),
                    path: dir_path,
                    entry_type: "directory".to_owned(),
                    size: 0,
                });
            }
        } else {
            // Direct child file.
            result.push(FileTreeEntry {
                name: rel.to_owned(),
                path: entry.path.clone(),
                entry_type: "file".to_owned(),
                size: entry.file_size,
            });
        }
    }

    result.sort_by(|a, b| {
        // Directories first, then files, alphabetically.
        let type_order_a = i32::from(a.entry_type != "directory");
        let type_order_b = i32::from(b.entry_type != "directory");
        type_order_a
            .cmp(&type_order_b)
            .then_with(|| a.name.cmp(&b.name))
    });

    result
}

/// Resolves a commit ref to the tree OID of that commit.
fn resolve_commit_tree(
    commit_ref: &str,
    repo: &ovc_core::repository::Repository,
) -> Result<ovc_core::id::ObjectId, ApiError> {
    let commit_oid = super::resolve_commit_spec(commit_ref, repo)?;
    let obj = repo
        .get_object(&commit_oid)
        .map_err(ApiError::from_core)?
        .ok_or_else(|| ApiError::not_found(&format!("commit object not found: {commit_oid}")))?;
    match obj {
        ovc_core::object::Object::Commit(c) => Ok(c.tree),
        _ => Err(ApiError::bad_request(&format!(
            "object is not a commit: {commit_oid}"
        ))),
    }
}

/// Merges working-directory filesystem entries into the index-based tree listing
/// so that new (untracked) files and directories appear in the file tree.
fn merge_workdir_entries(
    index_entries: &mut Vec<FileTreeEntry>,
    workdir: &std::path::Path,
    prefix_normalized: &str,
) {
    let dir_to_scan = if prefix_normalized.is_empty() {
        workdir.to_path_buf()
    } else {
        workdir.join(prefix_normalized)
    };

    let Ok(read_dir) = std::fs::read_dir(&dir_to_scan) else {
        return; // directory doesn't exist on disk — nothing to merge
    };

    // Collect names already present from the index.
    let existing: std::collections::HashSet<String> =
        index_entries.iter().map(|e| e.name.clone()).collect();

    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files/dirs and the .ovc internal directory.
        if name.starts_with('.') {
            continue;
        }

        // Already in the index — skip.
        if existing.contains(&name) {
            continue;
        }

        let Ok(ft) = entry.file_type() else {
            continue;
        };

        let path = if prefix_normalized.is_empty() {
            name.clone()
        } else {
            format!("{prefix_normalized}/{name}")
        };

        if ft.is_dir() {
            index_entries.push(FileTreeEntry {
                name,
                path,
                entry_type: "directory".to_owned(),
                size: 0,
            });
        } else if ft.is_file() {
            let size = entry.metadata().map_or(0, |m| m.len());
            index_entries.push(FileTreeEntry {
                name,
                path,
                entry_type: "file".to_owned(),
                size,
            });
        }
    }

    // Re-sort: directories first, then files, alphabetically.
    index_entries.sort_by(|a, b| {
        let type_order_a = i32::from(a.entry_type != "directory");
        let type_order_b = i32::from(b.entry_type != "directory");
        type_order_a
            .cmp(&type_order_b)
            .then_with(|| a.name.cmp(&b.name))
    });
}

/// Handler: `GET /api/v1/repos/:id/tree`
pub async fn get_tree(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<TreeQuery>,
) -> Result<Json<Vec<FileTreeEntry>>, ApiError> {
    let workdir_path = find_workdir_for_repo(&app, &id);
    let (repo, _ovc_path) = open_repo_blocking(&app, &id).await?;
    let prefix = query.path.unwrap_or_default();
    let commit_ref = query.commit_ref;

    let entries = tokio::task::spawn_blocking(move || -> Result<Vec<FileTreeEntry>, ApiError> {
        let prefix_normalized = prefix.trim_matches('/');

        if let Some(ref cref) = commit_ref {
            // Browse the tree at a specific commit — no workdir merge.
            let tree_oid = resolve_commit_tree(cref, &repo)?;
            let mut index = ovc_core::index::Index::new();
            index
                .read_tree(&tree_oid, repo.object_store())
                .map_err(ApiError::from_core)?;
            Ok(build_tree_listing(index.entries(), prefix_normalized))
        } else {
            // Default: browse the current index + working directory.
            let mut entries = build_tree_listing(repo.index().entries(), prefix_normalized);
            // Merge in untracked files from the working directory.
            if let Some(ref workdir) = workdir_path {
                merge_workdir_entries(&mut entries, workdir, prefix_normalized);
            }
            Ok(entries)
        }
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(entries))
}

/// Handler: `GET /api/v1/repos/:id/blob`
pub async fn get_blob(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<BlobQuery>,
) -> Result<Json<FileContent>, ApiError> {
    let file_path = &query.path;
    if file_path.is_empty()
        || file_path.starts_with('/')
        || file_path.starts_with('\\')
        || file_path.contains("..")
    {
        return Err(ApiError::bad_request("invalid file path"));
    }

    let (repo, _ovc_path) = open_repo_blocking(&app, &id).await?;
    let file_path = query.path;
    let commit_ref = query.commit_ref;

    let content = tokio::task::spawn_blocking(move || -> Result<FileContent, ApiError> {
        let blob_oid = if let Some(ref cref) = commit_ref {
            // Look up the blob in the specified commit's tree.
            let tree_oid = resolve_commit_tree(cref, &repo)?;
            let mut index = ovc_core::index::Index::new();
            index
                .read_tree(&tree_oid, repo.object_store())
                .map_err(ApiError::from_core)?;
            let entry = index.get_entry(&file_path).ok_or_else(|| {
                ApiError::not_found(&format!("file '{file_path}' not found at ref '{cref}'"))
            })?;
            entry.oid
        } else {
            // Default: look up in the current index.
            let entry = repo.index().get_entry(&file_path).ok_or_else(|| {
                ApiError::not_found(&format!("file '{file_path}' not found in index"))
            })?;
            entry.oid
        };

        let blob = repo
            .get_object(&blob_oid)
            .map_err(ApiError::from_core)?
            .ok_or_else(|| ApiError::not_found("blob object not found"))?;

        let ovc_core::object::Object::Blob(data) = blob else {
            return Err(ApiError::internal("expected blob object"));
        };

        let is_binary = ovc_core::diff::is_binary(&data);
        let size_bytes = data.len() as u64;
        let content_str = if is_binary {
            String::new()
        } else {
            String::from_utf8_lossy(&data).into_owned()
        };

        Ok(FileContent {
            path: file_path,
            content: content_str,
            is_binary,
            size_bytes,
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(content))
}

/// Handler: `POST /api/v1/repos/:id/stage`
///
/// Stages files by reading their content from the working directory and adding
/// them to the repository index. Returns `501 Not Implemented` if no working
/// directory is accessible for this repository.
pub async fn stage_files(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<StageRequest>,
) -> Result<axum::http::StatusCode, ApiError> {
    if req.paths.is_empty() {
        return Err(ApiError::bad_request("paths must not be empty"));
    }

    let workdir = find_workdir_for_repo(&app, &id).ok_or_else(|| {
        ApiError::not_implemented(
            "no working directory found for this repository — staging files requires \
             filesystem access (set OVC_WORKDIR_MAP or ensure the .ovc file is alongside project files)",
        )
    })?;

    // Validate all paths before acquiring the repo lock.
    for path in &req.paths {
        if path.is_empty() || path.starts_with('/') || path.starts_with('\\') || path.contains("..")
        {
            return Err(ApiError::bad_request(&format!(
                "invalid file path: '{path}'"
            )));
        }
    }

    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _ovc_path) = open_repo_blocking(&app, &id).await?;
    let paths = req.paths;

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        for path in &paths {
            let file_path = workdir.join(path);

            // If the file doesn't exist on disk but is in the index,
            // stage the deletion by removing it from the index.
            if !file_path.exists() {
                repo.index_mut().unstage_file(path);
                continue;
            }

            let content = std::fs::read(&file_path).map_err(|e| {
                tracing::error!("failed to read file '{path}': {e}");
                ApiError::internal("failed to read file")
            })?;

            let mode = {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let metadata = std::fs::metadata(&file_path).map_err(|e| {
                        tracing::error!("failed to stat '{path}': {e}");
                        ApiError::internal("failed to stat file")
                    })?;
                    if metadata.permissions().mode() & 0o111 != 0 {
                        ovc_core::object::FileMode::Executable
                    } else {
                        ovc_core::object::FileMode::Regular
                    }
                }
                #[cfg(not(unix))]
                {
                    ovc_core::object::FileMode::Regular
                }
            };

            let (index, store) = repo.index_and_store_mut();
            index
                .stage_file(path, &content, mode, store)
                .map_err(ApiError::from_core)?;
        }
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

/// Handler: `POST /api/v1/repos/:id/unstage`
pub async fn unstage_files(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UnstageRequest>,
) -> Result<axum::http::StatusCode, ApiError> {
    if req.paths.is_empty() {
        return Err(ApiError::bad_request("paths must not be empty"));
    }

    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _ovc_path) = open_repo_blocking(&app, &id).await?;
    let paths = req.paths;

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        for path in &paths {
            repo.index_mut().unstage_file(path);
        }
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

/// Handler: `POST /api/v1/repos/:id/restore`
///
/// Restores staged entries to their HEAD versions. For files that exist in the
/// HEAD tree, the index entry reverts to match HEAD. For files that were newly
/// added (not in HEAD), the entry is removed from the index entirely.
pub async fn restore_files(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<RestoreRequest>,
) -> Result<axum::http::StatusCode, ApiError> {
    if req.paths.is_empty() {
        return Err(ApiError::bad_request("paths must not be empty"));
    }

    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _ovc_path) = open_repo_blocking(&app, &id).await?;
    let paths = req.paths;

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        // Resolve HEAD tree OID for restore operations.
        let head_tree_oid = repo.ref_store().resolve_head().ok().and_then(|commit_oid| {
            repo.get_object(&commit_oid).ok().flatten().and_then(|obj| {
                if let ovc_core::object::Object::Commit(c) = obj {
                    Some(c.tree)
                } else {
                    None
                }
            })
        });

        for path in &paths {
            if let Some(ref tree_oid) = head_tree_oid {
                let (index, store) = repo.index_and_store_mut();
                index
                    .restore_to_head(path, tree_oid, store)
                    .map_err(ApiError::from_core)?;
            } else {
                // No HEAD commit: removing the entry is the correct behavior
                // (there is nothing to restore to).
                repo.index_mut().unstage_file(path);
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

    Ok(axum::http::StatusCode::OK)
}

/// Handler: `GET /api/v1/repos/:id/blame/{*path}`
///
/// Returns line-by-line blame for a file, attributing each line to the commit
/// that last modified it.
///
/// An optional `?ref=` query parameter accepts any commit specifier (branch
/// name, tag, full OID, `HEAD`, `HEAD~N`) to blame at a historical commit
/// instead of the current HEAD.
pub async fn get_blame(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, file_path)): Path<(String, String)>,
    Query(query): Query<BlameQuery>,
) -> Result<Json<BlameResponse>, ApiError> {
    if file_path.is_empty() || file_path.contains("..") {
        return Err(ApiError::bad_request("invalid file path"));
    }

    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let response = tokio::task::spawn_blocking(move || -> Result<BlameResponse, ApiError> {
        let target_oid = if let Some(ref cref) = query.commit_ref {
            resolve_commit_spec(cref, &repo)?
        } else {
            repo.ref_store()
                .resolve_head()
                .map_err(|_| ApiError::bad_request("no HEAD commit found"))?
        };

        let blame_lines = ovc_core::blame::blame(&file_path, target_oid, repo.object_store())
            .map_err(ApiError::from_core)?;

        let lines = blame_lines
            .into_iter()
            .map(|bl| BlameLineResponse {
                commit_id: bl.commit_id.to_string(),
                author: bl.author,
                timestamp: bl.timestamp,
                line_number: bl.line_number,
                content: bl.content,
            })
            .collect();

        Ok(BlameResponse {
            file: file_path,
            lines,
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(response))
}

/// Handler: `POST /api/v1/repos/:id/clean`
///
/// Removes untracked files from the working directory. Optionally accepts a
/// list of paths to restrict cleaning to, and a `dry_run` flag to preview
/// what would be deleted without performing any deletions.
///
/// Returns `501 Not Implemented` if no working directory is accessible.
pub async fn clean_files(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<CleanRequest>,
) -> Result<Json<CleanResponse>, ApiError> {
    // Validate requested paths before any I/O.
    if let Some(ref paths) = req.paths {
        for path in paths {
            if path.is_empty()
                || path.starts_with('/')
                || path.starts_with('\\')
                || path.contains("..")
            {
                return Err(ApiError::bad_request(&format!(
                    "invalid file path: '{path}'"
                )));
            }
        }
    }

    let workdir_path = find_workdir_for_repo(&app, &id).ok_or_else(|| {
        ApiError::not_implemented(
            "no working directory found for this repository — cleaning files requires \
             filesystem access (set OVC_WORKDIR_MAP or ensure the .ovc file is alongside project files)",
        )
    })?;

    let dry_run = req.dry_run.unwrap_or(false);
    let filter_paths = req.paths;

    let (repo, _ovc_path) = open_repo_blocking(&app, &id).await?;

    let response = tokio::task::spawn_blocking(move || -> Result<CleanResponse, ApiError> {
        let workdir = ovc_core::workdir::WorkDir::new(workdir_path.clone());
        let ignore = ovc_core::ignore::IgnoreRules::load(&workdir_path);

        let head_tree_oid = repo.ref_store().resolve_head().ok().and_then(|commit_oid| {
            repo.get_object(&commit_oid).ok().flatten().and_then(|obj| {
                if let ovc_core::object::Object::Commit(c) = obj {
                    Some(c.tree)
                } else {
                    None
                }
            })
        });

        let status_entries = workdir
            .compute_status(
                repo.index(),
                head_tree_oid.as_ref(),
                repo.object_store(),
                &ignore,
            )
            .map_err(ApiError::from_core)?;

        let mut untracked: Vec<String> = status_entries
            .iter()
            .filter(|s| s.staged == ovc_core::workdir::FileStatus::Untracked)
            .map(|s| s.path.clone())
            .collect();

        // Filter to requested paths if provided.
        if let Some(ref filter) = filter_paths {
            let filter_set: std::collections::HashSet<&str> =
                filter.iter().map(String::as_str).collect();
            untracked.retain(|p| filter_set.contains(p.as_str()));
        }

        if !dry_run {
            for path in &untracked {
                workdir.delete_file(path).map_err(ApiError::from_core)?;
            }
        }

        Ok(CleanResponse { deleted: untracked })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(response))
}

// ── File CRUD helpers ───────────────────────────────────────────────────

/// Maximum file size for individual uploads (16 MiB).
///
/// This must not exceed the global `MAX_REQUEST_BODY_BYTES` limit set in
/// `routes/mod.rs` via `DefaultBodyLimit`. If you raise this constant,
/// you must also raise the global body limit (or apply a per-route
/// `DefaultBodyLimit::max` override on the upload/blob routes), otherwise
/// axum will reject the request before the per-file check is ever reached.
const MAX_UPLOAD_FILE_BYTES: usize = 16 * 1024 * 1024;
const MAX_UPLOAD_FILE_COUNT: usize = 100;

/// Validates a user-supplied relative path for safety.
///
/// Rejects empty paths, absolute paths, path traversal (`..`), null bytes,
/// and backslash separators.
fn validate_file_path(path: &str) -> Result<(), ApiError> {
    if path.is_empty() {
        return Err(ApiError::bad_request("file path must not be empty"));
    }
    if path.len() > 1024 {
        return Err(ApiError::bad_request(
            "file path too long (max 1024 characters)",
        ));
    }
    if path.split('/').count() > 64 {
        return Err(ApiError::bad_request(
            "path too deeply nested (max 64 levels)",
        ));
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(ApiError::bad_request("absolute paths are not allowed"));
    }
    if path.contains("..") {
        return Err(ApiError::bad_request(
            "path traversal ('..') is not allowed",
        ));
    }
    if path.contains('\0') {
        return Err(ApiError::bad_request("null bytes are not allowed in paths"));
    }
    if path.contains('\\') {
        return Err(ApiError::bad_request(
            "backslash separators are not allowed — use forward slashes",
        ));
    }
    // Prevent writes to the `.ovc/` internal metadata directory.
    if path == ".ovc" || path.starts_with(".ovc/") {
        return Err(ApiError::bad_request(
            "cannot write to .ovc/ internal directory",
        ));
    }
    Ok(())
}

/// Resolves a workdir for the given repo, returning an appropriate error if
/// no working directory is accessible.
fn require_workdir(app: &AppState, repo_id: &str) -> Result<PathBuf, ApiError> {
    find_workdir_for_repo(app, repo_id).ok_or_else(|| {
        ApiError::not_implemented(
            "no working directory found for this repository — file operations require \
             filesystem access (set OVC_WORKDIR_MAP or ensure the .ovc file is alongside project files)",
        )
    })
}

// ── File CRUD endpoints ─────────────────────────────────────────────────

/// Handler: `PUT /api/v1/repos/:id/blob`
///
/// Creates or updates a file in the working directory. Accepts UTF-8 or
/// base64-encoded content for binary files.
pub async fn put_blob(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<PutBlobRequest>,
) -> Result<Json<PutBlobResponse>, ApiError> {
    validate_file_path(&req.path)?;

    let workdir = require_workdir(&app, &id)?;

    let content_bytes = match req.encoding.as_str() {
        "utf8" | "utf-8" => req.content.into_bytes(),
        "base64" => base64::engine::general_purpose::STANDARD
            .decode(&req.content)
            .map_err(|e| ApiError::bad_request(&format!("invalid base64 content: {e}")))?,
        other => {
            return Err(ApiError::bad_request(&format!(
                "unsupported encoding '{other}' — use 'utf8' or 'base64'"
            )));
        }
    };

    if content_bytes.len() > MAX_UPLOAD_FILE_BYTES {
        return Err(ApiError::bad_request(&format!(
            "file content exceeds maximum size of {MAX_UPLOAD_FILE_BYTES} bytes"
        )));
    }

    let file_path = req.path.clone();
    let response_path = req.path;

    let size = tokio::task::spawn_blocking(move || -> Result<u64, ApiError> {
        let target = workdir.join(&file_path);

        // Canonicalize the workdir first so we have a stable baseline.
        let workdir_canonical = workdir.canonicalize().map_err(|e| {
            tracing::error!("failed to resolve workdir: {e}");
            ApiError::internal("failed to resolve working directory")
        })?;

        // Resolve the parent directory BEFORE writing any data. This closes
        // the TOCTOU window: if a symlink is present, we detect the escape
        // before any bytes land on disk.
        let parent = target
            .parent()
            .ok_or_else(|| ApiError::bad_request("invalid path: no parent directory"))?;
        std::fs::create_dir_all(parent).map_err(|e| {
            tracing::error!("failed to create parent dirs: {e}");
            ApiError::internal("failed to create parent directories")
        })?;
        let canonical_parent = parent.canonicalize().map_err(|e| {
            tracing::error!("failed to resolve parent dir: {e}");
            ApiError::internal("failed to resolve parent directory")
        })?;
        if !canonical_parent.starts_with(&workdir_canonical) {
            return Err(ApiError::bad_request(
                "resolved path escapes the working directory",
            ));
        }

        // Build the final path from the verified canonical parent so the
        // write target cannot differ from what we just checked.
        let file_name = target
            .file_name()
            .ok_or_else(|| ApiError::bad_request("invalid path: missing file name"))?;
        let safe_target = canonical_parent.join(file_name);

        std::fs::write(&safe_target, &content_bytes).map_err(|e| {
            tracing::error!("failed to write file: {e}");
            ApiError::internal("failed to write file")
        })?;

        Ok(content_bytes.len() as u64)
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(PutBlobResponse {
        path: response_path,
        size_bytes: size,
    }))
}

/// Handler: `DELETE /api/v1/repos/:id/blob`
///
/// Deletes a file from the working directory.
pub async fn delete_blob(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<DeleteBlobRequest>,
) -> Result<Json<DeleteBlobResponse>, ApiError> {
    validate_file_path(&req.path)?;

    let workdir = require_workdir(&app, &id)?;
    let file_path = req.path.clone();
    let response_path = req.path;

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let target = workdir.join(&file_path);

        // Verify the target resolves inside the workdir.
        let workdir_canonical = workdir.canonicalize().map_err(|e| {
            tracing::error!("failed to resolve workdir: {e}");
            ApiError::internal("failed to resolve working directory")
        })?;

        if !target.exists() {
            return Err(ApiError::not_found(&format!(
                "file '{file_path}' not found in working directory"
            )));
        }

        let target_canonical = target.canonicalize().map_err(|e| {
            tracing::error!("failed to resolve path: {e}");
            ApiError::internal("failed to resolve path")
        })?;
        if !target_canonical.starts_with(&workdir_canonical) {
            return Err(ApiError::bad_request(
                "resolved path escapes the working directory",
            ));
        }

        // Only delete regular files, not directories.
        if target_canonical.is_dir() {
            return Err(ApiError::bad_request(
                "path is a directory — use a different mechanism to remove directories",
            ));
        }

        std::fs::remove_file(&target_canonical).map_err(|e| {
            tracing::error!("failed to delete file: {e}");
            ApiError::internal("failed to delete file")
        })?;

        Ok(())
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(DeleteBlobResponse {
        path: response_path,
    }))
}

/// Handler: `POST /api/v1/repos/:id/upload`
///
/// Accepts multipart form data with one or more files and a `path` field
/// specifying the target directory within the working directory.
async fn parse_multipart_upload(
    multipart: &mut Multipart,
) -> Result<(String, Vec<(String, Vec<u8>)>), ApiError> {
    let mut target_dir = String::new();
    let mut file_buffers: Vec<(String, Vec<u8>)> = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad_request(&format!("multipart parse error: {e}")))?
    {
        let field_name = field.name().unwrap_or("").to_owned();

        if field_name == "path" {
            target_dir = field
                .text()
                .await
                .map_err(|e| ApiError::bad_request(&format!("failed to read path field: {e}")))?;
            continue;
        }

        let file_name = field.file_name().unwrap_or("unnamed").to_owned();

        if file_name.is_empty()
            || file_name.contains('/')
            || file_name.contains('\\')
            || file_name.contains("..")
            || file_name.contains('\0')
        {
            return Err(ApiError::bad_request(&format!(
                "invalid upload filename: '{file_name}'"
            )));
        }

        let data = field
            .bytes()
            .await
            .map_err(|e| ApiError::bad_request(&format!("failed to read upload data: {e}")))?;

        if data.len() > MAX_UPLOAD_FILE_BYTES {
            return Err(ApiError::bad_request(&format!(
                "uploaded file '{file_name}' exceeds maximum size of {MAX_UPLOAD_FILE_BYTES} bytes"
            )));
        }

        file_buffers.push((file_name, data.to_vec()));

        if file_buffers.len() > MAX_UPLOAD_FILE_COUNT {
            return Err(ApiError::bad_request(&format!(
                "upload exceeds maximum of {MAX_UPLOAD_FILE_COUNT} files per request"
            )));
        }
    }

    if !target_dir.is_empty() {
        validate_file_path(&target_dir)?;
    }

    if file_buffers.is_empty() {
        return Err(ApiError::bad_request(
            "no files were included in the upload",
        ));
    }

    Ok((target_dir, file_buffers))
}

pub async fn upload_files(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, ApiError> {
    let workdir = require_workdir(&app, &id)?;
    let (target_dir, file_buffers) = parse_multipart_upload(&mut multipart).await?;

    let files = tokio::task::spawn_blocking(move || -> Result<Vec<UploadedFile>, ApiError> {
        let workdir_canonical = workdir.canonicalize().map_err(|e| {
            tracing::error!("failed to resolve workdir: {e}");
            ApiError::internal("failed to resolve working directory")
        })?;

        let base_dir = if target_dir.is_empty() {
            workdir.clone()
        } else {
            workdir.join(&target_dir)
        };

        std::fs::create_dir_all(&base_dir).map_err(|e| {
            tracing::error!("failed to create target directory: {e}");
            ApiError::internal("failed to create target directory")
        })?;

        let mut results = Vec::with_capacity(file_buffers.len());

        for (name, data) in &file_buffers {
            let target = base_dir.join(name);

            std::fs::write(&target, data).map_err(|e| {
                tracing::error!("failed to write '{name}': {e}");
                ApiError::internal("failed to write uploaded file")
            })?;

            // Verify the written file resolves inside the workdir.
            let target_canonical = target.canonicalize().map_err(|e| {
                tracing::error!("failed to resolve written path: {e}");
                ApiError::internal("failed to resolve written path")
            })?;
            if !target_canonical.starts_with(&workdir_canonical) {
                let _ = std::fs::remove_file(&target_canonical);
                return Err(ApiError::bad_request(
                    "resolved path escapes the working directory",
                ));
            }

            let rel_path = if target_dir.is_empty() {
                name.clone()
            } else {
                format!("{target_dir}/{name}")
            };

            results.push(UploadedFile {
                path: rel_path,
                size_bytes: data.len() as u64,
            });
        }

        Ok(results)
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(UploadResponse { files }))
}

/// Handler: `POST /api/v1/repos/:id/mkdir`
///
/// Creates a directory (and any missing parent directories) in the working
/// directory.
pub async fn mkdir(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<MkdirRequest>,
) -> Result<Json<MkdirResponse>, ApiError> {
    validate_file_path(&req.path)?;

    let workdir = require_workdir(&app, &id)?;
    let dir_path = req.path.clone();
    let response_path = req.path;

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let workdir_canonical = workdir.canonicalize().map_err(|e| {
            tracing::error!("failed to resolve workdir: {e}");
            ApiError::internal("failed to resolve working directory")
        })?;

        let target = workdir.join(&dir_path);

        std::fs::create_dir_all(&target).map_err(|e| {
            tracing::error!("failed to create directory: {e}");
            ApiError::internal("failed to create directory")
        })?;

        // Verify the created directory resolves inside the workdir.
        let target_canonical = target.canonicalize().map_err(|e| {
            tracing::error!("failed to resolve created path: {e}");
            ApiError::internal("failed to resolve created path")
        })?;
        if !target_canonical.starts_with(&workdir_canonical) {
            let _ = std::fs::remove_dir_all(&target_canonical);
            return Err(ApiError::bad_request(
                "resolved path escapes the working directory",
            ));
        }

        Ok(())
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(MkdirResponse {
        path: response_path,
    }))
}

/// Handler: `POST /api/v1/repos/:id/move`
///
/// Moves or renames a file within the working directory. Creates target parent
/// directories if they do not exist. Uses `std::fs::rename` for same-filesystem
/// moves with a fallback to copy-then-delete for cross-filesystem scenarios.
pub async fn move_file(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<MoveFileRequest>,
) -> Result<Json<MoveFileResponse>, ApiError> {
    validate_file_path(&req.from_path)?;
    validate_file_path(&req.to_path)?;

    if req.from_path == req.to_path {
        return Err(ApiError::bad_request(
            "source and destination paths must differ",
        ));
    }

    let workdir = require_workdir(&app, &id)?;
    let from_path = req.from_path.clone();
    let to_path = req.to_path.clone();

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let workdir_canonical = workdir.canonicalize().map_err(|e| {
            tracing::error!("failed to resolve workdir: {e}");
            ApiError::internal("failed to resolve working directory")
        })?;

        let source = workdir.join(&from_path);
        if !source.exists() {
            return Err(ApiError::not_found("source file not found"));
        }

        // Verify source resolves inside the workdir.
        let source_canonical = source.canonicalize().map_err(|e| {
            tracing::error!("failed to resolve source path: {e}");
            ApiError::internal("failed to resolve source path")
        })?;
        if !source_canonical.starts_with(&workdir_canonical) {
            return Err(ApiError::bad_request(
                "source path escapes the working directory",
            ));
        }

        // Verify source is a file, not a directory.
        if source_canonical.is_dir() {
            return Err(ApiError::bad_request(
                "source path is a directory — only files can be moved",
            ));
        }

        let target = workdir.join(&to_path);

        // Create parent directories for the target if needed.
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                tracing::error!("failed to create parent directories for target: {e}");
                ApiError::internal("failed to create target parent directories")
            })?;
        }

        // Attempt rename (fast path for same filesystem).
        if let Err(rename_err) = std::fs::rename(&source_canonical, &target) {
            // Fallback: copy + delete for cross-filesystem moves.
            // ErrorKind::CrossesDevices is unstable, so we always try the
            // fallback when rename fails with any OS error.
            tracing::debug!("rename failed ({rename_err}), falling back to copy+delete");
            std::fs::copy(&source_canonical, &target).map_err(|e| {
                tracing::error!("failed to copy file during move: {e}");
                ApiError::internal("failed to move file")
            })?;
            std::fs::remove_file(&source_canonical).map_err(|e| {
                tracing::error!("failed to remove source after copy: {e}");
                ApiError::internal("failed to complete file move")
            })?;
        }

        // Verify the target resolves inside the workdir.
        let target_canonical = target.canonicalize().map_err(|e| {
            tracing::error!("failed to resolve target path: {e}");
            ApiError::internal("failed to resolve target path")
        })?;
        if !target_canonical.starts_with(&workdir_canonical) {
            // Target escaped the workdir (e.g., via a symlink). Remove it.
            let _ = std::fs::remove_file(&target_canonical);
            return Err(ApiError::bad_request(
                "target path escapes the working directory",
            ));
        }

        Ok(())
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(MoveFileResponse {
        from_path: req.from_path,
        to_path: req.to_path,
        success: true,
    }))
}
