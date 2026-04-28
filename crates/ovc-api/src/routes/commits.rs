//! Commit log, create, show, and diff endpoints.

use std::collections::{BinaryHeap, HashSet};
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;

use ovc_core::keys::{OvcPublicKey, VerifyResult, verify_commit};

use crate::auth::Claims;
use crate::error::ApiError;
use crate::models::{
    CommitAuthor, CommitInfo, CommitLog, CreateCommitRequest, DescribeResponse, DiffHunk, DiffLine,
    DiffLineKind, DiffResponse, DiffStats, FileDiff, ShortlogAuthorEntry, ShortlogResponse,
};
use crate::routes::files::find_workdir_for_repo;
use crate::routes::repos::open_repo_blocking;
use crate::state::AppState;

/// Query parameters for the log endpoint.
#[derive(Debug, Deserialize)]
pub struct LogQuery {
    /// Maximum number of commits to return.
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// When present, only return commits that touched this path.
    ///
    /// Mirrors `git log -- <path>`: a commit is included when the blob OID at
    /// `path` in the commit's tree differs from the blob OID at `path` in its
    /// first parent's tree, or when the file exists in a root commit's tree.
    pub path: Option<String>,
    /// Cursor for pagination: start the walk from this commit OID (exclusive)
    /// instead of HEAD. The commit identified by `after` is not included in
    /// the results — the walk begins from its parents.
    pub after: Option<String>,
}

const fn default_limit() -> usize {
    50
}

/// Maximum commit message size (64 KiB). Prevents storing excessively large
/// strings in the object store and superblock.
const MAX_COMMIT_MESSAGE_BYTES: usize = 64 * 1024;

/// Query parameters for the diff endpoint.
#[derive(Debug, Deserialize)]
pub struct DiffQuery {
    /// If `true`, show staged diff (index vs HEAD).
    #[serde(default)]
    pub staged: Option<bool>,
}

