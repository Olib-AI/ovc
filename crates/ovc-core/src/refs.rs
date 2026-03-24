//! Reference store for branches, tags, and HEAD.
//!
//! [`RefStore`] manages named references (branches and tags), the HEAD pointer,
//! and a reflog that records the history of reference updates.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::id::ObjectId;
use crate::object::Identity;

/// Maximum depth for symbolic reference resolution to prevent infinite loops.
const MAX_SYMBOLIC_DEPTH: usize = 10;

/// Maximum allowed length for a reference name component (branch or tag short name).
const MAX_REF_NAME_LENGTH: usize = 256;

/// Validates a reference name (short name, without `refs/heads/` or `refs/tags/` prefix).
///
/// Rejects names that contain control characters, null bytes, path traversal
/// sequences, or characters that could break downstream tooling (git refspec
/// special characters). This provides defense-in-depth so that even callers
/// that bypass the API validation layer (e.g., CLI commands, git import) cannot
/// create refs with dangerous names.
fn validate_ref_name(name: &str) -> CoreResult<()> {
    if name.is_empty() {
        return Err(CoreError::FormatError {
            reason: "ref name must not be empty".into(),
        });
    }
    if name.len() > MAX_REF_NAME_LENGTH {
        return Err(CoreError::FormatError {
            reason: format!("ref name exceeds maximum length of {MAX_REF_NAME_LENGTH} characters"),
        });
    }
    if name.contains("..") {
        return Err(CoreError::FormatError {
            reason: "ref name must not contain '..'".into(),
        });
    }
    for ch in name.chars() {
        if ch.is_control() || ch == '\0' {
            return Err(CoreError::FormatError {
                reason: "ref name must not contain control characters or null bytes".into(),
            });
        }
        if matches!(ch, '\\' | '~' | '^' | ':' | '?' | '*' | '[' | ' ') {
            return Err(CoreError::FormatError {
                reason: format!("ref name must not contain '{ch}'"),
            });
        }
    }
    Ok(())
}

/// Maximum number of reflog entries retained per reference store.
///
/// The reflog is serialized into the superblock and persisted inside the
/// encrypted `.ovc` file. Without a cap, long-lived repositories accumulate
/// unbounded reflog entries, steadily inflating file size and memory usage.
/// 10 000 entries covers months of daily development and is sufficient for
/// any practical `reflog` inspection.
const MAX_REFLOG_ENTRIES: usize = 10_000;

/// The target of a reference: either a direct object id or a symbolic name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefTarget {
    /// Points directly to an object.
    Direct(ObjectId),
    /// Points to another reference by name (e.g., `"refs/heads/main"`).
    Symbolic(String),
}

/// A single entry in the reflog, recording a reference update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflogEntry {
    /// The reference that was updated.
    pub ref_name: String,
    /// The previous value (None for newly created refs).
    pub old_value: Option<ObjectId>,
    /// The new value.
    pub new_value: ObjectId,
    /// The identity of the person who made the change.
    pub identity: Identity,
    /// A human-readable message describing the change.
    pub message: String,
}

/// Manages named references (branches, tags), HEAD, and the reflog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefStore {
    /// Named references mapping full ref names to targets.
    refs: BTreeMap<String, RefTarget>,
    /// The HEAD pointer.
    head: RefTarget,
    /// Ordered log of reference changes.
    reflog: Vec<ReflogEntry>,
    /// Annotation messages for annotated tags, keyed by full ref name
    /// (`refs/tags/<name>`). Lightweight tags have no entry here.
    ///
    /// Serialized with a default of empty map so that repositories created
    /// before this field was introduced deserialize without error.
    #[serde(default)]
    tag_messages: BTreeMap<String, String>,
}

impl RefStore {
    /// Creates a new `RefStore` with HEAD pointing symbolically to the given default branch.
    #[must_use]
    pub fn new(default_branch: &str) -> Self {
        Self {
            refs: BTreeMap::new(),
            head: RefTarget::Symbolic(format!("refs/heads/{default_branch}")),
            reflog: Vec::new(),
            tag_messages: BTreeMap::new(),
        }
    }

    /// Returns the current HEAD target.
    #[must_use]
    pub const fn head(&self) -> &RefTarget {
        &self.head
    }

    /// Sets the HEAD to a new target.
    pub fn set_head(&mut self, target: RefTarget) {
        self.head = target;
    }

    /// Resolves HEAD to a direct `ObjectId`, following symbolic refs up to `MAX_SYMBOLIC_DEPTH`.
    ///
    /// Returns an error if HEAD is unresolvable (e.g., points to an unborn branch).
    pub fn resolve_head(&self) -> CoreResult<ObjectId> {
        self.resolve_target(&self.head, MAX_SYMBOLIC_DEPTH)
    }

