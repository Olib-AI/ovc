//! Working directory operations for OVC repositories.
//!
//! [`WorkDir`] provides operations for scanning, reading, and writing files
//! in the working directory, as well as computing file status relative to
//! the index and HEAD tree.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::error::{CoreError, CoreResult};
use crate::id;
use crate::ignore::IgnoreRules;
use crate::index::Index;
use crate::object::FileMode;
use crate::store::ObjectStore;

/// Status of a file relative to a reference (index or HEAD).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    /// The file has not been modified.
    Unmodified,
    /// The file has been modified.
    Modified,
    /// The file is newly added.
    Added,
    /// The file has been deleted.
    Deleted,
    /// The file is not tracked.
    Untracked,
    /// The file is ignored by ignore rules.
    Ignored,
}

/// A file's status in both the staging area and working directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    /// The file path relative to the repository root.
    pub path: String,
    /// Status relative to HEAD (staged changes).
    pub staged: FileStatus,
    /// Status relative to the index (unstaged changes).
    pub unstaged: FileStatus,
}

/// An entry discovered during a working directory scan.
#[derive(Debug, Clone)]
pub struct WorkDirEntry {
    /// Path relative to the working directory root (forward-slash separated).
    pub path: String,
    /// File size in bytes.
    pub size: u64,
    /// Whether the file is executable.
    pub is_executable: bool,
    /// Last modification time (seconds since epoch).
    pub mtime_secs: i64,
    /// Last modification time (nanosecond component).
    pub mtime_nanos: u32,
}

/// Handle for working directory operations.
#[derive(Debug, Clone)]
pub struct WorkDir {
    root: PathBuf,
}

impl WorkDir {
    /// Creates a new `WorkDir` handle for the given root directory.
    #[must_use]
    pub const fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Returns the root path of the working directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Scans the working directory for all non-ignored files.
    pub fn scan_files(&self, ignore: &IgnoreRules) -> CoreResult<Vec<WorkDirEntry>> {
        let mut entries = Vec::new();
        scan_recursive(&self.root, "", ignore, &mut entries)?;
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }

    /// Validates that the resolved path stays within the working directory root.
    ///
    /// Checks for path traversal components (`..`) to prevent escaping the
    /// root directory. Uses lexical analysis rather than `canonicalize` to
    /// avoid TOCTOU races and to work with paths that do not yet exist on disk.
    fn validate_path(&self, path: &str) -> CoreResult<PathBuf> {
        let full_path = self.root.join(path);

        // Reject any path component that is `..` to prevent traversal.
        for component in full_path.components() {
            if component == std::path::Component::ParentDir {
                return Err(CoreError::FormatError {
                    reason: format!(
                        "path traversal detected: '{path}' escapes working directory root"
                    ),
                });
            }
        }

        // As a secondary defense, verify the joined path starts with root
        // after stripping away current-dir components via `components()`.
        let normalized: PathBuf = full_path.components().collect();
        let normalized_root: PathBuf = self.root.components().collect();
        if !normalized.starts_with(&normalized_root) {
            return Err(CoreError::FormatError {
                reason: format!("path traversal detected: '{path}' escapes working directory root"),
            });
        }

        Ok(full_path)
    }

    /// Reads a file's contents from the working directory.
    pub fn read_file(&self, path: &str) -> CoreResult<Vec<u8>> {
        let full_path = self.validate_path(path)?;
        std::fs::read(&full_path).map_err(CoreError::from)
    }

    /// Writes content to a file in the working directory, creating parent directories.
    pub fn write_file(&self, path: &str, content: &[u8], _mode: FileMode) -> CoreResult<()> {
        let full_path = self.validate_path(path)?;
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&full_path, content).map_err(CoreError::from)
    }

    /// Deletes a file from the working directory.
    pub fn delete_file(&self, path: &str) -> CoreResult<()> {
        let full_path = self.validate_path(path)?;
        if full_path.exists() {
            std::fs::remove_file(&full_path)?;
        }
        Ok(())
    }

    /// Computes the status of all tracked and untracked files.
    pub fn compute_status(
        &self,
        index: &Index,
        head_tree: Option<&crate::id::ObjectId>,
        store: &ObjectStore,
        ignore: &IgnoreRules,
    ) -> CoreResult<Vec<StatusEntry>> {
        let mut status_map = BTreeMap::<String, StatusEntry>::new();

        let head_entries = build_head_entries(head_tree, store)?;

        // Compare index vs HEAD (staged changes).
        for entry in index.entries() {
            let staged = head_entries
                .get(&entry.path)
                .map_or(FileStatus::Added, |head_oid| {
                    if *head_oid == entry.oid {
                        FileStatus::Unmodified
                    } else {
                        FileStatus::Modified
                    }
                });

            status_map.insert(
                entry.path.clone(),
                StatusEntry {
                    path: entry.path.clone(),
                    staged,
                    unstaged: FileStatus::Unmodified,
                },
            );
        }

        // Check for files deleted from index that were in HEAD.
        for path in head_entries.keys() {
            if index.get_entry(path).is_none() {
                status_map.insert(
                    path.clone(),
                    StatusEntry {
                        path: path.clone(),
                        staged: FileStatus::Deleted,
                        unstaged: FileStatus::Unmodified,
                    },
                );
            }
        }

        // Scan working directory and compare against index (unstaged changes).
        let workdir_files = self.scan_files(ignore)?;
        let workdir_paths: BTreeSet<String> =
            workdir_files.iter().map(|e| e.path.clone()).collect();

        for wf in &workdir_files {
            if let Some(index_entry) = index.get_entry(&wf.path) {
                // Fast path: if the file's mtime and size match the index entry,
                // skip the expensive content read + hash. This mirrors git's stat
                // cache optimisation and dramatically reduces I/O on large repos.
                let stat_match = !index_entry.flags.assume_unchanged
                    && wf.size == index_entry.file_size
                    && wf.mtime_secs == index_entry.mtime_secs
                    && wf.mtime_nanos == index_entry.mtime_nanos;

                if index_entry.flags.assume_unchanged || stat_match {
                    // Treat as unmodified — skip content hash.
                    continue;
                }

                let content = self.read_file(&wf.path)?;
                let disk_oid = id::hash_blob(&content);

                if disk_oid != index_entry.oid
                    && let Some(status) = status_map.get_mut(&wf.path)
                {
                    status.unstaged = FileStatus::Modified;
                }
            } else if !ignore.is_ignored(&wf.path) {
                status_map
                    .entry(wf.path.clone())
                    .or_insert_with(|| StatusEntry {
                        path: wf.path.clone(),
                        staged: FileStatus::Untracked,
                        unstaged: FileStatus::Untracked,
                    });
            }
        }

        // Check for files in index but not on disk (unstaged deletion).
        for entry in index.entries() {
            if !workdir_paths.contains(&entry.path)
                && let Some(status) = status_map.get_mut(&entry.path)
            {
                status.unstaged = FileStatus::Deleted;
            }
        }

        Ok(status_map.into_values().collect())
    }
}

