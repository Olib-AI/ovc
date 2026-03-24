//! Run history persistence — stores action run records as JSON files.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{ActionsError, ActionsResult};
use crate::runner::ActionResult;

/// A complete record of a single action run (potentially multiple actions).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRunRecord {
    /// Unique run identifier.
    pub run_id: String,
    /// Trigger that initiated this run.
    pub trigger: String,
    /// ISO-8601 timestamp of the run.
    pub timestamp: String,
    /// Results for each action in this run.
    pub results: Vec<ActionResult>,
    /// Overall status summary.
    pub overall_status: String,
    /// Total duration in milliseconds.
    pub total_duration_ms: u64,
}

/// Manages reading and writing action run history.
pub struct ActionHistory {
    history_dir: PathBuf,
}

impl ActionHistory {
    /// Create a new history manager rooted at `repo_root/.ovc/actions-history/`.
    #[must_use]
    pub fn new(repo_root: &Path) -> Self {
        Self {
            history_dir: repo_root.join(".ovc").join("actions-history"),
        }
    }

    /// Maximum bytes of stdout/stderr stored per action result.
    const MAX_OUTPUT_BYTES: usize = 64 * 1024;

    /// Persist a run record as a JSON file.
    ///
    /// stdout and stderr are capped at 64 KiB per action result to prevent
    /// unbounded storage and accidental secret retention.
    pub fn record_run(&self, record: &ActionRunRecord) -> ActionsResult<()> {
        validate_run_id(&record.run_id)?;
        std::fs::create_dir_all(&self.history_dir)?;

        let mut capped = record.clone();
        for result in &mut capped.results {
            truncate_output(&mut result.stdout, Self::MAX_OUTPUT_BYTES);
            truncate_output(&mut result.stderr, Self::MAX_OUTPUT_BYTES);
        }

        let file_path = self.history_dir.join(format!("{}.json", capped.run_id));
        let json = serde_json::to_string_pretty(&capped).map_err(|e| ActionsError::Config {
            reason: e.to_string(),
        })?;
        std::fs::write(file_path, json)?;
        Ok(())
    }

    /// List the most recent runs (sorted newest first), limited to `limit`.
    pub fn list_runs(&self, limit: usize) -> ActionsResult<Vec<ActionRunRecord>> {
        if !self.history_dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut entries: Vec<_> = std::fs::read_dir(&self.history_dir)?
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
            .collect();

        // Sort by modification time, newest first
        entries.sort_by(|a, b| {
            let ta = a
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            let tb = b
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            tb.cmp(&ta)
        });

        let mut records = Vec::new();
        for entry in entries.into_iter().take(limit) {
            let content = std::fs::read_to_string(entry.path())?;
            let record: ActionRunRecord =
                serde_json::from_str(&content).map_err(|e| ActionsError::Config {
                    reason: e.to_string(),
                })?;
            records.push(record);
        }

        Ok(records)
    }

    /// Remove all history records.
    pub fn clear(&self) -> ActionsResult<usize> {
        if !self.history_dir.is_dir() {
            return Ok(0);
        }
        let mut count = 0usize;
        for entry in std::fs::read_dir(&self.history_dir)?
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        {
            std::fs::remove_file(entry.path())?;
            count += 1;
        }
        Ok(count)
    }

    /// Get a specific run by its ID.
    pub fn get_run(&self, run_id: &str) -> ActionsResult<Option<ActionRunRecord>> {
        validate_run_id(run_id)?;
        let file_path = self.history_dir.join(format!("{run_id}.json"));
        if !file_path.is_file() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(file_path)?;
        let record: ActionRunRecord =
            serde_json::from_str(&content).map_err(|e| ActionsError::Config {
                reason: e.to_string(),
            })?;
        Ok(Some(record))
    }
}

/// Truncate a string in-place to at most `max_bytes` bytes (on a char boundary),
/// appending a `[truncated]` marker if truncation occurred.
fn truncate_output(s: &mut String, max_bytes: usize) {
    if s.len() <= max_bytes {
        return;
    }
    // Find the nearest char boundary at or before max_bytes.
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    s.push_str("\n[truncated]");
}

/// Reject run IDs that could cause path traversal.
fn validate_run_id(run_id: &str) -> ActionsResult<()> {
    if run_id.is_empty()
        || run_id.contains('/')
        || run_id.contains('\\')
        || run_id.contains("..")
        || run_id.contains('\0')
    {
        return Err(ActionsError::PathTraversal {
            path: run_id.to_owned(),
        });
    }
    Ok(())
}
