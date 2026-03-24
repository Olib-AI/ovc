//! Submodule management for nested OVC repositories.
//!
//! A submodule is another `.ovc` file tracked by the parent repository.
//! Configuration is stored in the superblock as a map of submodule names
//! to [`SubmoduleConfig`] entries.
//!
//! # Lifecycle
//!
//! Submodules are currently **config-only**: adding a submodule stores its
//! configuration in the parent repository but does not clone or initialise the
//! nested repository. The [`SubmoduleStatus::Configured`] variant reflects
//! this state and is surfaced through the API so the UI can inform users that
//! manual initialisation is required.

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};

/// Lifecycle status of a submodule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SubmoduleStatus {
    /// The submodule is recorded in the configuration but has not been
    /// initialised (no nested `.ovc` file exists at `path`).
    #[default]
    Configured,
}

impl SubmoduleStatus {
    /// Returns the string representation used in API responses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Configured => "configured",
        }
    }
}

/// Configuration for a single submodule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmoduleConfig {
    /// Relative path where the submodule is checked out.
    ///
    /// Must be a forward-slash delimited relative path with no `..` components
    /// and no leading `/`. Validated by [`SubmoduleConfig::validate`].
    pub path: String,
    /// Remote URL or local path of the submodule source.
    ///
    /// Must be non-empty and either resemble a URL (contains `://` or starts
    /// with a recognised scheme) or be an absolute filesystem path.
    /// Validated by [`SubmoduleConfig::validate`].
    pub url: String,
    /// Name of the `.ovc` file for this submodule.
    pub ovc_file: String,
    /// Pinned sequence number of the submodule commit.
    pub pinned_sequence: u64,
    /// Lifecycle status of this submodule entry.
    #[serde(default)]
    pub status: SubmoduleStatus,
}

impl SubmoduleConfig {
    /// Validates that the `path` and `url` fields meet invariants required for
    /// safe, correct operation.
    ///
    /// # Path rules
    /// - Must not be empty.
    /// - Must not be absolute (no leading `/`).
    /// - Must not contain `..` components (prevents workdir escape).
    /// - Must not contain `\` (Windows-style separators are rejected for
    ///   cross-platform consistency; callers should normalise separators).
    ///
    /// # URL rules
    /// - Must not be empty.
    /// - Must either contain `://` (URL scheme), start with `/` or `~/`
    ///   (absolute/home-relative path), or start with `.` (relative path).
    ///   This rejects obviously invalid values like plain alphanumeric strings
    ///   while remaining permissive for the range of valid inputs.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidInput`] with a descriptive message if any
    /// validation rule is violated.
    pub fn validate(&self) -> CoreResult<()> {
        // ── Path validation ──────────────────────────────────────────────────
        if self.path.is_empty() {
            return Err(CoreError::Config {
                reason: "submodule path must not be empty".into(),
            });
        }
        if self.path.starts_with('/') {
            return Err(CoreError::Config {
                reason: "submodule path must be relative, not absolute".into(),
            });
        }
        if self.path.contains('\\') {
            return Err(CoreError::Config {
                reason: "submodule path must use forward slashes, not backslashes".into(),
            });
        }
        // Reject any `..` component regardless of surrounding separators.
        for component in self.path.split('/') {
            if component == ".." {
                return Err(CoreError::Config {
                    reason: "submodule path must not contain '..' components".into(),
                });
            }
        }

        // ── URL validation ───────────────────────────────────────────────────
        if self.url.is_empty() {
            return Err(CoreError::Config {
                reason: "submodule url must not be empty".into(),
            });
        }
        // Accept URLs with a scheme, absolute paths, home-relative paths, and
        // relative paths. Reject anything else (e.g. bare words like "foo").
        let url_looks_valid = self.url.contains("://")
            || self.url.starts_with('/')
            || self.url.starts_with("~/")
            || self.url.starts_with("./")
            || self.url.starts_with("../");
        if !url_looks_valid {
            return Err(CoreError::Config {
                reason: format!(
                    "submodule url '{}' does not look like a valid URL or filesystem path; \
                     expected a scheme (e.g. https://), an absolute path (/…), or a relative \
                     path (./…)",
                    self.url
                ),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> SubmoduleConfig {
        SubmoduleConfig {
            path: "vendor/lib".into(),
            url: "https://example.com/lib.ovc".into(),
            ovc_file: "lib.ovc".into(),
            pinned_sequence: 0,
            status: SubmoduleStatus::Configured,
        }
    }

    #[test]
    fn valid_config_passes() {
        assert!(base_config().validate().is_ok());
    }

    #[test]
    fn empty_path_rejected() {
        let mut c = base_config();
        c.path = String::new();
        assert!(c.validate().is_err());
    }

    #[test]
    fn absolute_path_rejected() {
        let mut c = base_config();
        c.path = "/etc/passwd".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn dotdot_path_rejected() {
        let mut c = base_config();
        c.path = "vendor/../../../etc/passwd".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn backslash_path_rejected() {
        let mut c = base_config();
        c.path = "vendor\\lib".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn empty_url_rejected() {
        let mut c = base_config();
        c.url = String::new();
        assert!(c.validate().is_err());
    }

    #[test]
    fn bare_word_url_rejected() {
        let mut c = base_config();
        c.url = "notaurl".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn absolute_path_url_accepted() {
        let mut c = base_config();
        c.url = "/home/user/repos/lib.ovc".into();
        assert!(c.validate().is_ok());
    }

    #[test]
    fn relative_path_url_accepted() {
        let mut c = base_config();
        c.url = "./sibling.ovc".into();
        assert!(c.validate().is_ok());
    }

    #[test]
    fn status_defaults_to_configured() {
        let status = SubmoduleStatus::default();
        assert_eq!(status, SubmoduleStatus::Configured);
        assert_eq!(status.as_str(), "configured");
    }
}
