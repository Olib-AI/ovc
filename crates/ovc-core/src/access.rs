//! Per-user access control for OVC repositories.
//!
//! Provides role-based access control (RBAC) where each authorized user
//! (identified by their key fingerprint) is assigned a role that governs
//! what operations they may perform.
//!
//! # Roles (ascending privilege)
//!
//! - **Read** — clone, view, comment on PRs
//! - **Write** — commit, push, create PRs, create branches
//! - **Admin** — manage branches, merge PRs, manage actions
//! - **Owner** — full control, manage access, transfer ownership

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::keys::KeyIdentity;

/// Access roles ordered by ascending privilege level.
///
/// Derives `PartialOrd`/`Ord` so permission checks can be written as
/// `user_role >= AccessRole::Write`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccessRole {
    /// Can clone, view content, and comment on PRs.
    Read = 0,
    /// Can commit, push, create PRs and branches.
    Write = 1,
    /// Can manage branches, merge PRs, configure actions.
    Admin = 2,
    /// Full control including access management.
    Owner = 3,
}

impl std::fmt::Display for AccessRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read => write!(f, "read"),
            Self::Write => write!(f, "write"),
            Self::Admin => write!(f, "admin"),
            Self::Owner => write!(f, "owner"),
        }
    }
}

impl AccessRole {
    /// Parses a role from a string (case-insensitive).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "read" => Some(Self::Read),
            "write" => Some(Self::Write),
            "admin" => Some(Self::Admin),
            "owner" => Some(Self::Owner),
            _ => None,
        }
    }
}

/// Access record for a single user in the repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAccess {
    /// Key fingerprint (`SHA256:<base64>`) identifying the user.
    pub fingerprint: String,
    /// Role assigned to this user.
    pub role: AccessRole,
    /// Cached identity from the user's public key (display hint).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<KeyIdentity>,
    /// Ed25519 verifying key bytes (32 bytes). Stored so the server can
    /// verify signatures without needing the `.pub` file on disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_public_key: Option<Vec<u8>>,
    /// ISO 8601 timestamp when access was granted.
    pub added_at: String,
    /// Fingerprint of the user who granted this access.
    pub added_by: String,
}

/// Branch protection rules governing direct pushes and merges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchProtection {
    /// Number of approved reviews required before merging.
    #[serde(default)]
    pub required_approvals: u32,
    /// Whether CI checks must pass before merging.
    #[serde(default)]
    pub require_ci_pass: bool,
    /// Roles allowed to merge into this branch.
    #[serde(default = "default_merge_roles")]
    pub allowed_merge_roles: Vec<AccessRole>,
    /// Roles allowed to push directly to this branch.
    #[serde(default = "default_push_roles")]
    pub allowed_push_roles: Vec<AccessRole>,
}

fn default_merge_roles() -> Vec<AccessRole> {
    vec![AccessRole::Admin, AccessRole::Owner]
}

fn default_push_roles() -> Vec<AccessRole> {
    vec![AccessRole::Admin, AccessRole::Owner]
}

impl Default for BranchProtection {
    fn default() -> Self {
        Self {
            required_approvals: 0,
            require_ci_pass: false,
            allowed_merge_roles: default_merge_roles(),
            allowed_push_roles: default_push_roles(),
        }
    }
}

/// Repository-level access control list.
///
/// When `users` is empty, the repository operates in legacy mode with
/// no access enforcement (backward compatible with existing repos).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccessControl {
    /// Per-user access entries.
    #[serde(default)]
    pub users: Vec<UserAccess>,
    /// Per-branch protection rules (keyed by branch name, not full ref).
    #[serde(default)]
    pub branch_protection: BTreeMap<String, BranchProtection>,
}

/// Operations that can be checked against the access control list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// Read repository content.
    Read,
    /// Write content (commit, push, create branches/PRs).
    Write,
    /// Administrative operations (merge PRs, delete branches, configure actions).
    Admin,
    /// Manage access control (grant/revoke users, set roles).
    ManageAccess,
}

