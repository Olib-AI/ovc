//! Cross-process file locking for `.ovc` repositories.
//!
//! Uses atomic file creation (`create_new`) to prevent concurrent access
//! to the same `.ovc` file from multiple processes. The lock file contains
//! diagnostic information (PID, hostname, timestamp) for stale lock detection.

use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::error::{CoreError, CoreResult};

/// An advisory lock on a `.ovc` repository file.
///
/// The lock is held for the lifetime of this struct. Dropping it releases
/// the lock by removing the lock file.
pub struct RepoLock {
    /// Path to the lock file (for cleanup on drop).
    lock_path: PathBuf,
}

impl RepoLock {
    /// Default timeout for acquiring a lock.
    const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

    /// Interval between retry attempts.
    const RETRY_INTERVAL: Duration = Duration::from_millis(100);

    /// Maximum age of a lock file before it is considered stale (5 minutes).
    const STALE_THRESHOLD: Duration = Duration::from_mins(5);

    /// Acquires an exclusive lock on the repository at `ovc_path`.
    ///
    /// Creates a `.ovc.lock` file using atomic `create_new` and writes
    /// diagnostic information. Blocks up to `timeout` waiting for the lock.
    /// Returns an error if the lock cannot be acquired within the timeout.
    pub fn acquire(ovc_path: &Path, timeout: Option<Duration>) -> CoreResult<Self> {
        let lock_path = lock_path_for(ovc_path);
        let timeout = timeout.unwrap_or(Self::DEFAULT_TIMEOUT);
        let start = Instant::now();

        loop {
            // Try to detect and clean up stale locks.
            Self::try_clean_stale_lock(&lock_path);

            match Self::try_create_lock_file(&lock_path) {
                Ok(()) => {
                    return Ok(Self { lock_path });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    if start.elapsed() >= timeout {
                        let holder = std::fs::read_to_string(&lock_path)
                            .unwrap_or_else(|_| "unknown".to_owned());
                        return Err(CoreError::LockError {
                            reason: format!(
                                "could not acquire lock on '{}' within {}s — held by:\n{}",
                                ovc_path.display(),
                                timeout.as_secs(),
                                holder.trim(),
                            ),
                        });
                    }
                    std::thread::sleep(Self::RETRY_INTERVAL);
                }
                Err(e) => return Err(CoreError::Io(e)),
            }
        }
    }

    /// Tries to acquire the lock without blocking.
    ///
    /// Returns `Ok(None)` if the lock is already held by another process.
    pub fn try_acquire(ovc_path: &Path) -> CoreResult<Option<Self>> {
        let lock_path = lock_path_for(ovc_path);

        match Self::try_create_lock_file(&lock_path) {
            Ok(()) => Ok(Some(Self { lock_path })),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(None),
            Err(e) => Err(CoreError::Io(e)),
        }
    }

    /// Attempts to atomically create and populate the lock file.
    fn try_create_lock_file(lock_path: &Path) -> io::Result<()> {
        use std::io::Write;

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock_path)?;

        let lock_info = format!(
            "pid: {}\nhost: {}\ntime: {}\n",
            std::process::id(),
            hostname(),
            unix_timestamp_secs(),
        );
        file.write_all(lock_info.as_bytes())?;
        file.flush()?;
        file.sync_all()?;

        Ok(())
    }

    /// Checks whether a lock file is stale and removes it if so.
    ///
    /// A lock is considered stale if ANY of these are true:
    /// 1. The holding process is dead (confirmed via `kill -0` on same host)
    /// 2. The lock is from a different host (we can't verify the PID remotely)
    /// 3. The lock is older than `STALE_THRESHOLD` (fallback for any edge case)
    fn try_clean_stale_lock(lock_path: &Path) {
        if !lock_path.exists() {
            return;
        }

        // Read lock info to determine holder.
        let lock_info = std::fs::read_to_string(lock_path).unwrap_or_default();
        let lock_host = lock_info
            .lines()
            .find_map(|l| l.strip_prefix("host: "))
            .unwrap_or("")
            .trim();
        let current_host = hostname();

        // Case 1: Different host — we can't check the PID remotely.
        // The holder is on another machine, so we can't verify it's alive.
        // Remove the lock — the holder will re-acquire if still active.
        if !lock_host.is_empty() && lock_host != "unknown" && lock_host != current_host {
            let _ = std::fs::remove_file(lock_path);
            return;
        }

        // Case 2: Same host — check if the PID is alive.
        // If the process is dead, the lock is definitively stale.
        if !is_lock_holder_alive(lock_path) {
            let _ = std::fs::remove_file(lock_path);
            return;
        }

        // Case 3: Fallback — lock is older than threshold regardless of PID.
        if let Ok(metadata) = std::fs::metadata(lock_path)
            && let Ok(modified) = metadata.modified()
            && let Ok(age) = modified.elapsed()
            && age > Self::STALE_THRESHOLD
        {
            let _ = std::fs::remove_file(lock_path);
        }
    }
}

impl Drop for RepoLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

/// Returns the lock file path for a given `.ovc` file.
fn lock_path_for(ovc_path: &Path) -> PathBuf {
    ovc_path.with_extension("ovc.lock")
}

/// Checks if the process that wrote the lock file is still alive.
fn is_lock_holder_alive(lock_path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(lock_path) else {
        return false;
    };

    for line in content.lines() {
        if let Some(pid_str) = line.strip_prefix("pid: ")
            && let Ok(pid) = pid_str.trim().parse::<i32>()
        {
            return is_process_alive(pid);
        }
    }
    false
}

/// Checks whether a process with the given PID is alive.
///
/// On Unix, uses `kill(pid, 0)` which checks existence without sending a signal.
/// On non-Unix platforms, conservatively assumes the process is alive.
fn is_process_alive(pid: i32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: This uses libc::kill which is always safe to call with signal 0.
        // However, since we forbid unsafe_code, we use Command instead.
        let result = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .status();
        result.is_ok_and(|s| s.success())
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true // Assume alive on non-Unix
    }
}

/// Returns the hostname for lock file diagnostic info.
fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "unknown".to_owned())
}

/// Returns the current Unix timestamp in seconds.
fn unix_timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_path_has_correct_extension() {
        let path = Path::new("/tmp/repo.ovc");
        let lock = lock_path_for(path);
        assert_eq!(lock, Path::new("/tmp/repo.ovc.lock"));
    }

    #[test]
    fn acquire_and_drop_releases_lock() {
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("test.ovc");
        std::fs::write(&ovc_path, b"dummy").unwrap();

        let lock_path = lock_path_for(&ovc_path);
        {
            let _lock = RepoLock::acquire(&ovc_path, Some(Duration::from_secs(1))).unwrap();
            assert!(lock_path.exists());
        }
        // After drop, lock file should be removed.
        assert!(!lock_path.exists());
    }

    #[test]
    fn try_acquire_returns_none_when_held() {
        let dir = tempfile::tempdir().unwrap();
        let ovc_path = dir.path().join("test.ovc");
        std::fs::write(&ovc_path, b"dummy").unwrap();

        let _lock = RepoLock::acquire(&ovc_path, Some(Duration::from_secs(1))).unwrap();
        let result = RepoLock::try_acquire(&ovc_path).unwrap();
        assert!(result.is_none());
    }
}
