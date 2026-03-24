//! Shared application state for the API server.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use zeroize::Zeroizing;

/// Per-IP rate limiter for authentication endpoints.
///
/// Uses a sliding-window approach: each IP gets a counter that resets after
/// [`AUTH_RATE_WINDOW`]. If the counter exceeds [`AUTH_RATE_LIMIT`], further
/// requests are rejected with 429 Too Many Requests.
const AUTH_RATE_LIMIT: u32 = 10;
const AUTH_RATE_WINDOW: std::time::Duration = std::time::Duration::from_secs(60);

/// A single rate-limit bucket for one IP address.
#[derive(Debug, Clone)]
pub struct RateBucket {
    /// Number of requests in the current window.
    pub count: u32,
    /// When the current window started.
    pub window_start: Instant,
}

/// Shared state accessible by all route handlers.
///
/// Wrapped in `Arc` at the router level so handlers receive `State<Arc<AppState>>`.
pub struct AppState {
    /// Directory containing `.ovc` repository files.
    pub repos_dir: PathBuf,
    /// JWT signing secret (HMAC-SHA256). Zeroized on drop to prevent leaking
    /// key material in freed memory.
    pub jwt_secret: Zeroizing<String>,
    /// In-memory cache of unlocked repository passwords, keyed by repo id.
    /// Populated by the `/repos/:id/unlock` endpoint. Values are zeroized on
    /// drop so passwords do not linger in freed heap memory.
    pub passwords: RwLock<HashMap<String, Zeroizing<String>>>,
    /// Per-repository mutex to serialise mutation operations (create commit,
    /// branch, tag, stash, rebase, cherry-pick, GC, etc.). Read-only endpoints
    /// skip the lock since they never call `save()`.
    pub repo_locks: RwLock<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Maps `repo_id` → working directory path. Populated from `--workdir` flags,
    /// `OVC_WORKDIR_MAP` env var, or auto-discovered from `.ovc-link` files.
    pub workdirs: RwLock<HashMap<String, PathBuf>>,
    /// Pending authentication challenges for key-based auth.
    /// Maps challenge hex → (challenge bytes, expiry instant).
    pub auth_challenges: RwLock<HashMap<String, (Vec<u8>, Instant)>>,
    /// Per-IP rate limiter for auth endpoints.
    pub auth_rate_limits: RwLock<HashMap<String, RateBucket>>,
    /// Short-lived cache of repo stats returned by `GET /repos`.
    ///
    /// Keyed by repo id. Each entry is `(populated_at, head_ref, stats)`.
    /// Entries are considered fresh for [`REPO_STATS_TTL`]. Stale entries are
    /// replaced on the next `list_repos` call. Write operations (commit, branch,
    /// etc.) can call [`AppState::invalidate_repo_stats`] to evict eagerly.
    pub repo_stats_cache: RwLock<HashMap<String, (Instant, String, crate::models::RepoStats)>>,
    /// Monotonically increasing version counter for issued tokens.
    ///
    /// Each token embeds the current version at issuance time. When any
    /// access revocation occurs (ACL change, manual revoke), this counter
    /// is incremented. Any token carrying a stale version is rejected,
    /// effectively invalidating all previously issued tokens in O(1) without
    /// a server-side revocation store.
    pub token_version: Arc<AtomicU64>,
}

