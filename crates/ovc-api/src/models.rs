//! Request and response types for the REST API.
//!
//! All types derive `Serialize` and/or `Deserialize` for JSON encoding.
//! Designed for consumption by a React SPA frontend.

use serde::{Deserialize, Serialize};

// ── Repository ──────────────────────────────────────────────────────────

/// Repository summary returned by list and detail endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoInfo {
    /// Repository identifier (filename without `.ovc` extension).
    pub id: String,
    /// Display name (same as id).
    pub name: String,
    /// Filesystem path to the `.ovc` file.
    pub path: String,
    /// Current HEAD reference.
    pub head: String,
    /// Aggregate statistics.
    pub repo_stats: RepoStats,
}

/// Aggregate repository statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoStats {
    /// Total number of commits.
    pub total_commits: u64,
    /// Total number of branches.
    pub total_branches: u64,
    /// Total number of tags.
    pub total_tags: u64,
    /// Number of files tracked in the index.
    pub tracked_files: u64,
}

/// Request body for creating a new repository.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateRepoRequest {
    /// Name for the new repository (used as the `.ovc` filename).
    pub name: String,
    /// Encryption password.
    pub password: String,
}

/// Request body for unlocking a repository.
#[derive(Debug, Clone, Deserialize)]
pub struct UnlockRepoRequest {
    /// The repository encryption password.
    pub password: String,
}

/// Request body for updating repository configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateRepoConfigRequest {
    /// New default commit author name.
    #[serde(default)]
    pub user_name: Option<String>,
    /// New default commit author email.
    #[serde(default)]
    pub user_email: Option<String>,
    /// New default branch name.
    #[serde(default)]
    pub default_branch: Option<String>,
}

/// Response for repository configuration.
#[derive(Debug, Clone, Serialize)]
pub struct RepoConfigResponse {
    /// Current commit author name.
    pub user_name: String,
    /// Current commit author email.
    pub user_email: String,
    /// Current default branch name.
    pub default_branch: String,
}

// ── Files ───────────────────────────────────────────────────────────────

/// A single entry in the file tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTreeEntry {
    /// Entry name (filename or directory name).
    pub name: String,
    /// Full path relative to repository root.
    pub path: String,
    /// Type: `"file"` or `"directory"`.
    pub entry_type: String,
    /// File size in bytes (0 for directories).
    pub size: u64,
}

/// File content returned by the blob endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContent {
    /// Path relative to repository root.
    pub path: String,
    /// Textual content (empty string for binary files).
    pub content: String,
    /// Whether the content is binary.
    pub is_binary: bool,
    /// Size in bytes.
    pub size_bytes: u64,
}

/// Working tree status response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    /// Current branch name.
    pub branch: String,
    /// Staged file status entries.
    pub staged: Vec<FileStatusEntry>,
    /// Unstaged file status entries.
    ///
    /// Always empty when the API server runs in daemon mode (no working
    /// directory access). Check `has_workdir` to determine availability.
    pub unstaged: Vec<FileStatusEntry>,
    /// Untracked file paths.
    ///
    /// Always empty when the API server runs in daemon mode (no working
    /// directory access). Check `has_workdir` to determine availability.
    pub untracked: Vec<String>,
    /// Whether the server has access to a working directory.
    ///
    /// When `false`, `unstaged` and `untracked` will always be empty arrays
    /// because the daemon cannot observe the filesystem working tree.
    pub has_workdir: bool,
}

/// Status of a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStatusEntry {
    /// File path relative to repository root.
    pub path: String,
    /// Status string: `"added"`, `"modified"`, `"deleted"`.
    pub status: String,
}

/// Request to stage files.
#[derive(Debug, Clone, Deserialize)]
pub struct StageRequest {
    /// Paths to stage.
    pub paths: Vec<String>,
}

/// Request to unstage files.
#[derive(Debug, Clone, Deserialize)]
pub struct UnstageRequest {
    /// Paths to unstage.
    pub paths: Vec<String>,
}

/// Request to restore staged files to their HEAD versions.
#[derive(Debug, Clone, Deserialize)]
pub struct RestoreRequest {
    /// Paths to restore.
    pub paths: Vec<String>,
}

// ── Clean ───────────────────────────────────────────────────────────────

/// Request body for cleaning untracked files from the working directory.
#[derive(Debug, Clone, Deserialize)]
pub struct CleanRequest {
    /// Specific paths to clean. If absent, all untracked files are cleaned.
    pub paths: Option<Vec<String>>,
    /// If `true`, return the list of files that would be deleted without
    /// actually deleting them. Defaults to `false`.
    pub dry_run: Option<bool>,
}

/// Response from a clean operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanResponse {
    /// Paths that were deleted (or would be deleted in dry-run mode).
    pub deleted: Vec<String>,
}

// ── File CRUD ────────────────────────────────────────────────────────────