/// Builds a map of path to `ObjectId` from a HEAD tree, if present.
fn build_head_entries(
    head_tree: Option<&crate::id::ObjectId>,
    store: &ObjectStore,
) -> CoreResult<BTreeMap<String, crate::id::ObjectId>> {
    let Some(tree_oid) = head_tree else {
        return Ok(BTreeMap::new());
    };
    let mut head_index = Index::new();
    head_index.read_tree(tree_oid, store)?;
    Ok(head_index
        .entries()
        .iter()
        .map(|e| (e.path.clone(), e.oid))
        .collect())
}

/// Recursively scans a directory, collecting non-ignored file entries.
fn scan_recursive(
    dir: &Path,
    prefix: &str,
    ignore: &IgnoreRules,
    entries: &mut Vec<WorkDirEntry>,
) -> CoreResult<()> {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(CoreError::from(e)),
    };

    for dir_entry in read_dir {
        let dir_entry = dir_entry?;
        let file_name = dir_entry.file_name();
        let name = file_name.to_string_lossy();

        let rel_path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };

        if ignore.is_ignored(&rel_path) {
            continue;
        }

        let file_type = dir_entry.file_type()?;
        // Skip symlinks entirely to prevent symlink-following attacks
        // (e.g., a symlink pointing outside the repo, or circular
        // directory symlinks causing infinite recursion).
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            scan_recursive(&dir_entry.path(), &rel_path, ignore, entries)?;
        } else if file_type.is_file() {
            let metadata = dir_entry.metadata()?;
            let (mtime_secs, mtime_nanos) = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or((0, 0), |d| {
                    (
                        i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
                        d.subsec_nanos(),
                    )
                });
            entries.push(WorkDirEntry {
                path: rel_path,
                size: metadata.len(),
                is_executable: is_executable(&metadata),
                mtime_secs,
                mtime_nanos,
            });
        }
    }

    Ok(())
}

/// Checks if a file is executable (Unix only; always false on other platforms).
#[cfg(unix)]
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &std::fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_files_with_ignore() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::write(root.join("file.txt"), b"hello").unwrap();
        std::fs::write(root.join("debug.log"), b"log").unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), b"fn main() {}").unwrap();

        std::fs::write(root.join(".ovcignore"), "*.log\n").unwrap();

        let ignore = IgnoreRules::load(root);
        let workdir = WorkDir::new(root.to_path_buf());
        let entries = workdir.scan_files(&ignore).unwrap();

        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"file.txt"));
        assert!(paths.contains(&"src/main.rs"));
        assert!(!paths.iter().any(|p| {
            std::path::Path::new(p)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("log"))
        }));
    }

    #[test]
    fn read_and_write_file() {
        let dir = tempfile::tempdir().unwrap();
        let workdir = WorkDir::new(dir.path().to_path_buf());

        workdir
            .write_file("subdir/test.txt", b"content", FileMode::Regular)
            .unwrap();

        let content = workdir.read_file("subdir/test.txt").unwrap();
        assert_eq!(content, b"content");
    }

    #[test]
    fn delete_file_test() {
        let dir = tempfile::tempdir().unwrap();
        let workdir = WorkDir::new(dir.path().to_path_buf());

        workdir
            .write_file("to_delete.txt", b"bye", FileMode::Regular)
            .unwrap();
        workdir.delete_file("to_delete.txt").unwrap();

        assert!(workdir.read_file("to_delete.txt").is_err());
    }

    #[test]
    fn compute_status_untracked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("untracked.txt"), b"new file").unwrap();

        let workdir = WorkDir::new(root.to_path_buf());
        let index = Index::new();
        let store = ObjectStore::default();
        let ignore = IgnoreRules::empty();

        let status = workdir
            .compute_status(&index, None, &store, &ignore)
            .unwrap();

        assert!(!status.is_empty());
        let untracked_entry = status.iter().find(|s| s.path == "untracked.txt").unwrap();
        assert_eq!(untracked_entry.staged, FileStatus::Untracked);
    }
}
