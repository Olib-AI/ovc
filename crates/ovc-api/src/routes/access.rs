//! Access management endpoints.
//!
//! Provides endpoints for managing per-user access control and branch
//! protection rules on OVC repositories.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as AxPath, State};

use ovc_core::access::{AccessRole, BranchProtection};
use ovc_core::keys::OvcPublicKey;

use crate::auth::{self, Claims};
use crate::error::ApiError;
use crate::models::{
    BranchProtectionInfo, GrantAccessRequest, ListAccessResponse, SetBranchProtectionRequest,
    SetRoleRequest, UserAccessInfo,
};
use crate::routes::repos::open_repo_blocking;
use crate::state::AppState;

/// Handler: `GET /api/v1/repos/:id/access`
///
/// Lists all users with access to the repository.
pub async fn list_access(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath(id): AxPath<String>,
) -> Result<Json<ListAccessResponse>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    // Fall back to the repo config's user identity for entries with no
    // stored identity (common for the repo creator when env vars were used).
    let config_identity = {
        let cfg = repo.config();
        if cfg.user_name.is_empty() && cfg.user_email.is_empty() {
            None
        } else if cfg.user_email.is_empty() {
            Some(cfg.user_name.clone())
        } else {
            Some(format!("{} <{}>", cfg.user_name, cfg.user_email))
        }
    };

    let users = repo
        .access_control()
        .users
        .iter()
        .map(|u| {
            let is_creator = u.role == AccessRole::Owner && u.fingerprint == u.added_by;
            let identity = u.identity.as_ref().map(ToString::to_string).or_else(|| {
                if is_creator {
                    config_identity.clone()
                } else {
                    None
                }
            });
            UserAccessInfo {
                is_repo_creator: is_creator,
                fingerprint: u.fingerprint.clone(),
                role: u.role.to_string(),
                identity,
                added_at: u.added_at.clone(),
                added_by: u.added_by.clone(),
            }
        })
        .collect();

    Ok(Json(ListAccessResponse { users }))
}