/// Request body for creating or updating a file via `PUT /repos/:id/blob`.
#[derive(Debug, Clone, Deserialize)]
pub struct PutBlobRequest {
    /// Relative path within the working directory.
    pub path: String,
    /// File content (interpreted according to `encoding`).
    pub content: String,
    /// Content encoding: `"utf8"` (default) or `"base64"` for binary data.
    #[serde(default = "default_encoding")]
    pub encoding: String,
}

/// Default file encoding.
fn default_encoding() -> String {
    "utf8".to_owned()
}

/// Response from a successful file write.
#[derive(Debug, Clone, Serialize)]
pub struct PutBlobResponse {
    /// The path that was written.
    pub path: String,
    /// Size of the written file in bytes.
    pub size_bytes: u64,
}

/// Request body for deleting a file via `DELETE /repos/:id/blob`.
#[derive(Debug, Clone, Deserialize)]
pub struct DeleteBlobRequest {
    /// Relative path within the working directory.
    pub path: String,
}

/// Response from a successful file deletion.
#[derive(Debug, Clone, Serialize)]
pub struct DeleteBlobResponse {
    /// The path that was deleted.
    pub path: String,
}

/// Request body for moving/renaming a file via `POST /repos/:id/move`.
#[derive(Debug, Clone, Deserialize)]
pub struct MoveFileRequest {
    /// Current relative path within the working directory.
    pub from_path: String,
    /// Target relative path within the working directory.
    pub to_path: String,
}

/// Response from a successful file move/rename.
#[derive(Debug, Clone, Serialize)]
pub struct MoveFileResponse {
    /// The original path.
    pub from_path: String,
    /// The new path.
    pub to_path: String,
    /// Whether the operation succeeded.
    pub success: bool,
}

/// Response from a multipart file upload.
#[derive(Debug, Clone, Serialize)]
pub struct UploadResponse {
    /// List of file paths that were created.
    pub files: Vec<UploadedFile>,
}

/// A single uploaded file.
#[derive(Debug, Clone, Serialize)]
pub struct UploadedFile {
    /// Relative path of the created file.
    pub path: String,
    /// Size of the written file in bytes.
    pub size_bytes: u64,
}

/// Request body for creating a directory via `POST /repos/:id/mkdir`.
#[derive(Debug, Clone, Deserialize)]
pub struct MkdirRequest {
    /// Relative path of the directory to create.
    pub path: String,
}

/// Response from a successful directory creation.
#[derive(Debug, Clone, Serialize)]
pub struct MkdirResponse {
    /// The path that was created.
    pub path: String,
}

// ── Commits ─────────────────────────────────────────────────────────────

/// Commit metadata returned by log and detail endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitInfo {
    /// Full commit object id (hex).
    pub id: String,
    /// Short id (first 12 hex characters).
    pub short_id: String,
    /// Commit message.
    pub message: String,
    /// Author identity.
    pub author: CommitAuthor,
    /// Author timestamp (ISO 8601).
    pub authored_at: String,
    /// Parent commit ids.
    pub parent_ids: Vec<String>,
    /// Signature status: `"verified"`, `"unverified"`, or `"unsigned"`.
    pub signature_status: String,
    /// Fingerprint of the signing key (if verified).
    pub signer_fingerprint: Option<String>,
    /// Signer identity string (e.g., `"Akram <akram@olib.ai>"`).
    pub signer_identity: Option<String>,
}

/// Commit author identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitAuthor {
    /// Display name.
    pub name: String,
    /// Email address.
    pub email: String,
}

/// Request body for creating a new commit.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateCommitRequest {
    /// Commit message.
    pub message: String,
    /// Author name.
    pub author_name: String,
    /// Author email.
    pub author_email: String,
    /// If `true`, amend the previous commit instead of creating a new one.
    pub amend: Option<bool>,
    /// If `true`, sign the commit with the configured Ed25519 key.
    /// Overrides the `OVC_SIGN_COMMITS` environment variable.
    pub sign: Option<bool>,
}

/// Commit log response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitLog {
    /// Ordered list of commits (newest first).
    pub commits: Vec<CommitInfo>,
}

// ── Shortlog ─────────────────────────────────────────────────────────────

/// Response from the shortlog endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortlogResponse {
    /// Author commit-count entries, sorted by count descending.
    pub authors: Vec<ShortlogAuthorEntry>,
}

/// A single author entry in the shortlog response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortlogAuthorEntry {
    /// Author display name.
    pub name: String,
    /// Author email address.
    pub email: String,
    /// Number of commits attributed to this author.
    pub count: usize,
}

// ── Diff ────────────────────────────────────────────────────────────────

/// Diff response containing changed files and statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffResponse {
    /// Per-file diffs.
    pub files: Vec<FileDiff>,
    /// Aggregate statistics.
    pub stats: DiffStats,
}

/// Aggregate diff statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffStats {
    /// Number of files changed.
    pub files_changed: u64,
    /// Total lines added.
    pub additions: u64,
    /// Total lines deleted.
    pub deletions: u64,
}

