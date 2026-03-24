//! Router construction for the OVC REST API.

pub mod access;
pub mod actions;
pub mod advanced;
pub mod archive;
pub mod branches;
pub mod commits;
pub mod docs;
pub mod files;
pub mod health;
pub mod notes;
pub mod pulls;
pub mod remotes;
pub mod repos;
pub mod search;
pub mod submodules;
pub mod sync;
pub mod tags;

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{Method, header};
use axum::routing::{delete, get, post, put};
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

use crate::auth;
use crate::error::ApiError;
use crate::state::AppState;

/// Resolves a commit specifier to an `ObjectId`.
///
/// Accepts: full 64-char hex, HEAD, HEAD~N, branch name, or tag name.
pub fn resolve_commit_spec(
    spec: &str,
    repo: &ovc_core::repository::Repository,
) -> Result<ovc_core::id::ObjectId, ApiError> {
    // Full hex ObjectId.
    if let Ok(oid) = spec.parse::<ovc_core::id::ObjectId>() {
        return Ok(oid);
    }

    // HEAD.
    if spec.eq_ignore_ascii_case("HEAD") {
        return repo.ref_store().resolve_head().map_err(ApiError::from_core);
    }

    // HEAD~N.
    if let Some(rest) = spec.strip_prefix("HEAD~") {
        let n: usize = rest
            .parse()
            .map_err(|_| ApiError::bad_request("invalid HEAD~N syntax"))?;
        let mut oid = repo
            .ref_store()
            .resolve_head()
            .map_err(ApiError::from_core)?;
        for _ in 0..n {
            let obj = repo
                .get_object(&oid)
                .map_err(ApiError::from_core)?
                .ok_or_else(|| ApiError::not_found(&format!("commit not found: {oid}")))?;
            match obj {
                ovc_core::object::Object::Commit(c) => {
                    if c.parents.is_empty() {
                        return Err(ApiError::bad_request(&format!(
                            "reached root commit before HEAD~{n}"
                        )));
                    }
                    oid = c.parents[0];
                }
                _ => {
                    return Err(ApiError::bad_request(&format!(
                        "object is not a commit: {oid}"
                    )));
                }
            }
        }
        return Ok(oid);
    }

    // Branch name.
    let branch_ref = format!("refs/heads/{spec}");
    if let Ok(oid) = repo.ref_store().resolve(&branch_ref) {
        return Ok(oid);
    }

    // Tag name.
    let tag_ref = format!("refs/tags/{spec}");
    if let Ok(oid) = repo.ref_store().resolve(&tag_ref) {
        return Ok(oid);
    }

    Err(ApiError::bad_request(&format!(
        "cannot resolve commit spec: {spec}"
    )))
}

/// Maximum request body size (16 MiB). Prevents memory exhaustion from
/// oversized payloads sent to the API server.
///
/// This value is the effective ceiling for all per-file and per-request size
/// checks in `files.rs` (`MAX_UPLOAD_FILE_BYTES`). The two constants must be
/// kept in sync: axum rejects requests above this limit before any handler
/// logic runs, making a higher per-file limit unreachable dead code.
const MAX_REQUEST_BODY_BYTES: usize = 16 * 1024 * 1024;

/// Characters that are forbidden in branch and tag names.
///
/// This follows git-check-ref-format rules: disallow path traversal sequences,
/// control characters, and special refspec characters that could be used for
/// injection or confuse downstream tooling.
pub fn validate_ref_name(name: &str) -> Result<(), ApiError> {
    if name.is_empty() {
        return Err(ApiError::bad_request("ref name must not be empty"));
    }
    if name.len() > 256 {
        return Err(ApiError::bad_request(
            "ref name must not exceed 256 characters",
        ));
    }
    if name.contains("..") {
        return Err(ApiError::bad_request("ref name must not contain '..'"));
    }

    for ch in name.chars() {
        if ch.is_control() || ch == '\0' {
            return Err(ApiError::bad_request(
                "ref name must not contain control characters or null bytes",
            ));
        }
        if matches!(ch, '\\' | '~' | '^' | ':' | '?' | '*' | '[' | ' ') {
            return Err(ApiError::bad_request(&format!(
                "ref name must not contain '{ch}'"
            )));
        }
    }

    Ok(())
}