/// Handler: `POST /api/v1/repos/:id/access/grant`
///
/// Grants access to a new user by their public key.
pub async fn grant_access(
    claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath(id): AxPath<String>,
    Json(req): Json<GrantAccessRequest>,
) -> Result<Json<UserAccessInfo>, ApiError> {
    auth::require_role(&claims, "owner")?;

    let role = AccessRole::parse(&req.role)
        .ok_or_else(|| ApiError::bad_request(&format!("invalid role: {}", req.role)))?;

    let public_key = if let Some(pem) = &req.public_key_pem {
        OvcPublicKey::parse(pem)
            .map_err(|e| ApiError::bad_request(&format!("invalid public key: {e}")))?
    } else if let Some(fingerprint) = &req.fingerprint {
        // Try to find the key by fingerprint in the user's local key store.
        let fp = fingerprint.clone();
        tokio::task::spawn_blocking(move || {
            let path = ovc_core::keys::find_key(&fp)
                .map_err(|e| ApiError::internal(&format!("key lookup failed: {e}")))?
                .ok_or_else(|| {
                    ApiError::not_found(&format!("no key found for fingerprint: {fp}"))
                })?;
            OvcPublicKey::load(&path)
                .map_err(|e| ApiError::bad_request(&format!("failed to load key: {e}")))
        })
        .await
        .map_err(|e| {
            tracing::error!("task join error: {e}");
            ApiError::internal("internal task error")
        })??
    } else {
        return Err(ApiError::bad_request(
            "must provide either 'public_key_pem' or 'fingerprint'",
        ));
    };

    let grantor = claims.sub.clone();
    let lock = app.repo_lock(&id);
    let _guard = lock.lock().await;

    let (mut repo, _ovc_path) = open_repo_blocking(&app, &id).await?;

    let info = tokio::task::spawn_blocking(move || -> Result<UserAccessInfo, ApiError> {
        repo.grant_access(&public_key, role, &grantor)
            .map_err(ApiError::from_core)?;
        repo.save().map_err(ApiError::from_core)?;

        let user = repo
            .access_control()
            .user_for(&public_key.fingerprint)
            .ok_or_else(|| ApiError::internal("user not found after grant"))?;

        Ok(UserAccessInfo {
            is_repo_creator: false, // newly granted users are never the creator
            fingerprint: user.fingerprint.clone(),
            role: user.role.to_string(),
            identity: user.identity.as_ref().map(ToString::to_string),
            added_at: user.added_at.clone(),
            added_by: user.added_by.clone(),
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(info))
}

/// Handler: `POST /api/v1/repos/:id/access/revoke`
///
/// Revokes a user's access by fingerprint.
pub async fn revoke_access(
    claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath(id): AxPath<String>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    auth::require_role(&claims, "owner")?;

    let fingerprint = req
        .get("fingerprint")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::bad_request("'fingerprint' is required"))?
        .to_owned();

    let lock = app.repo_lock(&id);
    let _guard = lock.lock().await;

    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    // Prevent revoking the repo creator — their key is the primary encryption
    // key. Removing it would make the repository unreadable.
    if let Some(user) = repo.access_control().user_for(&fingerprint)
        && user.role == AccessRole::Owner
        && user.fingerprint == user.added_by
    {
        return Err(ApiError::conflict(
            "cannot revoke the repo creator — their key is required to decrypt the repository",
        ));
    }

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        repo.revoke_access(&fingerprint)
            .map_err(ApiError::from_core)?;
        repo.save().map_err(ApiError::from_core)?;
        Ok(())
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    // Invalidate all outstanding tokens so the revoked user's session
    // cannot continue operating with their old permissions.
    app.revoke_all_tokens();

    Ok(Json(serde_json::json!({ "revoked": true })))
}

/// Handler: `PUT /api/v1/repos/:id/access/:fingerprint/role`
///
/// Changes a user's role.
pub async fn set_role(
    claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath((id, fingerprint)): AxPath<(String, String)>,
    Json(req): Json<SetRoleRequest>,
) -> Result<Json<UserAccessInfo>, ApiError> {
    auth::require_role(&claims, "owner")?;

    let role = AccessRole::parse(&req.role)
        .ok_or_else(|| ApiError::bad_request(&format!("invalid role: {}", req.role)))?;

    let lock = app.repo_lock(&id);
    let _guard = lock.lock().await;

    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let info = tokio::task::spawn_blocking(move || -> Result<UserAccessInfo, ApiError> {
        repo.set_role(&fingerprint, role)
            .map_err(ApiError::from_core)?;
        repo.save().map_err(ApiError::from_core)?;

        let user = repo
            .access_control()
            .user_for(&fingerprint)
            .ok_or_else(|| ApiError::not_found("user not found after role update"))?;

        Ok(UserAccessInfo {
            is_repo_creator: user.role == AccessRole::Owner && user.fingerprint == user.added_by,
            fingerprint: user.fingerprint.clone(),
            role: user.role.to_string(),
            identity: user.identity.as_ref().map(ToString::to_string),
            added_at: user.added_at.clone(),
            added_by: user.added_by.clone(),
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    // Invalidate all outstanding tokens so role changes take effect
    // immediately for all active sessions.
    app.revoke_all_tokens();

    Ok(Json(info))
}

/// Handler: `GET /api/v1/repos/:id/branch-protect`
///
/// Lists all branch protection rules.
pub async fn list_branch_protection(
    _claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath(id): AxPath<String>,
) -> Result<Json<Vec<BranchProtectionInfo>>, ApiError> {
    let (repo, _) = open_repo_blocking(&app, &id).await?;

    let rules: Vec<BranchProtectionInfo> = repo
        .access_control()
        .branch_protection
        .iter()
        .map(|(branch, p)| BranchProtectionInfo {
            branch: branch.clone(),
            required_approvals: p.required_approvals,
            require_ci_pass: p.require_ci_pass,
            allowed_merge_roles: p
                .allowed_merge_roles
                .iter()
                .map(ToString::to_string)
                .collect(),
            allowed_push_roles: p
                .allowed_push_roles
                .iter()
                .map(ToString::to_string)
                .collect(),
        })
        .collect();

    Ok(Json(rules))
}

/// Handler: `PUT /api/v1/repos/:id/branch-protect/:name`
///
/// Sets branch protection rules for a branch.
pub async fn set_branch_protection(
    claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath((id, branch)): AxPath<(String, String)>,
    Json(req): Json<SetBranchProtectionRequest>,
) -> Result<Json<BranchProtectionInfo>, ApiError> {
    auth::require_role(&claims, "admin")?;

    let lock = app.repo_lock(&id);
    let _guard = lock.lock().await;

    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    let parse_roles = |roles: &[String]| -> Result<Vec<AccessRole>, ApiError> {
        roles
            .iter()
            .map(|r| {
                AccessRole::parse(r)
                    .ok_or_else(|| ApiError::bad_request(&format!("invalid role: {r}")))
            })
            .collect()
    };

    let protection = BranchProtection {
        required_approvals: req.required_approvals.unwrap_or(0),
        require_ci_pass: req.require_ci_pass.unwrap_or(false),
        allowed_merge_roles: req
            .allowed_merge_roles
            .as_ref()
            .map(|r| parse_roles(r))
            .transpose()?
            .unwrap_or_else(|| vec![AccessRole::Admin, AccessRole::Owner]),
        allowed_push_roles: req
            .allowed_push_roles
            .as_ref()
            .map(|r| parse_roles(r))
            .transpose()?
            .unwrap_or_else(|| vec![AccessRole::Admin, AccessRole::Owner]),
    };

    let branch_clone = branch.clone();
    let info = tokio::task::spawn_blocking(move || -> Result<BranchProtectionInfo, ApiError> {
        repo.set_branch_protection(&branch_clone, protection.clone())
            .map_err(ApiError::from_core)?;
        repo.save().map_err(ApiError::from_core)?;

        Ok(BranchProtectionInfo {
            branch: branch_clone,
            required_approvals: protection.required_approvals,
            require_ci_pass: protection.require_ci_pass,
            allowed_merge_roles: protection
                .allowed_merge_roles
                .iter()
                .map(ToString::to_string)
                .collect(),
            allowed_push_roles: protection
                .allowed_push_roles
                .iter()
                .map(ToString::to_string)
                .collect(),
        })
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(info))
}

/// Handler: `DELETE /api/v1/repos/:id/branch-protect/:name`
///
/// Removes branch protection rules.
pub async fn remove_branch_protection(
    claims: Claims,
    State(app): State<Arc<AppState>>,
    AxPath((id, branch)): AxPath<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    auth::require_role(&claims, "admin")?;

    let lock = app.repo_lock(&id);
    let _guard = lock.lock().await;

    let (mut repo, _) = open_repo_blocking(&app, &id).await?;

    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        repo.remove_branch_protection(&branch)
            .map_err(ApiError::from_core)?;
        repo.save().map_err(ApiError::from_core)?;
        Ok(())
    })
    .await
    .map_err(|e| {
        tracing::error!("task join error: {e}");
        ApiError::internal("internal task error")
    })??;

    Ok(Json(serde_json::json!({ "removed": true })))
}