/// Diff for a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    /// File path.
    pub path: String,
    /// Change status: `"added"`, `"modified"`, `"deleted"`.
    pub status: String,
    /// Lines added in this file.
    #[serde(default)]
    pub additions: u64,
    /// Lines deleted in this file.
    #[serde(default)]
    pub deletions: u64,
    /// Hunks of changes (empty when `stats_only=true`).
    pub hunks: Vec<DiffHunk>,
}

/// A contiguous region of changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    /// Starting line in old file (1-based).
    pub old_start: usize,
    /// Number of lines from old file.
    pub old_count: usize,
    /// Starting line in new file (1-based).
    pub new_start: usize,
    /// Number of lines from new file.
    pub new_count: usize,
    /// Individual lines in this hunk.
    pub lines: Vec<DiffLine>,
}

/// A single line within a diff hunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffLine {
    /// Line kind.
    pub kind: DiffLineKind,
    /// Line content (without diff prefix).
    pub content: String,
}

/// The kind of a diff line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffLineKind {
    /// Unchanged context line.
    Context,
    /// Added line.
    Addition,
    /// Removed line.
    Deletion,
}

// ── Branches ────────────────────────────────────────────────────────────

/// Branch metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchInfo {
    /// Branch name.
    pub name: String,
    /// Commit id the branch points to (hex).
    pub commit_id: String,
    /// Whether this is the currently checked-out branch.
    pub is_current: bool,
}

/// Request body for creating a branch.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateBranchRequest {
    /// Branch name.
    pub name: String,
    /// Optional commit spec (hex OID, branch name, `HEAD~N`, etc.) to create
    /// the branch from. If absent, the branch is created at HEAD.
    pub start_point: Option<String>,
}

/// Request body for merging a branch.
#[derive(Debug, Clone, Deserialize)]
pub struct MergeRequest {
    /// Source branch to merge from.
    pub source_branch: String,
}

/// Merge operation response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeResponse {
    /// Outcome: `"merged"`, `"conflict"`, or `"already_up_to_date"`.
    pub status: String,
    /// Resulting commit id (if clean merge).
    pub commit_id: Option<String>,
    /// List of conflicted file paths (if any).
    pub conflict_files: Vec<String>,
    /// Human-readable description of the merge outcome.
    pub message: String,
}

// ── Tags ────────────────────────────────────────────────────────────────

/// Tag metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagInfo {
    /// Tag name.
    pub name: String,
    /// Commit id the tag points to (hex).
    pub commit_id: String,
    /// Annotation message (if the tag carries one).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Request body for creating a tag.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateTagRequest {
    /// Tag name.
    pub name: String,
    /// Commit id to tag. If absent, HEAD is used.
    pub commit_id: Option<String>,
    /// Annotation message for the tag.
    ///
    /// When present and non-empty, the tag is treated as an annotated tag and
    /// the message is persisted in the ref store. When absent or empty, a
    /// lightweight tag (no message) is created.
    pub message: Option<String>,
}

/// Request body for a reset operation.
#[derive(Debug, Clone, Deserialize)]
pub struct ResetRequest {
    /// Target commit id (hex, branch name, or HEAD~N).
    /// If absent, defaults to HEAD~1 (parent of current HEAD).
    pub commit_id: Option<String>,
    /// Reset mode: `"soft"`, `"mixed"`, or `"hard"`.
    /// Defaults to `"mixed"` if absent.
    #[serde(default = "default_reset_mode")]
    pub mode: String,
}

/// Default reset mode.
fn default_reset_mode() -> String {
    "mixed".to_owned()
}

/// Reset operation response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResetResponse {
    /// The commit id HEAD now points to (hex).
    pub commit_id: String,
    /// The reset mode that was applied.
    pub mode: String,
}

// ── Sync ────────────────────────────────────────────────────────────────

/// Sync status response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatusResponse {
    /// Status string: `"in_sync"`, `"local_ahead"`, `"remote_ahead"`, `"diverged"`, `"no_remote"`.
    pub status: String,
    /// Remote name (if configured).
    pub remote: Option<String>,
    /// Manifest version (if available).
    pub version: Option<u64>,
}

/// Push operation response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResponse {
    /// Number of chunks uploaded.
    pub chunks_uploaded: u64,
    /// Total bytes uploaded.
    pub bytes_uploaded: u64,
}

/// Pull operation response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullResponse {
    /// Number of chunks downloaded.
    pub chunks_downloaded: u64,
    /// Total bytes downloaded.
    pub bytes_downloaded: u64,
}

// ── Auth ────────────────────────────────────────────────────────────────

/// Request body for obtaining an auth token.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenRequest {
    /// Password or pre-shared secret.
    pub password: String,
}

/// Response containing a JWT token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    /// The JWT bearer token.
    pub token: String,
    /// Token expiry (ISO 8601).
    pub expires_at: String,
}

// ── Actions ─────────────────────────────────────────────────────────────