    /// Resolves any reference name to a direct `ObjectId`.
    pub fn resolve(&self, name: &str) -> CoreResult<ObjectId> {
        let target = self.refs.get(name).ok_or_else(|| CoreError::FormatError {
            reason: format!("reference not found: {name}"),
        })?;
        self.resolve_target(target, MAX_SYMBOLIC_DEPTH)
    }

    /// Sets (or creates) a branch reference and records a reflog entry.
    ///
    /// The reflog is capped at [`MAX_REFLOG_ENTRIES`]. When the limit is
    /// reached, the oldest entries are discarded to make room.
    ///
    /// Returns an error if the branch name contains invalid characters.
    pub fn set_branch(
        &mut self,
        name: &str,
        oid: ObjectId,
        identity: &Identity,
        message: &str,
    ) -> CoreResult<()> {
        // Validate the short name (strip prefix if already qualified).
        let short = name.strip_prefix("refs/heads/").unwrap_or(name);
        validate_ref_name(short)?;

        let full_name = Self::branch_ref_name(name);
        let old_value = self.resolve_ref_direct(&full_name);

        self.refs.insert(full_name.clone(), RefTarget::Direct(oid));

        self.reflog.push(ReflogEntry {
            ref_name: full_name,
            old_value,
            new_value: oid,
            identity: identity.clone(),
            message: message.to_owned(),
        });

        // Trim oldest entries when the reflog exceeds the cap.
        if self.reflog.len() > MAX_REFLOG_ENTRIES {
            let excess = self.reflog.len() - MAX_REFLOG_ENTRIES;
            self.reflog.drain(..excess);
        }

        Ok(())
    }

    /// Deletes a branch reference.
    pub fn delete_branch(&mut self, name: &str) -> CoreResult<()> {
        let full_name = Self::branch_ref_name(name);
        self.refs
            .remove(&full_name)
            .ok_or_else(|| CoreError::FormatError {
                reason: format!("branch not found: {name}"),
            })?;
        Ok(())
    }

    /// Renames a branch from `old_name` to `new_name`.
    ///
    /// Copies the target `ObjectId` from the old ref to the new ref, removes
    /// the old ref, and updates HEAD if it was a symbolic ref pointing to the
    /// old branch. Returns an error if `old_name` does not exist or `new_name`
    /// already exists.
    pub fn rename_branch(
        &mut self,
        old_name: &str,
        new_name: &str,
        identity: &Identity,
    ) -> CoreResult<()> {
        validate_ref_name(new_name.strip_prefix("refs/heads/").unwrap_or(new_name))?;

        let old_full = Self::branch_ref_name(old_name);
        let new_full = Self::branch_ref_name(new_name);

        if self.refs.contains_key(&new_full) {
            return Err(CoreError::AlreadyExists { path: new_full });
        }

        let target = self
            .refs
            .remove(&old_full)
            .ok_or_else(|| CoreError::FormatError {
                reason: format!("branch not found: {old_name}"),
            })?;

        let oid = match &target {
            RefTarget::Direct(oid) => *oid,
            RefTarget::Symbolic(_) => {
                return Err(CoreError::FormatError {
                    reason: format!("branch '{old_name}' is a symbolic ref; cannot rename"),
                });
            }
        };

        self.refs.insert(new_full.clone(), target);

        self.reflog.push(ReflogEntry {
            ref_name: new_full.clone(),
            old_value: None,
            new_value: oid,
            identity: identity.clone(),
            message: format!("branch: renamed {old_name} to {new_name}"),
        });

        if self.reflog.len() > MAX_REFLOG_ENTRIES {
            let excess = self.reflog.len() - MAX_REFLOG_ENTRIES;
            self.reflog.drain(..excess);
        }

        // Update HEAD if it was pointing at the old branch name.
        if self.head == RefTarget::Symbolic(old_full) {
            self.head = RefTarget::Symbolic(new_full);
        }

        Ok(())
    }