/// Handler: `GET /api/v1/repos/:id/log`
///
/// Returns the commit log following first-parent only (matching `git log`
/// default behavior). The commit count in [`RepoStats`](crate::models::RepoStats)
/// uses BFS across all parents for an accurate total.
#[allow(clippy::too_many_lines)]
pub async fn get_log(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<LogQuery>,
) -> Result<Json<CommitLog>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;
    let limit = query.limit.min(1000);
    let path_filter = query.path;
    let after_cursor = query.after;

    let log = tokio::task::spawn_blocking(move || -> Result<CommitLog, ApiError> {
        let authorized_keys = load_repo_authorized_keys(&repo);
        let mut commits = Vec::new();

        let Some(head_oid) = repo.ref_store().resolve_head().ok() else {
            return Ok(CommitLog { commits });
        };

        // BFS walk ordered by timestamp (newest first) so merged branch
        // commits are visible. Uses a max-heap keyed on (timestamp, oid) and
        // a visited set to avoid processing the same commit twice.
        let mut visited = HashSet::new();
        // Heap element: (timestamp, ObjectId) — BinaryHeap is a max-heap, so
        // the commit with the largest (newest) timestamp is popped first.
        let mut heap: BinaryHeap<(i64, ovc_core::id::ObjectId)> = BinaryHeap::new();

        // If an `after` cursor is provided, start from that commit's parents
        // instead of HEAD, avoiding re-walking already-fetched commits.
        if let Some(ref after_hex) = after_cursor {
            let after_oid = crate::routes::resolve_commit_spec(after_hex, &repo)?;
            visited.insert(after_oid);
            let obj = repo.get_object(&after_oid).map_err(ApiError::from_core)?;
            if let Some(ovc_core::object::Object::Commit(c)) = obj {
                for &parent_oid in &c.parents {
                    if visited.insert(parent_oid) {
                        let ts = repo
                            .get_object(&parent_oid)
                            .ok()
                            .flatten()
                            .and_then(|o| {
                                if let ovc_core::object::Object::Commit(pc) = o {
                                    Some(pc.author.timestamp)
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(0);
                        heap.push((ts, parent_oid));
                    }
                }
            }
        } else {
            visited.insert(head_oid);
            // Seed with a placeholder timestamp; we read the real one below.
            heap.push((i64::MAX, head_oid));
        }

        while let Some((_, oid)) = heap.pop() {
            if commits.len() >= limit {
                break;
            }

            let obj = repo.get_object(&oid).map_err(ApiError::from_core)?;
            let Some(ovc_core::object::Object::Commit(commit)) = obj else {
                continue;
            };

            // When a path filter is active, skip commits that did not touch
            // the requested path. We still traverse their parents so that we
            // can find earlier commits that did touch the file.
            if let Some(ref path) = path_filter
                && !commit_touched_path(&commit, path, &repo)?
            {
                // Enqueue parents before skipping — we must keep walking.
                for &parent_oid in &commit.parents {
                    if visited.insert(parent_oid) {
                        let ts = repo
                            .get_object(&parent_oid)
                            .ok()
                            .flatten()
                            .and_then(|obj| {
                                if let ovc_core::object::Object::Commit(c) = obj {
                                    Some(c.author.timestamp)
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(0);
                        heap.push((ts, parent_oid));
                    }
                }
                continue;
            }

            let (sig_status, sig_fp, sig_id) = signature_fields(&commit, &authorized_keys);

            let hex = oid.to_string();
            commits.push(CommitInfo {
                short_id: hex[..12.min(hex.len())].to_owned(),
                id: hex,
                message: commit.message.clone(),
                author: CommitAuthor {
                    name: commit.author.name.clone(),
                    email: commit.author.email.clone(),
                },
                authored_at: format_timestamp(commit.author.timestamp),
                parent_ids: commit.parents.iter().map(ToString::to_string).collect(),
                signature_status: sig_status,
                signer_fingerprint: sig_fp,
                signer_identity: sig_id,
            });

            // Enqueue all parents with their real timestamps.
            for &parent_oid in &commit.parents {
                if visited.insert(parent_oid) {
                    // Read the parent's timestamp for proper ordering.
                    let ts = repo
                        .get_object(&parent_oid)
                        .ok()
                        .flatten()
                        .and_then(|obj| {
                            if let ovc_core::object::Object::Commit(c) = obj {
                                Some(c.author.timestamp)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);
                    heap.push((ts, parent_oid));
                }
            }
        }

        Ok(CommitLog { commits })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(log))
}

/// Handler: `GET /api/v1/repos/:id/shortlog`
///
/// Returns commit counts grouped by author, analogous to `git shortlog -s -n`.
/// The result is sorted by commit count descending. Authors are identified by
/// the `(name, email)` pair from each commit's author field; duplicate
/// `(name, email)` combinations are merged into a single entry.
pub async fn get_shortlog(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ShortlogResponse>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let response = tokio::task::spawn_blocking(move || -> Result<ShortlogResponse, ApiError> {
        use std::collections::BinaryHeap;
        use std::collections::HashMap;
        use std::collections::HashSet;

        let Some(head_oid) = repo.ref_store().resolve_head().ok() else {
            return Ok(ShortlogResponse {
                authors: Vec::new(),
            });
        };

        // BFS over the full commit graph, deduplicating by OID to avoid
        // counting merge-base commits multiple times.
        let mut visited: HashSet<ovc_core::id::ObjectId> = HashSet::new();
        let mut heap: BinaryHeap<(i64, ovc_core::id::ObjectId)> = BinaryHeap::new();

        visited.insert(head_oid);
        heap.push((i64::MAX, head_oid));

        // Key: (name, email), Value: count.
        let mut counts: HashMap<(String, String), usize> = HashMap::new();

        while let Some((_, oid)) = heap.pop() {
            let obj = repo.get_object(&oid).map_err(ApiError::from_core)?;
            let Some(ovc_core::object::Object::Commit(commit)) = obj else {
                continue;
            };

            let key = (commit.author.name.clone(), commit.author.email.clone());
            *counts.entry(key).or_insert(0) += 1;

            for &parent_oid in &commit.parents {
                if visited.insert(parent_oid) {
                    let ts = repo
                        .get_object(&parent_oid)
                        .ok()
                        .flatten()
                        .and_then(|obj| {
                            if let ovc_core::object::Object::Commit(c) = obj {
                                Some(c.author.timestamp)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);
                    heap.push((ts, parent_oid));
                }
            }
        }

        let mut authors: Vec<ShortlogAuthorEntry> = counts
            .into_iter()
            .map(|((name, email), count)| ShortlogAuthorEntry { name, email, count })
            .collect();

        // Sort by count descending; break ties alphabetically by name.
        authors.sort_unstable_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));

        Ok(ShortlogResponse { authors })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(response))
}

/// Handler: `POST /api/v1/repos/:id/commits`
pub async fn create_commit(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<CreateCommitRequest>,
) -> Result<Json<CommitInfo>, ApiError> {
    if req.message.is_empty() {
        return Err(ApiError::bad_request("commit message must not be empty"));
    }

    // Cap commit message length to prevent storing excessively large strings
    // in the object store and superblock.
    if req.message.len() > MAX_COMMIT_MESSAGE_BYTES {
        return Err(ApiError::bad_request(
            "commit message must not exceed 64 KiB",
        ));
    }

    let repo_mtx = app.repo_lock(&id);
    let _repo_guard = repo_mtx.lock().await;
    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let amend = req.amend.unwrap_or(false);
    let sign_flag = req.sign;

    let info = tokio::task::spawn_blocking(move || -> Result<CommitInfo, ApiError> {
        let author = resolve_author(&req.author_name, &req.author_email);
        let should_sign = resolve_should_sign(sign_flag);

        if amend {
            execute_amend_commit(&mut repo, &req.message, &author, should_sign)
        } else {
            execute_new_commit(&mut repo, &req.message, &author, should_sign)
        }
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    // Invalidate cached stats: commit count changed.
    app.invalidate_repo_stats(&id);

    Ok(Json(info))
}

/// Resolves the author identity from request values, falling back to env vars.
fn resolve_author(name: &str, email: &str) -> ovc_core::object::Identity {
    let author_name = if name.is_empty() {
        std::env::var("OVC_AUTHOR_NAME").unwrap_or_else(|_| "Unknown".to_owned())
    } else {
        name.to_owned()
    };
    let author_email = if email.is_empty() {
        std::env::var("OVC_AUTHOR_EMAIL").unwrap_or_else(|_| "unknown@unknown".to_owned())
    } else {
        email.to_owned()
    };

    ovc_core::object::Identity {
        name: author_name,
        email: author_email,
        timestamp: chrono::Utc::now().timestamp(),
        tz_offset_minutes: 0,
    }
}

/// Determines whether to sign a commit. The request-level flag overrides the
/// `OVC_SIGN_COMMITS` environment variable.
fn resolve_should_sign(request_flag: Option<bool>) -> bool {
    request_flag.unwrap_or_else(|| {
        std::env::var("OVC_SIGN_COMMITS")
            .is_ok_and(|v| v == "true" || v == "1")
    })
}

/// Creates a new commit (non-amend path).
fn execute_new_commit(
    repo: &mut ovc_core::repository::Repository,
    message: &str,
    author: &ovc_core::object::Identity,
    should_sign: bool,
) -> Result<CommitInfo, ApiError> {
    let oid = if should_sign {
        let keypair = load_signing_keypair()?;
        repo.create_commit_signed(message, author, &keypair)
            .map_err(ApiError::from_core)?
    } else {
        repo.create_commit(message, author)
            .map_err(ApiError::from_core)?
    };

    repo.save().map_err(ApiError::from_core)?;

    // Read back the commit object to obtain the actual parent_ids.
    let parent_ids = repo
        .get_object(&oid)
        .ok()
        .flatten()
        .and_then(|obj| {
            if let ovc_core::object::Object::Commit(c) = obj {
                Some(c.parents.iter().map(ToString::to_string).collect())
            } else {
                None
            }
        })
        .unwrap_or_default();

    Ok(build_commit_info(
        oid,
        message,
        author,
        parent_ids,
        should_sign,
    ))
}

/// Amends the current HEAD commit with a new message/tree.
fn execute_amend_commit(
    repo: &mut ovc_core::repository::Repository,
    message: &str,
    author: &ovc_core::object::Identity,
    should_sign: bool,
) -> Result<CommitInfo, ApiError> {
    let head_oid = repo
        .ref_store()
        .resolve_head()
        .map_err(|_| ApiError::bad_request("cannot amend: no commits yet"))?;

    let head_obj = repo
        .get_object(&head_oid)
        .map_err(ApiError::from_core)?
        .ok_or_else(|| ApiError::internal("HEAD commit not found"))?;

    let ovc_core::object::Object::Commit(old_commit) = head_obj else {
        return Err(ApiError::bad_request("HEAD does not point to a commit"));
    };

    // Build tree from the current index.
    let (index, store) = repo.index_and_store_mut();
    let tree_oid = index.write_tree(store).map_err(ApiError::from_core)?;

    // Create the amended commit with the old commit's parents.
    let new_commit = ovc_core::object::Commit {
        tree: tree_oid,
        parents: old_commit.parents.clone(),
        author: author.clone(),
        committer: author.clone(),
        message: message.to_owned(),
        signature: None,
        sequence: old_commit.sequence,
    };

    let oid = if should_sign {
        insert_and_sign_commit(&new_commit, repo)?
    } else {
        repo.insert_object(&ovc_core::object::Object::Commit(new_commit))
            .map_err(ApiError::from_core)?
    };

    // Update the branch ref to point to the new commit.
    match repo.ref_store().head().clone() {
        ovc_core::refs::RefTarget::Symbolic(ref_name) => {
            let branch_name = ref_name
                .strip_prefix("refs/heads/")
                .unwrap_or(&ref_name)
                .to_owned();
            repo.ref_store_mut()
                .set_branch(
                    &branch_name,
                    oid,
                    author,
                    &format!("commit (amend): {message}"),
                )
                .map_err(ApiError::from_core)?;
        }
        ovc_core::refs::RefTarget::Direct(_) => {
            repo.ref_store_mut()
                .set_head(ovc_core::refs::RefTarget::Direct(oid));
        }
    }

    repo.save().map_err(ApiError::from_core)?;

    let parent_ids: Vec<String> = old_commit.parents.iter().map(ToString::to_string).collect();

    Ok(build_commit_info(
        oid,
        message,
        author,
        parent_ids,
        should_sign,
    ))
}

/// Constructs a `CommitInfo` response from commit components.
fn build_commit_info(
    oid: ovc_core::id::ObjectId,
    message: &str,
    author: &ovc_core::object::Identity,
    parent_ids: Vec<String>,
    signed: bool,
) -> CommitInfo {
    let hex = oid.to_string();
    CommitInfo {
        short_id: hex[..12.min(hex.len())].to_owned(),
        id: hex,
        message: message.to_owned(),
        author: CommitAuthor {
            name: author.name.clone(),
            email: author.email.clone(),
        },
        authored_at: format_timestamp(author.timestamp),
        parent_ids,
        signature_status: if signed {
            "unverified".to_owned()
        } else {
            "unsigned".to_owned()
        },
        signer_fingerprint: None,
        signer_identity: None,
    }
}

/// Loads the Ed25519 signing keypair from the `OVC_KEY` environment variable.
fn load_signing_keypair() -> Result<ovc_core::keys::OvcKeyPair, ApiError> {
    let key_query = std::env::var("OVC_KEY")
        .map_err(|_| ApiError::bad_request("signing requested but OVC_KEY is not set"))?;
    let pub_path = ovc_core::keys::find_key(&key_query)
        .map_err(|e| ApiError::internal(&format!("key search failed: {e}")))?
        .ok_or_else(|| ApiError::internal("signing key not found"))?;
    let priv_path = ovc_core::keys::private_key_path_for(&pub_path);
    let passphrase = std::env::var("OVC_KEY_PASSPHRASE").unwrap_or_default();
    ovc_core::keys::OvcKeyPair::load_private(&priv_path, passphrase.as_bytes())
        .map_err(|e| ApiError::internal(&format!("failed to load key: {e}")))
}

/// Inserts an unsigned commit and then signs it using the repository's
/// `sign_commit` method. Returns the signed commit's object id.
fn insert_and_sign_commit(
    commit: &ovc_core::object::Commit,
    repo: &mut ovc_core::repository::Repository,
) -> Result<ovc_core::id::ObjectId, ApiError> {
    let oid = repo
        .insert_object(&ovc_core::object::Object::Commit(commit.clone()))
        .map_err(ApiError::from_core)?;

    let keypair = load_signing_keypair()?;
    repo.sign_commit(&oid, &keypair)
        .map_err(ApiError::from_core)
}

/// Handler: `GET /api/v1/repos/:id/commits/:commit_id`
pub async fn get_commit(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, commit_id)): Path<(String, String)>,
) -> Result<Json<CommitInfo>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let info = tokio::task::spawn_blocking(move || -> Result<CommitInfo, ApiError> {
        let oid: ovc_core::id::ObjectId = commit_id
            .parse()
            .map_err(|_| ApiError::bad_request("invalid commit id format"))?;

        let obj = repo
            .get_object(&oid)
            .map_err(ApiError::from_core)?
            .ok_or_else(|| ApiError::not_found("commit not found"))?;

        let ovc_core::object::Object::Commit(commit) = obj else {
            return Err(ApiError::bad_request("object is not a commit"));
        };

        let authorized_keys = load_repo_authorized_keys(&repo);
        let (sig_status, sig_fp, sig_id) = signature_fields(&commit, &authorized_keys);

        let hex = oid.to_string();
        Ok(CommitInfo {
            short_id: hex[..12.min(hex.len())].to_owned(),
            id: hex,
            message: commit.message,
            author: CommitAuthor {
                name: commit.author.name,
                email: commit.author.email,
            },
            authored_at: format_timestamp(commit.author.timestamp),
            parent_ids: commit.parents.iter().map(ToString::to_string).collect(),
            signature_status: sig_status,
            signer_fingerprint: sig_fp,
            signer_identity: sig_id,
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(info))
}

/// Handler: `GET /api/v1/repos/:id/diff`
///
/// When `staged=true`, returns the diff between HEAD and the current index
/// (staged changes). When `staged=false` or omitted, returns the diff between
/// the index and the working directory (unstaged changes).
pub async fn get_diff(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<DiffQuery>,
) -> Result<Json<DiffResponse>, ApiError> {
    if query.staged == Some(true) {
        // Staged diff: index vs HEAD.
        let (repo, _) = open_repo_blocking(&app, &id).await?;
        let diff = tokio::task::spawn_blocking(move || -> Result<DiffResponse, ApiError> {
            compute_index_diff(&repo)
        })
        .await
        .map_err(|e| {
            tracing::error!("task join error: {e}");
            ApiError::internal("internal task error")
        })??;
        return Ok(Json(diff));
    }

    // Unstaged diff: working directory vs index.
    let workdir_path = find_workdir_for_repo(&app, &id).ok_or_else(|| {
        ApiError::bad_request(
            "no working directory found for this repository; \
             configure one via OVC_WORKDIR_MAP or OVC_WORKDIR_SCAN, \
             or use staged=true for the staged diff",
        )
    })?;

    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let diff = tokio::task::spawn_blocking(move || -> Result<DiffResponse, ApiError> {
        compute_workdir_diff(&repo, &workdir_path)
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(diff))
}

/// Handler: `GET /api/v1/repos/:id/diff/:commit_id`
///
/// Returns the diff introduced by a specific commit (compared to its first parent).
#[derive(Debug, Deserialize)]
pub struct CommitDiffQuery {
    /// If `true`, return only file stats (path, status, additions, deletions)
    /// without full hunk data. Much faster for large commits.
    #[serde(default)]
    pub stats_only: Option<bool>,
}

pub async fn get_commit_diff(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, commit_id)): Path<(String, String)>,
    Query(query): Query<CommitDiffQuery>,
) -> Result<Json<DiffResponse>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;
    let stats_only = query.stats_only.unwrap_or(false);

    let diff = tokio::task::spawn_blocking(move || -> Result<DiffResponse, ApiError> {
        let oid: ovc_core::id::ObjectId = commit_id
            .parse()
            .map_err(|_| ApiError::bad_request("invalid commit id"))?;

        let obj = repo
            .get_object(&oid)
            .map_err(ApiError::from_core)?
            .ok_or_else(|| ApiError::not_found("commit not found"))?;

        let ovc_core::object::Object::Commit(commit) = obj else {
            return Err(ApiError::bad_request("object is not a commit"));
        };

        // Build current commit's file index.
        let mut commit_index = ovc_core::index::Index::new();
        commit_index
            .read_tree(&commit.tree, repo.object_store())
            .map_err(ApiError::from_core)?;

        // Build parent's file index (empty if no parent).
        let mut parent_index = ovc_core::index::Index::new();
        if let Some(parent_oid) = commit.parents.first()
            && let Some(ovc_core::object::Object::Commit(parent_commit)) =
                repo.get_object(parent_oid).map_err(ApiError::from_core)?
        {
            parent_index
                .read_tree(&parent_commit.tree, repo.object_store())
                .map_err(ApiError::from_core)?;
        }

        if stats_only {
            compute_diff_between_indices_stats_only(&parent_index, &commit_index, &repo)
        } else {
            compute_diff_between_indices(&parent_index, &commit_index, &repo)
        }
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(diff))
}

/// Handler: `GET /api/v1/repos/:id/diff/:commit_id/file`
///
/// Returns the diff for a single file within a commit. Used for lazy-loading
/// individual file diffs when a commit has many changed files.
pub async fn get_commit_file_diff(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, commit_id)): Path<(String, String)>,
    Query(query): Query<FileDiffQuery>,
) -> Result<Json<FileDiff>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let file_diff = tokio::task::spawn_blocking(move || -> Result<FileDiff, ApiError> {
        let oid: ovc_core::id::ObjectId = commit_id
            .parse()
            .map_err(|_| ApiError::bad_request("invalid commit id"))?;

        let obj = repo
            .get_object(&oid)
            .map_err(ApiError::from_core)?
            .ok_or_else(|| ApiError::not_found("commit not found"))?;

        let ovc_core::object::Object::Commit(commit) = obj else {
            return Err(ApiError::bad_request("object is not a commit"));
        };

        // Resolve file OID in this commit's tree.
        let new_oid = repo
            .object_store()
            .lookup_path_in_tree(&commit.tree, &query.path)
            .map_err(ApiError::from_core)?;

        // Resolve file OID in parent's tree.
        let old_oid = commit
            .parents
            .first()
            .and_then(|parent_oid| repo.get_object(parent_oid).ok().flatten())
            .and_then(|obj| {
                if let ovc_core::object::Object::Commit(c) = obj {
                    Some(c)
                } else {
                    None
                }
            })
            .and_then(|parent_commit| {
                repo.object_store()
                    .lookup_path_in_tree(&parent_commit.tree, &query.path)
                    .ok()
                    .flatten()
            });

        let old_data = old_oid
            .map(|oid| get_blob_data(&repo, &oid))
            .transpose()?
            .unwrap_or_default();
        let new_data = new_oid
            .map(|oid| get_blob_data(&repo, &oid))
            .transpose()?
            .unwrap_or_default();

        let status = match (old_oid.is_some(), new_oid.is_some()) {
            (false, true) => "added",
            (true, false) => "deleted",
            _ => "modified",
        };

        let (hunks, adds, dels) = compute_hunks(&old_data, &new_data);

        Ok(FileDiff {
            path: query.path,
            status: status.to_owned(),
            additions: adds,
            deletions: dels,
            hunks,
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(file_diff))
}

#[derive(Debug, Deserialize)]
pub struct FileDiffQuery {
    /// File path to diff.
    pub path: String,
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Looks up the blob OID for `path` in a commit's tree using a targeted
/// tree walk. Only decompresses tree objects along the path — O(depth)
/// instead of O(total entries) for the full tree rebuild.
fn blob_oid_in_commit(
    commit: &ovc_core::object::Commit,
    path: &str,
    repo: &ovc_core::repository::Repository,
) -> Result<Option<ovc_core::id::ObjectId>, ApiError> {
    repo.object_store()
        .lookup_path_in_tree(&commit.tree, path)
        .map_err(ApiError::from_core)
}

/// Returns `true` when a commit touched `path` relative to its first parent.
///
/// For a root commit (no parents), the file is considered "touched" if it
/// exists in the commit's tree.
fn commit_touched_path(
    commit: &ovc_core::object::Commit,
    path: &str,
    repo: &ovc_core::repository::Repository,
) -> Result<bool, ApiError> {
    let current_oid = blob_oid_in_commit(commit, path, repo)?;

    let Some(parent_oid) = commit.parents.first() else {
        // Root commit: include if the file exists at all.
        return Ok(current_oid.is_some());
    };

    let parent_obj = repo
        .get_object(parent_oid)
        .map_err(ApiError::from_core)?
        .ok_or_else(|| ApiError::not_found(&format!("parent commit not found: {parent_oid}")))?;

    let ovc_core::object::Object::Commit(parent_commit) = parent_obj else {
        return Err(ApiError::bad_request("parent object is not a commit"));
    };

    let parent_blob_oid = blob_oid_in_commit(&parent_commit, path, repo)?;

    Ok(current_oid != parent_blob_oid)
}

/// Computes the diff between HEAD tree and the current index.
pub fn compute_index_diff(
    repo: &ovc_core::repository::Repository,
) -> Result<DiffResponse, ApiError> {
    let head_tree_oid = repo.ref_store().resolve_head().ok().and_then(|commit_oid| {
        repo.get_object(&commit_oid).ok().flatten().and_then(|obj| {
            if let ovc_core::object::Object::Commit(c) = obj {
                Some(c.tree)
            } else {
                None
            }
        })
    });

    let mut head_index = ovc_core::index::Index::new();
    if let Some(tree_oid) = head_tree_oid {
        head_index
            .read_tree(&tree_oid, repo.object_store())
            .map_err(ApiError::from_core)?;
    }

    compute_diff_between_indices(&head_index, repo.index(), repo)
}

/// Computes the diff between the working directory and the current index.
///
/// For each file that differs between the workdir and the index, produces a
/// `FileDiff` entry mirroring the format used by `compute_index_diff`.
fn compute_workdir_diff(
    repo: &ovc_core::repository::Repository,
    workdir_path: &std::path::Path,
) -> Result<DiffResponse, ApiError> {
    let head_tree_oid = repo.ref_store().resolve_head().ok().and_then(|commit_oid| {
        repo.get_object(&commit_oid).ok().flatten().and_then(|obj| {
            if let ovc_core::object::Object::Commit(c) = obj {
                Some(c.tree)
            } else {
                None
            }
        })
    });

    let workdir = ovc_core::workdir::WorkDir::new(workdir_path.to_path_buf());
    let ignore = ovc_core::ignore::IgnoreRules::load(workdir_path);
    let status_entries = workdir
        .compute_status(
            repo.index(),
            head_tree_oid.as_ref(),
            repo.object_store(),
            &ignore,
        )
        .map_err(ApiError::from_core)?;

    let mut files = Vec::new();
    let mut total_additions = 0u64;
    let mut total_deletions = 0u64;

    for entry in &status_entries {
        match entry.unstaged {
            ovc_core::workdir::FileStatus::Modified => {
                // Modified: diff index blob against workdir file content.
                let index_data = repo
                    .index()
                    .get_entry(&entry.path)
                    .map(|e| get_blob_data(repo, &e.oid))
                    .transpose()?
                    .unwrap_or_default();
                let workdir_file = workdir_path.join(&entry.path);
                let workdir_data = std::fs::read(&workdir_file).unwrap_or_default();

                let (hunks, adds, dels) = compute_hunks(&index_data, &workdir_data);
                total_additions += adds;
                total_deletions += dels;
                files.push(FileDiff {
                    path: entry.path.clone(),
                    status: "modified".to_owned(),
                    additions: adds,
                    deletions: dels,
                    hunks,
                });
            }
            ovc_core::workdir::FileStatus::Deleted => {
                // Deleted: entire index content shown as deletions.
                let index_data = repo
                    .index()
                    .get_entry(&entry.path)
                    .map(|e| get_blob_data(repo, &e.oid))
                    .transpose()?
                    .unwrap_or_default();

                let (hunks, adds, dels) = compute_hunks(&index_data, &[]);
                total_additions += adds;
                total_deletions += dels;
                files.push(FileDiff {
                    path: entry.path.clone(),
                    status: "deleted".to_owned(),
                    additions: adds,
                    deletions: dels,
                    hunks,
                });
            }
            _ => {}
        }

        // Untracked files (not in index) — show entire content as additions.
        if entry.staged == ovc_core::workdir::FileStatus::Untracked {
            let workdir_file = workdir_path.join(&entry.path);
            let workdir_data = std::fs::read(&workdir_file).unwrap_or_default();

            let (hunks, adds, dels) = compute_hunks(&[], &workdir_data);
            total_additions += adds;
            total_deletions += dels;
            files.push(FileDiff {
                path: entry.path.clone(),
                status: "added".to_owned(),
                additions: adds,
                deletions: dels,
                hunks,
            });
        }
    }

    Ok(DiffResponse {
        stats: DiffStats {
            files_changed: files.len() as u64,
            additions: total_additions,
            deletions: total_deletions,
        },
        files,
    })
}

/// Computes per-file diffs between two indices.
/// Maximum number of files to compute full hunks for in a single request.
/// Files beyond this limit get stats only (additions/deletions counts).
const MAX_FULL_DIFF_FILES: usize = 50;

pub fn compute_diff_between_indices(
    old_index: &ovc_core::index::Index,
    new_index: &ovc_core::index::Index,
    repo: &ovc_core::repository::Repository,
) -> Result<DiffResponse, ApiError> {
    compute_diff_between_indices_inner(old_index, new_index, repo, false)
}

pub fn compute_diff_between_indices_stats_only(
    old_index: &ovc_core::index::Index,
    new_index: &ovc_core::index::Index,
    repo: &ovc_core::repository::Repository,
) -> Result<DiffResponse, ApiError> {
    compute_diff_between_indices_inner(old_index, new_index, repo, true)
}

fn compute_diff_between_indices_inner(
    old_index: &ovc_core::index::Index,
    new_index: &ovc_core::index::Index,
    repo: &ovc_core::repository::Repository,
    stats_only: bool,
) -> Result<DiffResponse, ApiError> {
    let mut files = Vec::new();
    let mut total_additions = 0u64;
    let mut total_deletions = 0u64;

    // Collect all paths from both indices.
    let mut all_paths = std::collections::BTreeSet::new();
    for entry in old_index.entries() {
        all_paths.insert(entry.path.clone());
    }
    for entry in new_index.entries() {
        all_paths.insert(entry.path.clone());
    }

    let mut full_diff_count = 0usize;

    for path in &all_paths {
        let old_entry = old_index.get_entry(path);
        let new_entry = new_index.get_entry(path);

        match (old_entry, new_entry) {
            (Some(old), Some(new)) if old.oid == new.oid => {
                // Unchanged, skip.
            }
            (Some(old), Some(new)) => {
                // Modified.
                let old_data = get_blob_data(repo, &old.oid)?;
                let new_data = get_blob_data(repo, &new.oid)?;
                let skip_hunks = stats_only || full_diff_count >= MAX_FULL_DIFF_FILES;
                let (hunks, adds, dels) = if skip_hunks {
                    count_line_changes(&old_data, &new_data)
                } else {
                    full_diff_count += 1;
                    compute_hunks(&old_data, &new_data)
                };
                total_additions += adds;
                total_deletions += dels;
                files.push(FileDiff {
                    path: path.clone(),
                    status: "modified".to_owned(),
                    additions: adds,
                    deletions: dels,
                    hunks,
                });
            }
            (None, Some(new)) => {
                // Added.
                let new_data = get_blob_data(repo, &new.oid)?;
                let skip_hunks = stats_only || full_diff_count >= MAX_FULL_DIFF_FILES;
                let (hunks, adds, dels) = if skip_hunks {
                    count_line_changes(&[], &new_data)
                } else {
                    full_diff_count += 1;
                    compute_hunks(&[], &new_data)
                };
                total_additions += adds;
                total_deletions += dels;
                files.push(FileDiff {
                    path: path.clone(),
                    status: "added".to_owned(),
                    additions: adds,
                    deletions: dels,
                    hunks,
                });
            }
            (Some(old), None) => {
                // Deleted.
                let old_data = get_blob_data(repo, &old.oid)?;
                let skip_hunks = stats_only || full_diff_count >= MAX_FULL_DIFF_FILES;
                let (hunks, adds, dels) = if skip_hunks {
                    count_line_changes(&old_data, &[])
                } else {
                    full_diff_count += 1;
                    compute_hunks(&old_data, &[])
                };
                total_additions += adds;
                total_deletions += dels;
                files.push(FileDiff {
                    path: path.clone(),
                    status: "deleted".to_owned(),
                    additions: adds,
                    deletions: dels,
                    hunks,
                });
            }
            (None, None) => {}
        }
    }

    Ok(DiffResponse {
        stats: DiffStats {
            files_changed: files.len() as u64,
            additions: total_additions,
            deletions: total_deletions,
        },
        files,
    })
}

/// Fast line-count-only diff: counts additions and deletions without
/// building full hunks. Returns `(empty_hunks, additions, deletions)`.
#[allow(clippy::naive_bytecount)]
fn count_line_changes(old: &[u8], new: &[u8]) -> (Vec<DiffHunk>, u64, u64) {
    let old_lines = if old.is_empty() {
        0u64
    } else {
        old.iter().filter(|&&b| b == b'\n').count() as u64 + 1
    };
    let new_lines = if new.is_empty() {
        0u64
    } else {
        new.iter().filter(|&&b| b == b'\n').count() as u64 + 1
    };
    // For added files: all new lines are additions
    // For deleted files: all old lines are deletions
    // For modified files: approximate as delete-old + add-new (pessimistic but fast)
    if old.is_empty() {
        (Vec::new(), new_lines, 0)
    } else if new.is_empty() {
        (Vec::new(), 0, old_lines)
    } else if old == new {
        (Vec::new(), 0, 0)
    } else {
        // Run a quick line diff just for counts (no hunk construction).
        let core_hunks = ovc_core::diff::diff_to_hunks(old, new, 0);
        let mut adds = 0u64;
        let mut dels = 0u64;
        for h in &core_hunks {
            for line in &h.lines {
                match line {
                    ovc_core::diff::HunkLine::Addition(_) => adds += 1,
                    ovc_core::diff::HunkLine::Deletion(_) => dels += 1,
                    ovc_core::diff::HunkLine::Context(_) => {}
                }
            }
        }
        (Vec::new(), adds, dels)
    }
}

/// Retrieves blob data by object id.
fn get_blob_data(
    repo: &ovc_core::repository::Repository,
    oid: &ovc_core::id::ObjectId,
) -> Result<Vec<u8>, ApiError> {
    let obj = repo
        .get_object(oid)
        .map_err(ApiError::from_core)?
        .ok_or_else(|| ApiError::not_found("blob not found"))?;

    match obj {
        ovc_core::object::Object::Blob(data) => Ok(data),
        _ => Err(ApiError::internal("expected blob object")),
    }
}

/// Default number of context lines around each hunk.
const DEFAULT_CONTEXT_LINES: usize = 3;

/// Computes diff hunks and counts additions/deletions.
fn compute_hunks(old: &[u8], new: &[u8]) -> (Vec<DiffHunk>, u64, u64) {
    compute_hunks_with_context(old, new, DEFAULT_CONTEXT_LINES)
}

/// Computes diff hunks with a configurable number of context lines.
fn compute_hunks_with_context(
    old: &[u8],
    new: &[u8],
    context_lines: usize,
) -> (Vec<DiffHunk>, u64, u64) {
    let core_hunks = ovc_core::diff::diff_to_hunks(old, new, context_lines);
    let mut additions = 0u64;
    let mut deletions = 0u64;

    let hunks = core_hunks
        .into_iter()
        .map(|h| {
            let lines = h
                .lines
                .into_iter()
                .map(|line| match line {
                    ovc_core::diff::HunkLine::Context(data) => DiffLine {
                        kind: DiffLineKind::Context,
                        content: String::from_utf8_lossy(&data).into_owned(),
                    },
                    ovc_core::diff::HunkLine::Addition(data) => {
                        additions += 1;
                        DiffLine {
                            kind: DiffLineKind::Addition,
                            content: String::from_utf8_lossy(&data).into_owned(),
                        }
                    }
                    ovc_core::diff::HunkLine::Deletion(data) => {
                        deletions += 1;
                        DiffLine {
                            kind: DiffLineKind::Deletion,
                            content: String::from_utf8_lossy(&data).into_owned(),
                        }
                    }
                })
                .collect();

            DiffHunk {
                old_start: h.old_start,
                old_count: h.old_count,
                new_start: h.new_start,
                new_count: h.new_count,
                lines,
            }
        })
        .collect();

    (hunks, additions, deletions)
}

/// Formats a unix timestamp as an ISO 8601 string.
#[must_use]
pub fn format_timestamp(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0).map_or_else(|| secs.to_string(), |dt| dt.to_rfc3339())
}

/// Loads authorized public keys from a repository's key slots.
///
/// For each key slot fingerprint, attempts to find the corresponding `.pub`
/// file in `~/.ssh/ovc/`. Falls back to all local keys if no slots exist.
fn load_repo_authorized_keys(repo: &ovc_core::repository::Repository) -> Vec<OvcPublicKey> {
    let fingerprints = repo.list_keys();
    if fingerprints.is_empty() {
        return ovc_core::keys::list_keys()
            .ok()
            .map(|keys| {
                keys.into_iter()
                    .filter_map(|(_name, _fp, path)| OvcPublicKey::load(&path).ok())
                    .collect()
            })
            .unwrap_or_default();
    }

    fingerprints
        .into_iter()
        .filter_map(|fp| {
            ovc_core::keys::find_key(fp)
                .ok()
                .flatten()
                .and_then(|path| OvcPublicKey::load(&path).ok())
        })
        .collect()
}

/// Extracts signature status fields from a commit for API responses.
fn signature_fields(
    commit: &ovc_core::object::Commit,
    authorized_keys: &[OvcPublicKey],
) -> (String, Option<String>, Option<String>) {
    let result = verify_commit(commit, authorized_keys);
    match result {
        VerifyResult::Verified {
            fingerprint,
            identity,
        } => (
            "verified".to_owned(),
            Some(fingerprint),
            identity.map(|id| id.to_string()),
        ),
        VerifyResult::Unverified { .. } => ("unverified".to_owned(), None, None),
        VerifyResult::NotSigned => ("unsigned".to_owned(), None, None),
    }
}

/// Handler: `GET /api/v1/repos/:id/describe/:commit_id`
///
/// Returns the nearest tag description for a commit. Walks first-parent ancestry
/// from the given commit, returning the first tag found with the distance.
pub async fn describe_commit(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path((id, commit_id)): Path<(String, String)>,
) -> Result<Json<DescribeResponse>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let response = tokio::task::spawn_blocking(move || -> Result<DescribeResponse, ApiError> {
        let target_oid: ovc_core::id::ObjectId = commit_id
            .parse()
            .map_err(|_| ApiError::bad_request("invalid commit id format"))?;

        // Build a reverse map: oid -> tag name.
        let tags = repo.ref_store().list_tags();
        let mut oid_to_tag: std::collections::HashMap<ovc_core::id::ObjectId, String> =
            std::collections::HashMap::new();
        for (name, oid, _msg) in &tags {
            oid_to_tag.insert(**oid, (*name).to_owned());
        }

        // Walk first-parent ancestry from the target commit.
        let mut current = Some(target_oid);
        let mut depth = 0usize;
        let max_depth = 10_000;

        while let Some(oid) = current {
            if let Some(tag_name) = oid_to_tag.get(&oid) {
                let description = if depth == 0 {
                    tag_name.clone()
                } else {
                    format!("{tag_name}~{depth}")
                };
                return Ok(DescribeResponse {
                    commit_id: target_oid.to_string(),
                    description,
                });
            }

            if depth >= max_depth {
                break;
            }

            let obj = repo
                .get_object(&oid)
                .map_err(ApiError::from_core)?
                .ok_or_else(|| ApiError::not_found("commit not found during ancestry walk"))?;

            let ovc_core::object::Object::Commit(commit) = obj else {
                break;
            };

            current = commit.parents.first().copied();
            depth += 1;
        }

        // No tag found in ancestry.
        let hex = target_oid.to_string();
        Ok(DescribeResponse {
            commit_id: hex.clone(),
            description: hex[..12.min(hex.len())].to_owned(),
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(response))
}

/// Query parameters for the compare endpoint.
#[derive(Debug, Deserialize)]
pub struct CompareQuery {
    /// Base ref (branch name, tag name, or full hex commit id).
    pub base: String,
    /// Head ref (branch name, tag name, or full hex commit id).
    pub head: String,
}

/// Handler: `GET /api/v1/repos/:id/compare?base=REF&head=REF`
///
/// Computes a tree-level diff between any two refs. Both `base` and `head` are
/// resolved through [`super::resolve_commit_spec`], so branch names, tag
/// names, `HEAD`, `HEAD~N`, and full hex OIDs are all accepted.
pub async fn get_compare(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<CompareQuery>,
) -> Result<Json<DiffResponse>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let diff = tokio::task::spawn_blocking(move || -> Result<DiffResponse, ApiError> {
        let base_oid = super::resolve_commit_spec(&query.base, &repo)?;
        let head_oid = super::resolve_commit_spec(&query.head, &repo)?;

        // Load the tree index for the base commit.
        let base_obj = repo
            .get_object(&base_oid)
            .map_err(ApiError::from_core)?
            .ok_or_else(|| ApiError::not_found(&format!("base commit not found: {base_oid}")))?;
        let ovc_core::object::Object::Commit(base_commit) = base_obj else {
            return Err(ApiError::bad_request(
                "base ref does not resolve to a commit",
            ));
        };
        let mut base_index = ovc_core::index::Index::new();
        base_index
            .read_tree(&base_commit.tree, repo.object_store())
            .map_err(ApiError::from_core)?;

        // Load the tree index for the head commit.
        let head_obj = repo
            .get_object(&head_oid)
            .map_err(ApiError::from_core)?
            .ok_or_else(|| ApiError::not_found(&format!("head commit not found: {head_oid}")))?;
        let ovc_core::object::Object::Commit(head_commit) = head_obj else {
            return Err(ApiError::bad_request(
                "head ref does not resolve to a commit",
            ));
        };
        let mut head_index = ovc_core::index::Index::new();
        head_index
            .read_tree(&head_commit.tree, repo.object_store())
            .map_err(ApiError::from_core)?;

        compute_diff_between_indices(&base_index, &head_index, &repo)
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(diff))
}