/// Summary information about a configured action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionInfo {
    /// Action key name.
    pub name: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Category (lint, format, build, test, audit, etc.).
    pub category: String,
    /// Programming language this action targets.
    pub language: Option<String>,
    /// External tool name.
    pub tool: Option<String>,
    /// Trigger events this action responds to.
    pub triggers: Vec<String>,
    /// Information about the most recent run, if available.
    pub last_run: Option<ActionLastRun>,
}

/// Summary of an action's most recent run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionLastRun {
    /// Outcome status (passed, failed, etc.).
    pub status: String,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// ISO-8601 timestamp.
    pub timestamp: String,
}

/// Response containing a list of configured actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionListResponse {
    /// All configured actions.
    pub actions: Vec<ActionInfo>,
}

/// Request body for running actions.
#[derive(Debug, Clone, Deserialize)]
pub struct RunActionsRequest {
    /// Specific action names to run. If absent, all actions for the trigger are run.
    pub names: Option<Vec<String>>,
    /// Trigger context (e.g. "pre-commit", "manual").
    pub trigger: Option<String>,
    /// Whether to run fix commands instead of check commands.
    #[serde(default)]
    pub fix: bool,
    /// File paths affected by this trigger context.
    ///
    /// When provided, actions with path-based conditions will be filtered
    /// against these paths. Defaults to an empty list if absent.
    pub changed_paths: Option<Vec<String>>,
}

/// Response from running actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunActionsResponse {
    /// Unique run identifier.
    pub run_id: String,
    /// Trigger that initiated this run.
    pub trigger: String,
    /// Overall outcome status.
    pub overall_status: String,
    /// Total duration in milliseconds.
    pub total_duration_ms: u64,
    /// Per-action results.
    pub results: Vec<ActionResultResponse>,
}

/// Result of a single action execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResultResponse {
    /// Action key name.
    pub name: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Category.
    pub category: String,
    /// Outcome status.
    pub status: String,
    /// Process exit code (if applicable).
    pub exit_code: Option<i32>,
    /// Captured stdout.
    pub stdout: String,
    /// Captured stderr.
    pub stderr: String,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// ISO-8601 start timestamp.
    pub started_at: String,
    /// ISO-8601 finish timestamp.
    pub finished_at: String,
    /// Whether Docker was used for execution.
    #[serde(default)]
    pub docker_used: bool,
}

/// Response from language detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionResponse {
    /// Detected languages and toolchains.
    pub languages: Vec<DetectedLanguageInfo>,
    /// Suggested starter configuration.
    pub suggested_config: serde_json::Value,
}

/// A single detected language.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedLanguageInfo {
    /// Language name.
    pub language: String,
    /// Detection confidence level.
    pub confidence: String,
    /// Marker file that triggered detection.
    pub marker_file: String,
    /// Directory where the marker was found.
    pub root_dir: String,
}

/// Response containing action run history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionHistoryListResponse {
    /// Recent run summaries, newest first.
    pub runs: Vec<ActionRunSummary>,
}

/// Summary of a single action run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRunSummary {
    /// Unique run identifier.
    pub run_id: String,
    /// Trigger that initiated this run.
    pub trigger: String,
    /// ISO-8601 timestamp.
    pub timestamp: String,
    /// Overall outcome status.
    pub overall_status: String,
    /// Total duration in milliseconds.
    pub total_duration_ms: u64,
    /// Total number of actions in this run.
    pub action_count: usize,
    /// Number of actions that passed.
    pub passed_count: usize,
    /// Number of actions that failed.
    pub failed_count: usize,
}

/// Request body for initializing actions configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct InitActionsRequest {
    /// Whether to overwrite an existing configuration file.
    #[serde(default)]
    pub force: bool,
}

/// Request body for running a single action.
#[derive(Debug, Clone, Deserialize)]
pub struct RunSingleActionRequest {
    /// Whether to run the fix command instead of the check command.
    #[serde(default)]
    pub fix: bool,
}

/// Request body for saving actions configuration YAML.
#[derive(Debug, Clone, Deserialize)]
pub struct PutActionsConfigRequest {
    /// The raw YAML content to write to `.ovc/actions.yml`.
    pub content: String,
}

// ── Dependency update check ──────────────────────────────────────────────

/// Structured report returned by `GET /api/v1/repos/:id/dependencies`.
#[derive(Debug, Clone, Serialize)]
pub struct DependencyReportResponse {
    /// Per-manifest breakdown.
    pub manifests: Vec<ManifestReportResponse>,
    /// Total number of dependencies with available updates.
    pub total_updates: usize,
    /// Count of major-version updates.
    pub major_updates: usize,
    /// Count of minor-version updates.
    pub minor_updates: usize,
    /// Count of patch-version updates.
    pub patch_updates: usize,
}

/// Results for a single manifest file.
#[derive(Debug, Clone, Serialize)]
pub struct ManifestReportResponse {
    /// Manifest file path relative to repo root.
    pub file: String,
    /// Package manager name (e.g. `"Cargo"`, `"npm"`).
    pub package_manager: String,
    /// Individual dependency statuses.
    pub dependencies: Vec<DependencyStatusResponse>,
}

