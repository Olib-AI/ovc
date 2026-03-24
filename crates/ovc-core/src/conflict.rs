//! Conflict detection for concurrent modifications and iCloud sync.
//!
//! Detects when the `.ovc` file has been modified externally (e.g., by
//! another process or iCloud sync) since it was last loaded.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::error::{CoreError, CoreResult};
use crate::format::{FileTrailer, TRAILER_SIZE};

/// Snapshot of file state at open time, used for conflict detection.
#[derive(Debug, Clone)]
pub struct FileSnapshot {
    /// The `file_sequence` when we opened the file.
    pub sequence_at_open: u64,
    /// The file size when we opened it.
    pub size_at_open: u64,
    /// The modification time when we opened it (seconds since epoch).
    pub mtime_at_open: u64,
}

impl FileSnapshot {
    /// Captures the current state of an `.ovc` file.
    pub fn capture(path: &Path, file_sequence: u64) -> CoreResult<Self> {
        let metadata = std::fs::metadata(path)?;
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());

        Ok(Self {
            sequence_at_open: file_sequence,
            size_at_open: metadata.len(),
            mtime_at_open: mtime,
        })
    }

    /// Checks if the file on disk has been modified since we captured the snapshot.
    ///
    /// Returns `Ok(())` if the file has not changed, or a [`CoreError::ConflictDetected`]
    /// error describing the conflict.
    pub fn check_for_conflict(&self, path: &Path) -> CoreResult<()> {
        let metadata = std::fs::metadata(path)?;
        let current_mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());

        // Quick check: did mtime or size change?
        if current_mtime != self.mtime_at_open || metadata.len() != self.size_at_open {
            // Read the trailer to check file_sequence.
            let current_sequence = read_file_sequence(path)?;
            if current_sequence != self.sequence_at_open {
                return Err(CoreError::ConflictDetected {
                    reason: format!(
                        "repository file was modified externally (sequence {} -> {}). \
                         Another process or iCloud sync may have updated the file. \
                         Use 'ovc pull' to merge remote changes, or 'ovc stash push' \
                         to save your work before re-opening.",
                        self.sequence_at_open, current_sequence
                    ),
                });
            }
        }

        Ok(())
    }
}

/// Reads just the `file_sequence` from a `.ovc` file's trailer without decrypting.
///
/// This reads only the last 32 bytes of the file and parses the trailer,
/// making it very fast for conflict detection.
pub fn read_file_sequence(path: &Path) -> CoreResult<u64> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path)?;
    let file_size = file.seek(SeekFrom::End(0))?;

    let trailer_size_u64 = u64::try_from(TRAILER_SIZE).expect("TRAILER_SIZE fits in u64");
    if file_size < trailer_size_u64 {
        return Err(CoreError::FormatError {
            reason: "file too small for trailer".into(),
        });
    }

    file.seek(SeekFrom::End(
        -i64::try_from(TRAILER_SIZE).expect("TRAILER_SIZE fits in i64"),
    ))?;
    let mut trailer_bytes = [0u8; TRAILER_SIZE];
    file.read_exact(&mut trailer_bytes)?;
    let trailer = FileTrailer::deserialize(&trailer_bytes)?;

    Ok(trailer.file_sequence)
}

/// Scans for iCloud conflict copies of the given `.ovc` file.
///
/// macOS iCloud creates files like `"filename (conflicted copy YYYY-MM-DD).ovc"`
/// when two devices modify the same file. This function finds them.
#[must_use]
pub fn find_icloud_conflicts(ovc_path: &Path) -> Vec<PathBuf> {
    let Some(parent) = ovc_path.parent() else {
        return Vec::new();
    };
    let Some(stem) = ovc_path.file_stem().and_then(|s| s.to_str()) else {
        return Vec::new();
    };

    let own_name = ovc_path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    let mut conflicts = Vec::new();

    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name_str) = name.to_str() else {
                continue;
            };

            // Match iCloud conflict patterns:
            //   "name (conflicted copy DATE).ovc"
            //   "name (conflict from DEVICE).ovc"
            if name_str.starts_with(stem)
                && name_str != own_name
                && (name_str.contains("conflicted") || name_str.contains("conflict"))
                && Path::new(name_str)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("ovc"))
            {
                conflicts.push(entry.path());
            }
        }
    }

    conflicts.sort();
    conflicts
}

/// Represents a detected conflict between the local repo and an external modification.
#[derive(Debug)]
pub struct ConflictInfo {
    /// The local `file_sequence` when we opened.
    pub local_sequence: u64,
    /// The remote/current `file_sequence` on disk.
    pub remote_sequence: u64,
    /// Any iCloud conflict copies found.
    pub icloud_conflicts: Vec<PathBuf>,
    /// Human-readable description.
    pub description: String,
}

/// Performs a full conflict check: sequence comparison plus iCloud conflict scan.
pub fn detect_conflicts(
    ovc_path: &Path,
    snapshot: &FileSnapshot,
) -> CoreResult<Option<ConflictInfo>> {
    let current_sequence = read_file_sequence(ovc_path)?;
    let icloud_conflicts = find_icloud_conflicts(ovc_path);

    if current_sequence != snapshot.sequence_at_open || !icloud_conflicts.is_empty() {
        let mut desc = String::new();
        if current_sequence != snapshot.sequence_at_open {
            let _ = write!(
                desc,
                "File modified externally (sequence {} -> {}). ",
                snapshot.sequence_at_open, current_sequence
            );
        }
        if !icloud_conflicts.is_empty() {
            let _ = write!(
                desc,
                "Found {} iCloud conflict file(s): {}",
                icloud_conflicts.len(),
                icloud_conflicts
                    .iter()
                    .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        Ok(Some(ConflictInfo {
            local_sequence: snapshot.sequence_at_open,
            remote_sequence: current_sequence,
            icloud_conflicts,
            description: desc,
        }))
    } else {
        Ok(None)
    }
}