    /// Lists all branches as `(short_name, oid)` pairs.
    #[must_use]
    pub fn list_branches(&self) -> Vec<(&str, &ObjectId)> {
        self.refs
            .iter()
            .filter_map(|(name, target)| {
                let short = name.strip_prefix("refs/heads/")?;
                if let RefTarget::Direct(oid) = target {
                    Some((short, oid))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Lists all tags as `(short_name, oid, optional_message)` triples.
    ///
    /// Lightweight tags have `None` for the message field. Annotated tags
    /// (created with a non-empty message via [`create_tag`]) carry their
    /// message in the third element.
    #[must_use]
    pub fn list_tags(&self) -> Vec<(&str, &ObjectId, Option<&str>)> {
        self.refs
            .iter()
            .filter_map(|(name, target)| {
                let short = name.strip_prefix("refs/tags/")?;
                if let RefTarget::Direct(oid) = target {
                    let msg = self.tag_messages.get(name).map(String::as_str);
                    Some((short, oid, msg))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Creates a tag pointing to the given object.
    ///
    /// When `message` is `Some`, the tag is treated as an annotated tag and
    /// the message is persisted alongside the ref. When `message` is `None`
    /// the tag is a lightweight ref with no annotation.
    ///
    /// Returns an error if the tag name contains invalid characters or the
    /// tag already exists.
    pub fn create_tag(
        &mut self,
        name: &str,
        oid: ObjectId,
        message: Option<&str>,
    ) -> CoreResult<()> {
        validate_ref_name(name)?;
        let full_name = format!("refs/tags/{name}");
        if self.refs.contains_key(&full_name) {
            return Err(CoreError::AlreadyExists { path: full_name });
        }
        self.refs.insert(full_name.clone(), RefTarget::Direct(oid));
        if let Some(msg) = message
            && !msg.is_empty()
        {
            self.tag_messages.insert(full_name, msg.to_owned());
        }
        Ok(())
    }

    /// Deletes a tag and its annotation message (if any).
    pub fn delete_tag(&mut self, name: &str) -> CoreResult<()> {
        let full_name = format!("refs/tags/{name}");
        self.refs
            .remove(&full_name)
            .ok_or_else(|| CoreError::FormatError {
                reason: format!("tag not found: {name}"),
            })?;
        // Remove annotation message if present; a lightweight tag has no entry
        // here, so this is a no-op for those.
        self.tag_messages.remove(&full_name);
        Ok(())
    }

    /// Returns reflog entries for the given reference name.
    #[must_use]
    pub fn get_reflog(&self, ref_name: &str) -> Vec<&ReflogEntry> {
        self.reflog
            .iter()
            .filter(|entry| entry.ref_name == ref_name)
            .collect()
    }

    /// Returns all reflog entries grouped by reference name.
    ///
    /// Used by GC to include reflog-referenced objects as roots, preventing
    /// premature collection of force-pushed or amended commits.
    #[must_use]
    pub fn all_reflog_entries(&self) -> BTreeMap<&str, Vec<&ReflogEntry>> {
        let mut groups: BTreeMap<&str, Vec<&ReflogEntry>> = BTreeMap::new();
        for entry in &self.reflog {
            groups.entry(&entry.ref_name).or_default().push(entry);
        }
        groups
    }

    // ── Internal helpers ──────────────────────────────────────────────

    /// Converts a short branch name to its full ref path.
    fn branch_ref_name(name: &str) -> String {
        if name.starts_with("refs/heads/") {
            name.to_owned()
        } else {
            format!("refs/heads/{name}")
        }
    }

    /// Resolves a `RefTarget` to a direct `ObjectId`, following symbolic refs.
    fn resolve_target(&self, target: &RefTarget, depth: usize) -> CoreResult<ObjectId> {
        if depth == 0 {
            return Err(CoreError::FormatError {
                reason: "maximum symbolic reference depth exceeded".into(),
            });
        }
        match target {
            RefTarget::Direct(oid) => Ok(*oid),
            RefTarget::Symbolic(name) => {
                let next = self.refs.get(name).ok_or_else(|| CoreError::FormatError {
                    reason: format!("symbolic reference target not found: {name}"),
                })?;
                self.resolve_target(next, depth - 1)
            }
        }
    }

    /// Tries to resolve a full ref name to a direct `ObjectId` without error.
    fn resolve_ref_direct(&self, full_name: &str) -> Option<ObjectId> {
        self.refs.get(full_name).and_then(|t| match t {
            RefTarget::Direct(oid) => Some(*oid),
            RefTarget::Symbolic(_) => None,
        })
    }
}

impl Default for RefStore {
    fn default() -> Self {
        Self::new("main")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_identity() -> Identity {
        Identity {
            name: "Test User".into(),
            email: "test@example.com".into(),
            timestamp: 1_700_000_000,
            tz_offset_minutes: 0,
        }
    }

    #[test]
    fn new_head_is_symbolic_to_default_branch() {
        let store = RefStore::new("main");
        assert_eq!(store.head(), &RefTarget::Symbolic("refs/heads/main".into()));
    }

    #[test]
    fn set_and_resolve_branch() {
        let mut store = RefStore::new("main");
        let oid = crate::id::hash_blob(b"test");
        let id = test_identity();
        store.set_branch("main", oid, &id, "initial").unwrap();

        let resolved = store.resolve("refs/heads/main").unwrap();
        assert_eq!(resolved, oid);
    }

    #[test]
    fn resolve_head_through_symbolic() {
        let mut store = RefStore::new("main");
        let oid = crate::id::hash_blob(b"test");
        let id = test_identity();
        store.set_branch("main", oid, &id, "initial").unwrap();

        let resolved = store.resolve_head().unwrap();
        assert_eq!(resolved, oid);
    }

    #[test]
    fn resolve_head_unborn_branch_fails() {
        let store = RefStore::new("main");
        assert!(store.resolve_head().is_err());
    }

    #[test]
    fn delete_branch() {
        let mut store = RefStore::new("main");
        let oid = crate::id::hash_blob(b"test");
        let id = test_identity();
        store
            .set_branch("feature", oid, &id, "create feature")
            .unwrap();

        assert!(store.delete_branch("feature").is_ok());
        assert!(store.resolve("refs/heads/feature").is_err());
    }

    #[test]
    fn delete_nonexistent_branch_fails() {
        let mut store = RefStore::new("main");
        assert!(store.delete_branch("nonexistent").is_err());
    }

    #[test]
    fn list_branches() {
        let mut store = RefStore::new("main");
        let id = test_identity();
        let oid1 = crate::id::hash_blob(b"a");
        let oid2 = crate::id::hash_blob(b"b");
        store.set_branch("main", oid1, &id, "m").unwrap();
        store.set_branch("feature", oid2, &id, "f").unwrap();

        let branches = store.list_branches();
        assert_eq!(branches.len(), 2);
    }

    #[test]
    fn create_and_delete_tag() {
        let mut store = RefStore::new("main");
        let oid = crate::id::hash_blob(b"v1");
        store.create_tag("v1.0", oid, None).unwrap();

        let tags = store.list_tags();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].0, "v1.0");
        assert_eq!(*tags[0].1, oid);
        assert_eq!(tags[0].2, None);

        store.delete_tag("v1.0").unwrap();
        assert!(store.list_tags().is_empty());
    }

    #[test]
    fn annotated_tag_message_round_trip() {
        let mut store = RefStore::new("main");
        let oid = crate::id::hash_blob(b"v2");
        store.create_tag("v2.0", oid, Some("Release v2.0")).unwrap();

        let tags = store.list_tags();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].2, Some("Release v2.0"));

        // Message is removed when the tag is deleted.
        store.delete_tag("v2.0").unwrap();
        assert!(store.list_tags().is_empty());
        // Confirm internal map is also cleared (no orphaned entries).
        assert!(store.tag_messages.is_empty());
    }

    #[test]
    fn duplicate_tag_fails() {
        let mut store = RefStore::new("main");
        let oid = crate::id::hash_blob(b"v1");
        store.create_tag("v1.0", oid, None).unwrap();
        assert!(store.create_tag("v1.0", oid, None).is_err());
    }

    #[test]
    fn reflog_records_updates() {
        let mut store = RefStore::new("main");
        let id = test_identity();
        let oid1 = crate::id::hash_blob(b"first");
        let oid2 = crate::id::hash_blob(b"second");

        store.set_branch("main", oid1, &id, "first commit").unwrap();
        store
            .set_branch("main", oid2, &id, "second commit")
            .unwrap();

        let log = store.get_reflog("refs/heads/main");
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].old_value, None);
        assert_eq!(log[0].new_value, oid1);
        assert_eq!(log[1].old_value, Some(oid1));
        assert_eq!(log[1].new_value, oid2);
    }

    #[test]
    fn serde_round_trip() {
        let mut store = RefStore::new("develop");
        let id = test_identity();
        let oid = crate::id::hash_blob(b"data");
        store.set_branch("develop", oid, &id, "init").unwrap();
        store
            .create_tag("v0.1", oid, Some("First release"))
            .unwrap();

        let json = serde_json::to_string(&store).unwrap();
        let restored: RefStore = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.resolve_head().unwrap(), oid);
        let tags = restored.list_tags();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].2, Some("First release"));
    }

    #[test]
    fn serde_backward_compat_missing_tag_messages() {
        // Simulate a JSON payload from an older version of OVC that has no
        // `tag_messages` field. The `#[serde(default)]` annotation must make
        // this deserialize to an empty map without error.
        let json = r#"{
            "refs": { "refs/tags/v0.1": { "Direct": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" } },
            "head": { "Symbolic": "refs/heads/main" },
            "reflog": []
        }"#;
        let restored: RefStore = serde_json::from_str(json).unwrap();
        let tags = restored.list_tags();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].2, None);
    }
}