/// Status of a single dependency.
#[derive(Debug, Clone, Serialize)]
pub struct DependencyStatusResponse {
    /// Package name.
    pub name: String,
    /// Version as declared in the manifest.
    pub current_version: String,
    /// Latest published version.
    pub latest_version: String,
    /// `"major"`, `"minor"`, `"patch"`, `"up-to-date"`, or `"unknown"`.
    pub update_type: String,
    /// True if this is a dev/test dependency.
    pub dev: bool,
}

// ── Dependency auto-update (Dependabot-style) ───────────────────────────────

/// Request body for `POST /api/v1/repos/:id/dependencies/update`.
///
/// When `updates` is absent or empty the server auto-updates **all** outdated
/// dependencies discovered by the dependency checker.
#[derive(Debug, Clone, Deserialize)]
pub struct DependencyUpdateRequest {
    /// Explicit list of updates to apply.  Leave empty / omit to update all.
    #[serde(default)]
    pub updates: Vec<DependencyUpdateItem>,
}

/// A single dependency update to apply.
#[derive(Debug, Clone, Deserialize)]
pub struct DependencyUpdateItem {
    /// Package name (e.g. `"tokio"`, `"react"`).
    #[serde(alias = "dependency")]
    pub name: String,
    /// Manifest file relative to the repo working directory (e.g. `"Cargo.toml"`).
    pub file: String,
    /// Target version (bare semver, e.g. `"1.50.0"`).
    #[serde(alias = "to_version")]
    pub new_version: String,
}

/// Result for a single auto-update proposal.
#[derive(Debug, Clone, Serialize)]
pub struct DependencyProposal {
    /// Branch created for this update (e.g. `"deps/cargo/tokio-1.50.0"`).
    pub branch: String,
    /// Package name.
    pub dependency: String,
    /// Manifest file path.
    pub file: String,
    /// Version before the update.
    pub from_version: String,
    /// Version after the update.
    pub to_version: String,
    /// Semver update classification: `"major"`, `"minor"`, `"patch"`, or `"unknown"`.
    pub update_type: String,
    /// Whether this branch merges cleanly into the original branch.
    pub mergeable: bool,
    /// Paths that conflict when merging (empty when `mergeable` is true).
    pub conflict_files: Vec<String>,
    /// Human-readable error if the branch could not be created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Response from `POST /api/v1/repos/:id/dependencies/update`.
#[derive(Debug, Clone, Serialize)]
pub struct DependencyUpdateResponse {
    /// All proposals, one per dependency that was processed.
    pub proposals: Vec<DependencyProposal>,
    /// Number of branches successfully created.
    pub created: usize,
    /// Number of cleanly mergeable branches.
    pub mergeable: usize,
    /// Number of branches with merge conflicts.
    pub conflicting: usize,
}

/// A single deps/* branch with its merge-readiness.
#[derive(Debug, Clone, Serialize)]
pub struct ProposalInfo {
    /// Full branch name (e.g. `"deps/cargo/tokio-1.50.0"`).
    pub branch: String,
    /// Commit the branch points to.
    pub commit_id: String,
    /// Whether the branch merges cleanly into the current HEAD branch.
    pub mergeable: bool,
    /// Conflicting file paths (if any).
    pub conflict_files: Vec<String>,
    /// Dependency name parsed from the branch (e.g. `"tokio"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency: Option<String>,
    /// Manifest file parsed from the branch (e.g. `"Cargo.toml"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// Response from `GET /api/v1/repos/:id/dependencies/proposals`.
#[derive(Debug, Clone, Serialize)]
pub struct DependencyProposalsResponse {
    /// All `deps/*` branches, newest first.
    pub proposals: Vec<ProposalInfo>,
    /// Total count.
    pub total: usize,
}

// ── Remotes ─────────────────────────────────────────────────────────────

/// Remote configuration returned by list and detail endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteInfo {
    /// Remote name.
    pub name: String,
    /// Remote URL.
    pub url: String,
    /// Backend type identifier (e.g., `"local"`, `"gcs"`).
    pub backend_type: String,
}

/// Request body for adding a remote.
#[derive(Debug, Clone, Deserialize)]
pub struct AddRemoteRequest {
    /// Remote name.
    pub name: String,
    /// Remote URL.
    pub url: String,
    /// Backend type identifier (e.g., `"local"`, `"gcs"`).
    pub backend_type: String,
}

// ── Health ──────────────────────────────────────────────────────────────

/// Health check response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Status string.
    pub status: String,
    /// Server version.
    pub version: String,
}

// ── Stash ───────────────────────────────────────────────────────────────

/// Stash entry metadata returned by list and pop endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StashEntryInfo {
    /// Zero-based stash index.
    pub index: usize,
    /// Human-readable description.
    pub message: String,
    /// Commit id of the stashed state (hex).
    pub commit_id: String,
    /// Base commit id at stash time (hex).
    pub base_commit_id: String,
    /// Unix timestamp of stash creation.
    pub timestamp: i64,
}