impl AppState {
    /// Creates a new `AppState` with the given configuration.
    #[must_use]
    pub fn new(repos_dir: PathBuf, jwt_secret: String) -> Self {
        let mut workdirs = HashMap::new();

        // Auto-discover workdirs from OVC_WORKDIR_MAP env var
        // Format: "repo1:/path/to/workdir1,repo2:/path/to/workdir2"
        if let Ok(map) = std::env::var("OVC_WORKDIR_MAP") {
            for entry in map.split(',') {
                let entry = entry.trim();
                if let Some((name, path)) = entry.split_once(':') {
                    workdirs.insert(name.trim().to_owned(), PathBuf::from(path.trim()));
                }
            }
        }

        // Auto-discover by scanning for .ovc-link files that point to repos in repos_dir
        Self::discover_workdirs_from_links(&repos_dir, &mut workdirs);

        Self {
            repos_dir,
            jwt_secret: Zeroizing::new(jwt_secret),
            passwords: RwLock::new(HashMap::new()),
            repo_locks: RwLock::new(HashMap::new()),
            workdirs: RwLock::new(workdirs),
            auth_challenges: RwLock::new(HashMap::new()),
            auth_rate_limits: RwLock::new(HashMap::new()),
            repo_stats_cache: RwLock::new(HashMap::new()),
            token_version: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Scans common project directories for `.ovc-link` files that point to
    /// repos in the `repos_dir`. This auto-discovers workdir mappings.
    fn discover_workdirs_from_links(repos_dir: &Path, workdirs: &mut HashMap<String, PathBuf>) {
        // Check OVC_WORKDIR_SCAN env var for directories to scan
        // Default: scan nothing (user must configure explicitly)
        let scan_dirs = match std::env::var("OVC_WORKDIR_SCAN") {
            Ok(dirs) => dirs
                .split(',')
                .map(|s| PathBuf::from(s.trim()))
                .collect::<Vec<_>>(),
            Err(_) => return,
        };

        let repos_dir_canonical = repos_dir
            .canonicalize()
            .unwrap_or_else(|_| repos_dir.to_path_buf());

        for scan_dir in &scan_dirs {
            if !scan_dir.is_dir() {
                continue;
            }
            // Scan one level deep for .ovc-link files
            if let Ok(entries) = std::fs::read_dir(scan_dir) {
                for entry in entries.flatten() {
                    let link_path = entry.path().join(".ovc-link");
                    if link_path.is_file()
                        && let Ok(content) = std::fs::read_to_string(&link_path)
                    {
                        let target = PathBuf::from(content.trim());
                        if let Ok(canonical) = target.canonicalize()
                            && canonical.starts_with(&repos_dir_canonical)
                            && let Some(stem) = canonical.file_stem().and_then(|s| s.to_str())
                        {
                            workdirs.insert(stem.to_owned(), entry.path());
                        }
                    }
                }
            }
        }
    }

    /// Returns the working directory for a repo, if known.
    pub fn workdir_for(&self, repo_id: &str) -> Option<PathBuf> {
        self.workdirs.read().ok()?.get(repo_id).cloned()
    }

    /// Registers a workdir mapping.
    pub fn set_workdir(&self, repo_id: &str, workdir: PathBuf) {
        if let Ok(mut map) = self.workdirs.write() {
            map.insert(repo_id.to_owned(), workdir);
        }
    }

    /// Hard cap on the number of pending authentication challenges.
    ///
    /// New challenge requests are rejected with a 429-style error when this
    /// limit is hit, preventing a memory-exhaustion attack via unbounded GET
    /// `/auth/challenge` requests.
    pub const MAX_PENDING_CHALLENGES: usize = 10_000;

    /// Maximum number of per-repo locks to keep. When exceeded, locks that
    /// are not currently held (strong count == 1, only the map holds them)
    /// are evicted to prevent unbounded memory growth in long-running daemons.
    const MAX_REPO_LOCKS: usize = 1024;

    /// Removes all expired challenges from the challenge cache.
    ///
    /// Called by the background cleanup task every 60 seconds to bound memory
    /// use regardless of whether POST `/auth/key-auth` is ever called.
    pub fn cleanup_expired_challenges(&self) {
        let now = Instant::now();
        if let Ok(mut challenges) = self.auth_challenges.write() {
            challenges.retain(|_, (_, expiry)| *expiry > now);
        }
    }

    /// Returns the per-repo mutex for the given repository id, creating one if
    /// it does not already exist. Evicts unused entries when the map exceeds
    /// [`Self::MAX_REPO_LOCKS`].
    pub fn repo_lock(&self, repo_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        // Fast path: read lock.
        if let Ok(locks) = self.repo_locks.read()
            && let Some(lock) = locks.get(repo_id)
        {
            return Arc::clone(lock);
        }
        // Slow path: write lock to insert.
        // Recover from a poisoned lock: the data (a map of Mutexes) is still
        // perfectly usable even if a previous thread panicked while holding the
        // write guard. Panicking here would crash the entire API server.
        let mut locks = self.repo_locks.write().unwrap_or_else(|e| {
            tracing::warn!("repo_locks RwLock was poisoned; recovering from poison");
            e.into_inner()
        });

        // Evict unused locks when the map is too large.
        if locks.len() >= Self::MAX_REPO_LOCKS {
            locks.retain(|_, v| Arc::strong_count(v) > 1);
        }

        Arc::clone(
            locks
                .entry(repo_id.to_owned())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }

    /// How long a cached repo stats entry remains fresh.
    pub const REPO_STATS_TTL: Duration = Duration::from_secs(30);

    /// Returns cached `(head, stats)` for `repo_id` if the entry exists and
    /// is still within [`Self::REPO_STATS_TTL`]. Returns `None` on a cache
    /// miss or if the entry is stale.
    #[must_use]
    pub fn get_cached_repo_stats(
        &self,
        repo_id: &str,
    ) -> Option<(String, crate::models::RepoStats)> {
        // Acquire the read lock, clone what we need, then immediately release
        // the guard so it is not held across the return.
        let cache = self.repo_stats_cache.read().ok()?;
        let (populated_at, head, stats) = cache.get(repo_id)?;
        let result = if populated_at.elapsed() <= Self::REPO_STATS_TTL {
            Some((head.clone(), stats.clone()))
        } else {
            None
        };
        drop(cache);
        result
    }

    /// Inserts or updates the stats cache entry for `repo_id`.
    pub fn set_cached_repo_stats(
        &self,
        repo_id: &str,
        head: String,
        stats: crate::models::RepoStats,
    ) {
        if let Ok(mut cache) = self.repo_stats_cache.write() {
            cache.insert(repo_id.to_owned(), (Instant::now(), head, stats));
        }
    }

    /// Evicts the stats cache entry for `repo_id`.
    ///
    /// Call this after any write operation (commit, branch creation, tag, etc.)
    /// so that the next `list_repos` reflects fresh data instead of serving a
    /// stale TTL entry.
    pub fn invalidate_repo_stats(&self, repo_id: &str) {
        if let Ok(mut cache) = self.repo_stats_cache.write() {
            cache.remove(repo_id);
        }
    }

    /// Returns the current token version.
    ///
    /// Tokens that carry a version lower than this value are rejected.
    #[must_use]
    pub fn current_token_version(&self) -> u64 {
        self.token_version.load(Ordering::Acquire)
    }

    /// Increments the token version, invalidating all currently issued tokens.
    ///
    /// Call this whenever access control changes (ACL modification, manual
    /// revoke) so that existing sessions cannot continue operating with stale
    /// permissions.
    pub fn revoke_all_tokens(&self) {
        self.token_version.fetch_add(1, Ordering::AcqRel);
    }

    /// Checks and increments the auth rate limit for the given IP.
    ///
    /// Returns `true` if the request is allowed, `false` if rate-limited.
    /// Automatically resets the window when it expires and periodically
    /// evicts stale entries.
    #[allow(clippy::significant_drop_tightening)]
    pub fn check_auth_rate_limit(&self, ip: &str) -> bool {
        // Recover from a poisoned lock: rate-limit counters are non-critical
        // state. A transient panic in another thread must not crash the server.
        let mut limits = self.auth_rate_limits.write().unwrap_or_else(|e| {
            tracing::warn!("auth_rate_limits RwLock was poisoned; recovering from poison");
            e.into_inner()
        });
        let now = Instant::now();

        // Periodic cleanup: evict expired entries when the map gets large.
        if limits.len() > 10_000 {
            limits.retain(|_, bucket| now.duration_since(bucket.window_start) < AUTH_RATE_WINDOW);
        }

        let bucket = limits.entry(ip.to_owned()).or_insert(RateBucket {
            count: 0,
            window_start: now,
        });

        // Reset window if expired.
        if now.duration_since(bucket.window_start) >= AUTH_RATE_WINDOW {
            bucket.count = 0;
            bucket.window_start = now;
        }

        if bucket.count >= AUTH_RATE_LIMIT {
            return false;
        }

        bucket.count += 1;
        true
    }
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("repos_dir", &self.repos_dir)
            .field("jwt_secret", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}
