//! Write-ahead log for crash recovery.
//!
//! Before modifying the `.ovc` file, a WAL entry is written describing
//! the intended operation. If a crash occurs mid-write, the WAL is
//! replayed on next open to recover a consistent state.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};

/// A write-ahead log entry describing an in-progress operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalEntry {
    /// Unique identifier for this operation.
    pub operation_id: String,
    /// Type of operation being performed.
    pub operation: WalOperation,
    /// `file_sequence` before the operation started.
    pub sequence_before: u64,
    /// Unix timestamp when the operation started.
    pub started_at: u64,
    /// Current status of the operation.
    pub status: WalStatus,
}

/// Types of operations tracked by the WAL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WalOperation {
    /// A save/write operation.
    Save,
    /// A merge operation.
    Merge {
        /// The branch being merged.
        source_branch: String,
    },
    /// A rebase operation.
    Rebase {
        /// The target base commit or branch.
        onto: String,
    },
}

/// Status of a WAL entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WalStatus {
    /// Operation has started but not completed.
    InProgress,
    /// Operation completed successfully.
    Completed,
    /// Operation failed and needs recovery.
    Failed,
}

/// Manages the write-ahead log for a repository.
pub struct WriteAheadLog {
    /// Path to the WAL file (`.ovc.wal`).
    wal_path: PathBuf,
}

impl WriteAheadLog {
    /// Creates a new WAL manager for the given `.ovc` file.
    #[must_use]
    pub fn new(ovc_path: &Path) -> Self {
        Self {
            wal_path: ovc_path.with_extension("ovc.wal"),
        }
    }

    /// Checks if there is an incomplete WAL entry indicating crash recovery is needed.
    pub fn needs_recovery(&self) -> CoreResult<Option<WalEntry>> {
        if !self.wal_path.is_file() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&self.wal_path)?;
        let entry: WalEntry =
            serde_json::from_str(&content).map_err(|e| CoreError::Serialization {
                reason: format!("failed to parse WAL: {e}"),
            })?;