/// Request body for creating a stash entry.
#[derive(Debug, Clone, Deserialize)]
pub struct StashPushRequest {
    /// Stash description message.
    #[serde(default = "default_stash_message")]
    pub message: String,
}

/// Default stash message.
fn default_stash_message() -> String {
    "WIP".to_owned()
}

// ── Rebase ──────────────────────────────────────────────────────────────

/// Request body for a rebase operation.
#[derive(Debug, Clone, Deserialize)]
pub struct RebaseRequest {
    /// Target branch to rebase onto.
    pub onto: String,
}

/// Rebase operation response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebaseResponse {
    /// Outcome: `"success"` or `"conflict"`.
    pub status: String,
    /// New branch tip commit id (hex).
    pub new_tip: Option<String>,
    /// Number of replayed commits.
    pub replayed_count: usize,
    /// List of (old, new) commit id pairs.
    pub replayed: Vec<(String, String)>,
    /// Conflicted paths (if any).
    pub conflicts: Vec<String>,
}

// ── Cherry-pick ─────────────────────────────────────────────────────────

/// Request body for a cherry-pick operation.
#[derive(Debug, Clone, Deserialize)]
pub struct CherryPickRequest {
    /// Full commit id (hex) to cherry-pick.
    pub commit_id: String,
}

/// Cherry-pick operation response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CherryPickResponse {
    /// The new commit id (hex).
    pub new_commit_id: String,
    /// The original commit id that was cherry-picked.
    pub source_commit_id: String,
}

// ── Revert ──────────────────────────────────────────────────────────────

/// Request body for a revert operation.
#[derive(Debug, Clone, Deserialize)]
pub struct RevertRequest {
    /// Full commit id (hex) to revert.
    pub commit_id: String,
}

/// Revert operation response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevertResponse {
    /// The new commit id (hex) that undoes the reverted commit.
    pub new_commit_id: String,
    /// The original commit id that was reverted.
    pub source_commit_id: String,
}

// ── Garbage Collection ──────────────────────────────────────────────────

/// Garbage collection response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcResponse {
    /// Number of objects before GC.
    pub objects_before: usize,
    /// Number of objects after GC.
    pub objects_after: usize,
    /// Number of objects removed.
    pub objects_removed: usize,
    /// Compressed bytes before GC.
    pub bytes_before: u64,
    /// Compressed bytes after GC.
    pub bytes_after: u64,
    /// Bytes freed.
    pub bytes_freed: u64,
}

// ── Blame ───────────────────────────────────────────────────────────────

/// Blame response containing line-by-line authorship information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameResponse {
    /// Path of the blamed file.
    pub file: String,
    /// Per-line blame attributions.
    pub lines: Vec<BlameLineResponse>,
}

/// A single line in a blame result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameLineResponse {
    /// Commit id that last modified this line (hex).
    pub commit_id: String,
    /// Author name from the commit.
    pub author: String,
    /// Unix timestamp of the commit.
    pub timestamp: i64,
    /// 1-based line number.
    pub line_number: usize,
    /// Line content.
    pub content: String,
}

// ── Search ──────────────────────────────────────────────────────────────

/// Search response containing grep results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    /// The search pattern used.
    pub query: String,
    /// Total number of matching lines returned (after applying `max_results`).
    pub total_matches: usize,
    /// Individual match results.
    pub results: Vec<SearchMatch>,
    /// Whether the result set was truncated by the `max_results` limit.
    ///
    /// When `true`, there are more matches in the repository than what was
    /// returned. Increase `max_results` or refine your query.
    pub truncated: bool,
}

/// A single search match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    /// File path relative to repository root.
    pub path: String,
    /// 1-based line number.
    pub line_number: usize,
    /// Full text of the matching line.
    pub line: String,
}

/// Query parameters for the search endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct SearchQuery {
    /// Search pattern. Treated as a literal string unless `is_regex` is `true`.
    pub q: String,
    /// Whether to search case-insensitively.
    #[serde(default)]
    pub case_insensitive: bool,
    /// When `true`, treat `q` as a regular expression. When `false` (default),
    /// `q` is escaped so special regex characters are matched literally.
    #[serde(default)]
    pub is_regex: bool,
    /// Glob pattern to filter which files are searched (e.g. `"*.py"`,
    /// `"src/**/*.ts"`). When absent, all files are searched.
    #[serde(default)]
    pub file_pattern: Option<String>,
    /// Maximum number of results to return. Defaults to 1000.
    /// Must be between 1 and 10000.
    #[serde(default = "default_max_results")]
    pub max_results: usize,
}

/// Default maximum result count for search queries.
const fn default_max_results() -> usize {
    1000
}

// ── Notes ───────────────────────────────────────────────────────────────

/// Response for a single note.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteResponse {
    /// Commit id the note is attached to (hex).
    pub commit_id: String,
    /// Note message text.
    pub message: String,
}