/// Builds the complete API router with all middleware layers.
///
/// When `cors_origins` is non-empty, those origins are used for the CORS
/// `Access-Control-Allow-Origin` header. Otherwise, localhost defaults are
/// used for local development.
#[allow(clippy::too_many_lines)]
pub fn build_router(state: Arc<AppState>, cors_origins: &[String]) -> Router {
    let public_routes = Router::new()
        .route("/health", get(health::health))
        .route("/auth/token", post(auth::create_token))
        .route("/auth/verify", get(auth::verify_token))
        .route("/auth/challenge", get(auth::get_challenge))
        .route("/auth/key-auth", post(auth::key_auth))
        .route("/docs", get(docs::get_docs_index))
        .route("/docs/search", get(docs::search_docs))
        .route("/docs/{category}/{section}", get(docs::get_doc_section));

    let repo_routes = Router::new()
        .route("/", get(repos::list_repos).post(repos::create_repo))
        .route("/{id}", get(repos::get_repo).delete(repos::delete_repo))
        .route("/{id}/unlock", post(repos::unlock_repo))
        .route(
            "/{id}/config",
            get(repos::get_repo_config).put(repos::update_repo_config),
        )
        // File operations
        .route("/{id}/status", get(files::get_status))
        .route("/{id}/tree", get(files::get_tree))
        .route(
            "/{id}/blob",
            get(files::get_blob)
                .put(files::put_blob)
                .delete(files::delete_blob),
        )
        .route("/{id}/upload", post(files::upload_files))
        .route("/{id}/mkdir", post(files::mkdir))
        .route("/{id}/move", post(files::move_file))
        .route("/{id}/stage", post(files::stage_files))
        .route("/{id}/unstage", post(files::unstage_files))
        .route("/{id}/restore", post(files::restore_files))
        .route("/{id}/clean", post(files::clean_files))
        // Commit operations
        .route("/{id}/log", get(commits::get_log))
        .route("/{id}/compare", get(commits::get_compare))
        .route("/{id}/shortlog", get(commits::get_shortlog))
        .route("/{id}/commits", post(commits::create_commit))
        .route("/{id}/commits/{commit_id}", get(commits::get_commit))
        .route("/{id}/diff", get(commits::get_diff))
        .route("/{id}/diff/{commit_id}", get(commits::get_commit_diff))
        .route(
            "/{id}/diff/{commit_id}/file",
            get(commits::get_commit_file_diff),
        )
        // Branch operations
        .route(
            "/{id}/branches",
            get(branches::list_branches).post(branches::create_branch),
        )
        .route("/{id}/branches/{name}", delete(branches::delete_branch))
        .route(
            "/{id}/branches/{name}/checkout",
            post(branches::checkout_branch),
        )
        .route("/{id}/branches/{name}/merge", post(branches::merge_branch))
        // Pull request lifecycle (must come BEFORE the `/{branch}` wildcard)
        .route(
            "/{id}/pulls",
            get(pulls::list_pull_requests).post(pulls::create_pull_request),
        )
        .route(
            "/{id}/pulls/by-number/{number}",
            get(pulls::get_pull_request_by_number).patch(pulls::update_pull_request),
        )
        .route(
            "/{id}/pulls/by-number/{number}/merge",
            post(pulls::merge_pull_request),
        )
        .route(
            "/{id}/pulls/by-number/{number}/checks",
            post(pulls::run_pr_checks),
        )
        .route(
            "/{id}/pulls/by-number/{number}/reviews",
            get(pulls::list_reviews).post(pulls::create_review),
        )
        .route(
            "/{id}/pulls/by-number/{number}/comments",
            get(pulls::list_comments).post(pulls::create_comment),
        )
        .route(
            "/{id}/pulls/by-number/{number}/comments/{comment_id}",
            axum::routing::patch(pulls::update_comment).delete(pulls::delete_comment),
        )
        // Pull request / branch comparison (wildcard — must come after specific routes)
        .route("/{id}/pulls/{branch}", get(pulls::get_pull_request))
        // Access management
        .route("/{id}/access", get(access::list_access))
        .route("/{id}/access/grant", post(access::grant_access))
        .route("/{id}/access/revoke", post(access::revoke_access))
        .route("/{id}/access/{fingerprint}/role", put(access::set_role))
        // Branch protection
        .route("/{id}/branch-protect", get(access::list_branch_protection))
        .route(
            "/{id}/branch-protect/{name}",
            put(access::set_branch_protection).delete(access::remove_branch_protection),
        )
        // Tag operations
        .route("/{id}/tags", get(tags::list_tags).post(tags::create_tag))
        .route("/{id}/tags/{name}", delete(tags::delete_tag))
        // Sync operations
        .route("/{id}/sync/status", get(sync::sync_status))
        .route("/{id}/sync/push", post(sync::sync_push))
        .route("/{id}/sync/pull", post(sync::sync_pull))
        // Stash operations
        .route(
            "/{id}/stash",
            get(advanced::list_stash)
                .post(advanced::push_stash)
                .delete(advanced::clear_stash),
        )
        .route("/{id}/stash/{idx}/pop", post(advanced::pop_stash))
        .route("/{id}/stash/{idx}/apply", post(advanced::apply_stash))
        .route("/{id}/stash/{idx}", delete(advanced::drop_stash))
        // Reset
        .route("/{id}/reset", post(advanced::reset))
        // Rebase, cherry-pick, revert, GC
        .route("/{id}/rebase", post(advanced::rebase_branch))
        .route("/{id}/cherry-pick", post(advanced::cherry_pick))
        .route("/{id}/revert", post(advanced::revert_commit))
        .route("/{id}/gc", post(advanced::garbage_collect))
        // Remote management
        .route(
            "/{id}/remotes",
            get(remotes::list_remotes).post(remotes::add_remote),
        )
        .route("/{id}/remotes/{name}", delete(remotes::remove_remote))
        // Blame
        .route("/{id}/blame/{*path}", get(files::get_blame))
        // Search
        .route("/{id}/search", get(search::search))
        // Notes
        .route("/{id}/notes", get(notes::list_notes))
        .route(
            "/{id}/notes/{commit_id}",
            get(notes::get_note)
                .put(notes::set_note)
                .delete(notes::remove_note),
        )
        // Reflog
        .route("/{id}/reflog", get(advanced::get_reflog))
        // Describe
        .route("/{id}/describe/{commit_id}", get(commits::describe_commit))
        // Archive
        .route("/{id}/archive", get(archive::get_archive))
        // Submodules
        .route(
            "/{id}/submodules",
            get(submodules::list_submodules).post(submodules::add_submodule),
        )
        .route(
            "/{id}/submodules/{name}",
            delete(submodules::remove_submodule),
        )
        // Actions
        .route(
            "/{id}/actions/config",
            get(actions::get_config).put(actions::put_config),
        )
        .route(
            "/{id}/actions/config/{name}",
            get(actions::get_action_config)
                .put(actions::put_action_config)
                .delete(actions::delete_action_config),
        )
        .route("/{id}/actions/list", get(actions::list_actions))
        .route("/{id}/actions/run", post(actions::run_actions))
        .route("/{id}/actions/run/{name}", post(actions::run_single_action))
        .route("/{id}/actions/detect", get(actions::detect))
        .route(
            "/{id}/actions/history",
            get(actions::list_history).delete(actions::clear_history),
        )
        .route("/{id}/actions/history/{run_id}", get(actions::get_run))
        .route("/{id}/actions/init", post(actions::init_config))
        // Secrets management
        .route("/{id}/actions/secrets", get(actions::list_secrets))
        .route(
            "/{id}/actions/secrets/{name}",
            put(actions::set_secret).delete(actions::delete_secret),
        )
        // Docker status
        .route("/{id}/actions/docker/status", get(actions::docker_status))
        // Dependency update check
        .route("/{id}/dependencies", get(actions::get_dependencies))
        // Dependency auto-update (Dependabot-style)
        .route(
            "/{id}/dependencies/update",
            post(actions::update_dependencies),
        )
        .route("/{id}/dependencies/proposals", get(actions::list_proposals))
        .route(
            "/{id}/dependencies/proposals/{branch}",
            delete(actions::delete_proposal),
        )
        .route(
            "/{id}/dependencies/proposals/{branch}/merge",
            post(actions::merge_proposal),
        )
        .route(
            "/{id}/dependencies/proposals/{branch}/create-pr",
            post(actions::create_pr_from_proposal),
        );

    let origins: Vec<axum::http::HeaderValue> = if cors_origins.is_empty() {
        vec![
            "http://localhost:3000".parse().expect("valid origin"),
            "http://localhost:5173".parse().expect("valid origin"),
            "http://127.0.0.1:3000".parse().expect("valid origin"),
            "http://127.0.0.1:5173".parse().expect("valid origin"),
        ]
    } else {
        cors_origins
            .iter()
            .filter_map(|o| {
                o.parse().ok().or_else(|| {
                    tracing::warn!("ignoring invalid CORS origin: {o}");
                    None
                })
            })
            .collect()
    };

    Router::new()
        .nest("/api/v1", public_routes.nest("/repos", repo_routes))
        .fallback(crate::static_files::serve_frontend)
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .layer(
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::PATCH,
                    Method::DELETE,
                    Method::OPTIONS,
                ])
                .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
                .max_age(Duration::from_secs(3600)),
        )
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        // Security headers — defense-in-depth against XSS, clickjacking, and
        // MIME type sniffing.
        .layer(SetResponseHeaderLayer::overriding(
            header::X_FRAME_OPTIONS,
            axum::http::HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            axum::http::HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            axum::http::HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::HeaderName::from_static("x-xss-protection"),
            axum::http::HeaderValue::from_static("1; mode=block"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::HeaderName::from_static("permissions-policy"),
            axum::http::HeaderValue::from_static(
                "camera=(), microphone=(), geolocation=(), payment=()",
            ),
        ))
        .with_state(state)
}