        if entry.status == WalStatus::InProgress {
            Ok(Some(entry))
        } else {
            // Completed or failed entries with no in-progress work can be cleaned up.
            let _ = std::fs::remove_file(&self.wal_path);
            Ok(None)
        }
    }

    /// Begins a new WAL entry before starting an operation.
    pub fn begin(&self, operation: WalOperation, sequence_before: u64) -> CoreResult<WalEntry> {
        let entry = WalEntry {
            operation_id: generate_operation_id(),
            operation,
            sequence_before,
            started_at: unix_timestamp(),
            status: WalStatus::InProgress,
        };

        self.write_entry(&entry)?;
        Ok(entry)
    }

    /// Marks the current WAL entry as completed and removes the WAL file.
    pub fn complete(&self) -> CoreResult<()> {
        let _ = std::fs::remove_file(&self.wal_path);
        Ok(())
    }

    /// Marks the current WAL entry as failed for later recovery.
    pub fn mark_failed(&self) -> CoreResult<()> {
        if let Ok(content) = std::fs::read_to_string(&self.wal_path)
            && let Ok(mut entry) = serde_json::from_str::<WalEntry>(&content)
        {
            entry.status = WalStatus::Failed;
            let _ = self.write_entry(&entry);
        }
        Ok(())
    }

    /// Attempts crash recovery using the WAL and any backup/temp files.
    ///
    /// If orphaned `.ovc.*.tmp` files exist (from a randomized temp name) and
    /// the WAL indicates an in-progress save, we know the save did not
    /// complete. The original `.ovc` file should still be intact (since
    /// rename is atomic), so we just clean up the orphaned temp files.
    pub fn recover(ovc_path: &Path) -> CoreResult<RecoveryResult> {
        let wal = Self::new(ovc_path);
        let backup_path = ovc_path.with_extension("ovc.bak");

        let Some(entry) = wal.needs_recovery()? else {
            // No WAL, but check for orphaned temp files.
            if cleanup_orphaned_temp_files(ovc_path) {
                return Ok(RecoveryResult::CleanedUpTempFile);
            }
            return Ok(RecoveryResult::NoRecoveryNeeded);
        };

        match entry.status {
            WalStatus::InProgress => {
                // Save was in progress when crash occurred.
                // The atomic rename pattern means `.ovc` is either the old version
                // (rename did not happen) or the new version (rename completed).
                // Either way, the `.ovc` file is consistent. Just clean up.
                cleanup_orphaned_temp_files(ovc_path);
                let _ = std::fs::remove_file(&wal.wal_path);
                Ok(RecoveryResult::RecoveredFromCrash {
                    operation_id: entry.operation_id,
                    restored_sequence: entry.sequence_before,
                })
            }
            WalStatus::Failed => {
                // Operation was explicitly marked as failed.
                // If a backup exists, consider restoring it.
                if backup_path.exists() && ovc_path.exists() {
                    let current_seq = crate::conflict::read_file_sequence(ovc_path).ok();
                    let backup_seq = crate::conflict::read_file_sequence(&backup_path).ok();

                    if backup_seq > current_seq {
                        std::fs::rename(&backup_path, ovc_path)?;
                    } else {
                        let _ = std::fs::remove_file(&backup_path);
                    }
                }
                let _ = std::fs::remove_file(&wal.wal_path);
                Ok(RecoveryResult::RestoredFromBackup {
                    operation_id: entry.operation_id,
                })
            }
            WalStatus::Completed => {
                // Should not reach here (completed entries are cleaned up in `needs_recovery`).
                let _ = std::fs::remove_file(&wal.wal_path);
                Ok(RecoveryResult::NoRecoveryNeeded)
            }
        }
    }

    /// Writes a WAL entry to disk atomically, ensuring durability with `sync_all`.
    ///
    /// Uses a write-to-temp-then-rename pattern so that a disk-full error or
    /// crash during the write never truncates the existing WAL file. Without
    /// this, `std::fs::write` would truncate the WAL to zero bytes before
    /// writing, destroying recovery information if the write fails partway
    /// through (e.g., due to ENOSPC).
    fn write_entry(&self, entry: &WalEntry) -> CoreResult<()> {
        use std::io::Write;

        let json = serde_json::to_string_pretty(entry).map_err(|e| CoreError::Serialization {
            reason: format!("failed to serialize WAL: {e}"),
        })?;

        // Write to a temporary file first, then atomically rename.
        let tmp_path = self.wal_path.with_extension("wal.tmp");
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(json.as_bytes()).map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            CoreError::Io(e)
        })?;
        file.flush().map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            CoreError::Io(e)
        })?;
        file.sync_all().map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            CoreError::Io(e)
        })?;
        drop(file);

        std::fs::rename(&tmp_path, &self.wal_path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            CoreError::Io(e)
        })?;

        Ok(())
    }
}

/// Result of a crash recovery attempt.
#[derive(Debug)]
pub enum RecoveryResult {
    /// No recovery was needed.
    NoRecoveryNeeded,
    /// Cleaned up an orphaned temp file (no WAL present).
    CleanedUpTempFile,
    /// Recovered from a crash during a save operation.
    RecoveredFromCrash {
        /// The operation ID from the WAL entry.
        operation_id: String,
        /// The sequence number that was restored.
        restored_sequence: u64,
    },
    /// Restored from a backup file after a failed operation.
    RestoredFromBackup {
        /// The operation ID from the WAL entry.
        operation_id: String,
    },
}

/// Generates a unique operation ID using random bytes formatted as a UUID v4.
fn generate_operation_id() -> String {
    use rand::RngCore;

    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 1
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

/// Returns the current Unix timestamp in seconds.
fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Cleans up orphaned temp files left behind by interrupted save operations.
///
/// Temp files use a randomized name pattern: `<name>.ovc.<hex>.tmp`.
/// This function scans the parent directory for any files matching that
/// pattern (based on the `.ovc` file's stem) and removes them.
///
/// Returns `true` if any files were cleaned up.
fn cleanup_orphaned_temp_files(ovc_path: &Path) -> bool {
    let Some(parent) = ovc_path.parent() else {
        return false;
    };
    let Some(stem) = ovc_path.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };

    // Match files like "<stem>.ovc.<hex>.tmp" or the legacy "<stem>.ovc.tmp"
    let prefix = format!("{stem}.ovc.");
    let mut cleaned = false;

    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name_str) = name.to_str() else {
                continue;
            };
            if name_str.starts_with(&prefix)
                && std::path::Path::new(name_str)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"))
            {
                let _ = std::fs::remove_file(entry.path());
                cleaned = true;
            }
        }
    }

    cleaned
}