/// Request body for creating or updating a note.
#[derive(Debug, Clone, Deserialize)]
pub struct SetNoteRequest {
    /// Note message text.
    pub message: String,
}

// ── Reflog ──────────────────────────────────────────────────────────────

/// A single reflog entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflogEntryResponse {
    /// The reference that was updated.
    pub ref_name: String,
    /// Previous value (hex), if any.
    pub old_value: Option<String>,
    /// New value (hex).
    pub new_value: String,
    /// Identity name.
    pub identity_name: String,
    /// Identity email.
    pub identity_email: String,
    /// Unix timestamp of the reflog entry.
    pub timestamp: i64,
    /// Human-readable message describing the change.
    pub message: String,
}

/// Query parameters for the reflog endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct ReflogQuery {
    /// Maximum number of entries to return.
    #[serde(default = "default_reflog_limit")]
    pub limit: usize,
}

/// Default reflog limit.
const fn default_reflog_limit() -> usize {
    50
}

// ── Describe ────────────────────────────────────────────────────────────

/// Response from the describe endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescribeResponse {
    /// Commit id (hex).
    pub commit_id: String,
    /// Human-readable description (e.g., `"v1.0.0"` or `"v1.0.0~3"`).
    pub description: String,
}

// ── Archive ─────────────────────────────────────────────────────────────

/// Query parameters for the archive endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct ArchiveQuery {
    /// Archive format: `"tar"` or `"zip"`.
    #[serde(default = "default_archive_format")]
    pub format: String,
    /// Commit ref to archive (default: HEAD).
    #[serde(default)]
    pub commit: Option<String>,
}

/// Default archive format.
fn default_archive_format() -> String {
    "tar".to_owned()
}

// ── Pull Requests / Branch Comparison ───────────────────────────────────

/// Response from `GET /api/v1/repos/:id/pulls/:branch`.
///
/// Summarises how a branch compares against the repository's default branch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestView {
    /// The branch being compared.
    pub branch: String,
    /// The base branch (default branch, typically `"main"`).
    pub base: String,
    /// Commits that are in `branch` but not reachable from `base`.
    pub commits: Vec<CommitInfo>,
    /// File diff between `base` and `branch` heads.
    pub diff: DiffResponse,
    /// Whether the branch can be merged into `base` without conflicts.
    pub mergeable: bool,
    /// Paths that conflict when attempting a merge (empty when `mergeable`).
    pub conflict_files: Vec<String>,
    /// Number of commits `branch` is ahead of `base`.
    pub ahead_by: usize,
    /// Number of commits `base` is ahead of `branch`.
    pub behind_by: usize,
}

// ── Pull Request Lifecycle ───────────────────────────────────────────────

// Core PR types are defined in `ovc_core::pulls` and stored inside the
// encrypted superblock. Re-export them so existing API consumers (handlers,
// actions, tests) can continue importing from `crate::models`.
pub use ovc_core::pulls::{
    PrCheckResult, PrChecks, PrComment, PrState, PullRequest, Review, ReviewState,
};

/// Summary returned in list responses (same shape as `PullRequest` but
/// without the description to keep list payloads lightweight).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestSummary {
    /// PR number.
    pub number: u64,
    /// Title.
    pub title: String,
    /// Current state.
    pub state: PrState,
    /// Source branch.
    pub source_branch: String,
    /// Target branch.
    pub target_branch: String,
    /// Author.
    pub author: String,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
    /// ISO 8601 last-updated timestamp.
    pub updated_at: String,
}

/// Request body for `POST /api/v1/repos/:id/pulls` (create a PR).
#[derive(Debug, Clone, Deserialize)]
pub struct CreatePullRequestRequest {
    /// PR title.
    pub title: String,
    /// Extended description.
    pub description: Option<String>,
    /// Source branch containing changes.
    pub source_branch: String,
    /// Target branch (defaults to the repository's default branch).
    pub target_branch: Option<String>,
    /// Author display name.
    pub author: Option<String>,
}

/// Query parameters for `GET /api/v1/repos/:id/pulls`.
#[derive(Debug, Clone, Deserialize)]
pub struct ListPullRequestsQuery {
    /// Filter by state: `open`, `closed`, `merged`, or `all`. Defaults to `open`.
    #[serde(default = "default_pr_state_filter")]
    pub state: String,
}

/// Default PR state filter.
fn default_pr_state_filter() -> String {
    "open".to_owned()
}

/// Request body for `PATCH /api/v1/repos/:id/pulls/by-number/:number`.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdatePullRequestRequest {
    /// New title.
    pub title: Option<String>,
    /// New description.
    pub description: Option<String>,
    /// New state: `"open"` or `"closed"` (merge is done via the merge endpoint).
    pub state: Option<String>,
}

/// Request body for `POST /api/v1/repos/:id/pulls/by-number/:number/merge`.
#[derive(Debug, Clone, Deserialize)]
pub struct MergePullRequestRequest {
    /// Merge strategy: `"merge"`, `"squash"`, or `"rebase"`.
    /// Defaults to `"merge"`.
    pub strategy: Option<String>,
    /// If true, bypass CI check failures and force the merge.
    #[serde(default)]
    pub force: bool,
}