impl Permission {
    /// Returns the minimum role required for this permission.
    #[must_use]
    pub const fn min_role(self) -> AccessRole {
        match self {
            Self::Read => AccessRole::Read,
            Self::Write => AccessRole::Write,
            Self::Admin => AccessRole::Admin,
            Self::ManageAccess => AccessRole::Owner,
        }
    }
}

impl AccessControl {
    /// Returns `true` if the ACL is empty (legacy/unmanaged mode).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.users.is_empty()
    }

    /// Looks up the access record for a given fingerprint.
    #[must_use]
    pub fn user_for(&self, fingerprint: &str) -> Option<&UserAccess> {
        self.users.iter().find(|u| u.fingerprint == fingerprint)
    }

    /// Returns the role for a given fingerprint, if the user exists.
    #[must_use]
    pub fn role_for(&self, fingerprint: &str) -> Option<AccessRole> {
        self.user_for(fingerprint).map(|u| u.role)
    }

    /// Checks whether a user has a given permission.
    ///
    /// Returns `true` if:
    /// - The ACL is empty (legacy mode — all authenticated users have full access), or
    /// - The user's role meets or exceeds the permission's minimum role.
    #[must_use]
    pub fn can(&self, fingerprint: &str, permission: Permission) -> bool {
        if self.is_empty() {
            return true;
        }
        self.role_for(fingerprint)
            .is_some_and(|role| role >= permission.min_role())
    }

    /// Checks whether a user can push directly to a protected branch.
    ///
    /// Returns `true` if the branch has no protection rules or the user's
    /// role is in the allowed push roles list.
    #[must_use]
    pub fn can_push_to_branch(&self, fingerprint: &str, branch: &str) -> bool {
        if self.is_empty() {
            return true;
        }
        let Some(role) = self.role_for(fingerprint) else {
            return false;
        };
        self.branch_protection.get(branch).map_or_else(
            || role >= AccessRole::Write,
            |protection| protection.allowed_push_roles.contains(&role),
        )
    }

    /// Checks whether a user can merge into a protected branch.
    ///
    /// Returns `true` if the branch has no protection rules or the user's
    /// role is in the allowed merge roles list.
    #[must_use]
    pub fn can_merge_to_branch(&self, fingerprint: &str, branch: &str) -> bool {
        if self.is_empty() {
            return true;
        }
        let Some(role) = self.role_for(fingerprint) else {
            return false;
        };
        self.branch_protection.get(branch).map_or_else(
            || role >= AccessRole::Write,
            |protection| protection.allowed_merge_roles.contains(&role),
        )
    }

    /// Returns the branch protection rules for a branch, if any.
    #[must_use]
    pub fn branch_protection_for(&self, branch: &str) -> Option<&BranchProtection> {
        self.branch_protection.get(branch)
    }

    /// Merges another `AccessControl` into this one.
    ///
    /// Strategy:
    /// - New users from `other` are imported.
    /// - For users that exist in both, local Owner entries take priority
    ///   (prevents remote from stripping ownership). For non-Owner conflicts,
    ///   the entry with the later `added_at` timestamp wins.
    /// - Branch protection: local definitions take priority.
    pub fn merge_from(&mut self, other: &Self) {
        for remote_user in &other.users {
            if let Some(local_user) = self
                .users
                .iter_mut()
                .find(|u| u.fingerprint == remote_user.fingerprint)
            {
                // Local Owner entries always take priority.
                if local_user.role == AccessRole::Owner {
                    continue;
                }
                // For non-Owner conflicts, later timestamp wins.
                if remote_user.added_at > local_user.added_at {
                    *local_user = remote_user.clone();
                }
            } else {
                self.users.push(remote_user.clone());
            }
        }

        // Branch protection: local definitions take priority.
        for (branch, protection) in &other.branch_protection {
            self.branch_protection
                .entry(branch.clone())
                .or_insert_with(|| protection.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_user(fingerprint: &str, role: AccessRole) -> UserAccess {
        UserAccess {
            fingerprint: fingerprint.to_owned(),
            role,
            identity: None,
            signing_public_key: None,
            added_at: "2026-01-01T00:00:00Z".to_owned(),
            added_by: "SHA256:owner".to_owned(),
        }
    }

    #[test]
    fn empty_acl_allows_everything() {
        let acl = AccessControl::default();
        assert!(acl.can("any-fingerprint", Permission::ManageAccess));
        assert!(acl.can_push_to_branch("any", "main"));
        assert!(acl.can_merge_to_branch("any", "main"));
    }

    #[test]
    fn role_ordering() {
        assert!(AccessRole::Owner > AccessRole::Admin);
        assert!(AccessRole::Admin > AccessRole::Write);
        assert!(AccessRole::Write > AccessRole::Read);
    }

    #[test]
    fn permission_checks() {
        let mut acl = AccessControl::default();
        acl.users.push(make_user("reader", AccessRole::Read));
        acl.users.push(make_user("writer", AccessRole::Write));
        acl.users.push(make_user("admin", AccessRole::Admin));
        acl.users.push(make_user("owner", AccessRole::Owner));

        assert!(acl.can("reader", Permission::Read));
        assert!(!acl.can("reader", Permission::Write));

        assert!(acl.can("writer", Permission::Write));
        assert!(!acl.can("writer", Permission::Admin));

        assert!(acl.can("admin", Permission::Admin));
        assert!(!acl.can("admin", Permission::ManageAccess));

        assert!(acl.can("owner", Permission::ManageAccess));

        // Unknown user has no access.
        assert!(!acl.can("unknown", Permission::Read));
    }

    #[test]
    fn branch_protection_enforcement() {
        let mut acl = AccessControl::default();
        acl.users.push(make_user("writer", AccessRole::Write));
        acl.users.push(make_user("admin", AccessRole::Admin));

        acl.branch_protection.insert(
            "main".to_owned(),
            BranchProtection {
                required_approvals: 1,
                require_ci_pass: true,
                allowed_merge_roles: vec![AccessRole::Admin, AccessRole::Owner],
                allowed_push_roles: vec![AccessRole::Owner],
            },
        );

        // Writer can't push or merge to protected main.
        assert!(!acl.can_push_to_branch("writer", "main"));
        assert!(!acl.can_merge_to_branch("writer", "main"));

        // Admin can merge but not push.
        assert!(!acl.can_push_to_branch("admin", "main"));
        assert!(acl.can_merge_to_branch("admin", "main"));

        // Unprotected branch: writer can push.
        assert!(acl.can_push_to_branch("writer", "feature-x"));
    }

    #[test]
    fn merge_preserves_local_owner() {
        let mut local = AccessControl::default();
        local.users.push(make_user("alice", AccessRole::Owner));

        let mut remote = AccessControl::default();
        remote.users.push(UserAccess {
            fingerprint: "alice".to_owned(),
            role: AccessRole::Read,
            identity: None,
            signing_public_key: None,
            added_at: "2026-06-01T00:00:00Z".to_owned(),
            added_by: "SHA256:attacker".to_owned(),
        });
        remote.users.push(make_user("bob", AccessRole::Write));

        local.merge_from(&remote);

        // Alice stays Owner (local Owner always wins).
        assert_eq!(local.role_for("alice"), Some(AccessRole::Owner));
        // Bob was imported from remote.
        assert_eq!(local.role_for("bob"), Some(AccessRole::Write));
    }

    #[test]
    fn role_parse() {
        assert_eq!(AccessRole::parse("Read"), Some(AccessRole::Read));
        assert_eq!(AccessRole::parse("ADMIN"), Some(AccessRole::Admin));
        assert_eq!(AccessRole::parse("unknown"), None);
    }
}