/// Response from the merge-PR endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct MergePullRequestResponse {
    /// Outcome: `"merged"`, `"conflict"`, or `"already_up_to_date"`.
    pub status: String,
    /// The merge commit hash (when successful).
    pub commit_id: Option<String>,
    /// Conflicted file paths (when merge fails due to conflicts).
    pub conflicts: Vec<String>,
    /// The updated PR metadata.
    pub pull_request: PullRequest,
}

// ── Reviews & Comments ──────────────────────────────────────────────────
//
// `Review`, `ReviewState`, and `PrComment` are re-exported from
// `ovc_core::pulls` at the top of the Pull Request Lifecycle section.

/// Request body for creating a review.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateReviewRequest {
    /// Review decision.
    pub state: ReviewState,
    /// Review body text.
    pub body: String,
    /// Optional Ed25519 signature (base64).
    pub signature: Option<String>,
}

/// Request body for creating a comment.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateCommentRequest {
    /// Comment body text.
    pub body: String,
    /// File path for inline comments.
    pub file_path: Option<String>,
    /// Line number for inline comments.
    pub line_number: Option<u32>,
}

/// Request body for updating a comment.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateCommentRequest {
    /// Updated body text.
    pub body: String,
}

// ── Access Management ───────────────────────────────────────────────────

/// Request body for granting access to a user.
#[derive(Debug, Clone, Deserialize)]
pub struct GrantAccessRequest {
    /// Raw public key PEM content.
    pub public_key_pem: Option<String>,
    /// Or reference an existing key by fingerprint.
    pub fingerprint: Option<String>,
    /// Role to assign: `"owner"`, `"admin"`, `"write"`, or `"read"`.
    pub role: String,
}

/// Access entry returned in API responses.
#[derive(Debug, Clone, Serialize)]
pub struct UserAccessInfo {
    /// User's key fingerprint.
    pub fingerprint: String,
    /// Assigned role.
    pub role: String,
    /// Display identity.
    pub identity: Option<String>,
    /// When access was granted.
    pub added_at: String,
    /// Who granted access.
    pub added_by: String,
    /// True if this user is the repo creator (self-added owner). The repo
    /// creator's key is the primary encryption key — revoking it would make
    /// the repository unreadable, so the UI should prevent revocation.
    pub is_repo_creator: bool,
}

/// Response listing all users with access.
#[derive(Debug, Clone, Serialize)]
pub struct ListAccessResponse {
    /// List of users with access.
    pub users: Vec<UserAccessInfo>,
}

/// Request body for changing a user's role.
#[derive(Debug, Clone, Deserialize)]
pub struct SetRoleRequest {
    /// New role: `"owner"`, `"admin"`, `"write"`, or `"read"`.
    pub role: String,
}

/// Branch protection configuration request.
#[derive(Debug, Clone, Deserialize)]
pub struct SetBranchProtectionRequest {
    /// Number of required approvals before merge.
    pub required_approvals: Option<u32>,
    /// Whether CI must pass before merge.
    pub require_ci_pass: Option<bool>,
    /// Roles allowed to merge to this branch.
    pub allowed_merge_roles: Option<Vec<String>>,
    /// Roles allowed to push directly to this branch.
    pub allowed_push_roles: Option<Vec<String>>,
}

/// Branch protection info returned in API responses.
#[derive(Debug, Clone, Serialize)]
pub struct BranchProtectionInfo {
    /// Branch name.
    pub branch: String,
    /// Number of required approvals.
    pub required_approvals: u32,
    /// Whether CI must pass.
    pub require_ci_pass: bool,
    /// Allowed merge roles.
    pub allowed_merge_roles: Vec<String>,
    /// Allowed push roles.
    pub allowed_push_roles: Vec<String>,
}

// ── Submodules ──────────────────────────────────────────────────────────

/// Submodule information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmoduleInfo {
    /// Submodule name.
    pub name: String,
    /// Relative path where the submodule is checked out.
    pub path: String,
    /// Remote URL or local path of the submodule source.
    pub url: String,
    /// Name of the `.ovc` file.
    pub ovc_file: String,
    /// Pinned sequence number.
    pub pinned_sequence: u64,
    /// Lifecycle status of the submodule.
    ///
    /// `"configured"` means the submodule is recorded in the repository
    /// configuration but has not been initialised (no nested `.ovc` file
    /// exists yet). The UI should surface this to the user.
    pub status: String,
}

/// Request body for adding a submodule.
#[derive(Debug, Clone, Deserialize)]
pub struct AddSubmoduleRequest {
    /// Submodule name.
    pub name: String,
    /// Relative path where the submodule is checked out.
    pub path: String,
    /// Remote URL or local path.
    pub url: String,
    /// Name of the `.ovc` file.
    ///
    /// When absent, defaults to `"{name}.ovc"`.
    #[serde(default)]
    pub ovc_file: Option<String>,
}
