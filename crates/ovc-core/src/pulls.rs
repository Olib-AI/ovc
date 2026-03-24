//! Pull request storage for OVC repositories.
//!
//! Pull requests, reviews, and comments are stored inside the encrypted
//! superblock so they are protected by the same encryption as the rest
//! of the repository data.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Maximum number of pull requests per repository.
const MAX_PULL_REQUESTS: u64 = 10_000;

/// Pull request state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrState {
    /// The PR is open and accepting changes.
    Open,
    /// The PR was closed without merging.
    Closed,
    /// The PR was merged into the target branch.
    Merged,
}

impl std::fmt::Display for PrState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::Closed => write!(f, "closed"),
            Self::Merged => write!(f, "merged"),
        }
    }
}

/// Review state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    /// The reviewer approves the changes.
    Approved,
    /// The reviewer requests changes before merging.
    ChangesRequested,
    /// General comment without approval decision.
    Commented,
}

impl std::fmt::Display for ReviewState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Approved => write!(f, "approved"),
            Self::ChangesRequested => write!(f, "changes_requested"),
            Self::Commented => write!(f, "commented"),
        }
    }
}

/// CI check results for a pull request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrChecks {
    /// Overall status: "pending", "passing", "failing".
    pub status: String,
    /// Individual check results.
    pub results: Vec<PrCheckResult>,
    /// When the checks were last run.
    pub ran_at: String,
}

/// A single CI check result on a PR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrCheckResult {
    /// Action key name.
    pub name: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Action category.
    pub category: String,
    /// Outcome status.
    pub status: String,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Whether Docker was used for execution.
    pub docker_used: bool,
}

/// A review on a pull request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    /// Auto-assigned review ID.
    pub id: u64,
    /// Reviewer's key fingerprint (or display name for password-auth users).
    pub author: String,
    /// Reviewer's display identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_identity: Option<String>,
    /// Review decision.
    pub state: ReviewState,
    /// Review body text.
    pub body: String,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
    /// Optional Ed25519 signature of the review (base64).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Whether the signature was verified against an authorized key.
    #[serde(default)]
    pub verified: bool,
}

/// A comment on a pull request (general or inline on a file).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrComment {
    /// Auto-assigned comment ID.
    pub id: u64,
    /// Author's key fingerprint or display name.
    pub author: String,
    /// Author's display identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_identity: Option<String>,
    /// Comment body text.
    pub body: String,
    /// File path for inline comments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// Line number for inline comments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_number: Option<u32>,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
    /// ISO 8601 last-updated timestamp.
    pub updated_at: String,
}

/// Persistent pull request metadata stored inside the encrypted superblock.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    /// Auto-assigned PR number.
    pub number: u64,
    /// Human-readable title.
    pub title: String,
    /// Extended description (may be empty).
    pub description: String,
    /// Current state of the PR.
    pub state: PrState,
    /// Branch containing the changes.
    pub source_branch: String,
    /// Branch the changes will be merged into.
    pub target_branch: String,
    /// Author display name.
    pub author: String,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
    /// ISO 8601 last-updated timestamp.
    pub updated_at: String,
    /// ISO 8601 timestamp of when the PR was merged (if applicable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_at: Option<String>,
    /// Commit hash of the merge commit (if applicable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_commit: Option<String>,
    /// CI check results from the last actions run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checks: Option<PrChecks>,
    /// Reviews from authorized users.
    #[serde(default)]
    pub reviews: Vec<Review>,
    /// Comments on the PR (general and inline).
    #[serde(default)]
    pub comments: Vec<PrComment>,
    /// Number of approvals required before merging (0 = no requirement).
    #[serde(default)]
    pub required_approvals: u32,
}

/// Pull request store inside the superblock.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PullRequestStore {
    /// All pull requests, keyed by PR number.
    #[serde(default)]
    pub pull_requests: BTreeMap<u64, PullRequest>,
    /// Next PR number to assign.
    #[serde(default = "default_pr_counter")]
    pub next_number: u64,
}

const fn default_pr_counter() -> u64 {
    1
}

impl PullRequestStore {
    /// Returns the next PR number and increments the counter.
    ///
    /// Enforces [`MAX_PULL_REQUESTS`] against **both** the live collection size
    /// and the monotonic sequence counter, so the limit holds even after
    /// concurrent sync merges or any future delete operation.
    pub fn next_pr_number(&mut self) -> Result<u64, crate::error::CoreError> {
        // Guard on the actual number of stored PRs (live count), not just the
        // sequence counter. This prevents exceeding the limit if PRs were ever
        // bulk-imported via `merge_from` or if the counter and map diverge.
        if self.pull_requests.len() >= usize::try_from(MAX_PULL_REQUESTS).unwrap_or(usize::MAX) {
            return Err(crate::error::CoreError::Config {
                reason: format!("maximum pull request limit reached ({MAX_PULL_REQUESTS})"),
            });
        }
        // Also guard the sequence counter to prevent number wrap-around.
        if self.next_number > MAX_PULL_REQUESTS {
            return Err(crate::error::CoreError::Config {
                reason: format!("maximum pull request number exceeded ({MAX_PULL_REQUESTS})"),
            });
        }
        let number = self.next_number;
        self.next_number += 1;
        Ok(number)
    }

    /// Returns a pull request by number.
    #[must_use]
    pub fn get(&self, number: u64) -> Option<&PullRequest> {
        self.pull_requests.get(&number)
    }

    /// Returns a mutable reference to a pull request by number.
    pub fn get_mut(&mut self, number: u64) -> Option<&mut PullRequest> {
        self.pull_requests.get_mut(&number)
    }

    /// Inserts or updates a pull request.
    pub fn save(&mut self, pr: PullRequest) {
        self.pull_requests.insert(pr.number, pr);
    }

    /// Lists all pull requests, optionally filtered by state.
    #[must_use]
    pub fn list(&self, state_filter: Option<PrState>) -> Vec<&PullRequest> {
        let mut prs: Vec<&PullRequest> = self
            .pull_requests
            .values()
            .filter(|pr| state_filter.is_none_or(|s| pr.state == s))
            .collect();
        prs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        prs
    }

    /// Merges another store into this one.
    ///
    /// New PRs from `other` are imported directly. For PRs that exist in
    /// both stores, reviews and comments are merged by ID to prevent data
    /// loss during concurrent iCloud sync operations. State/title/description
    /// use the most-recently-updated version.
    pub fn merge_from(&mut self, other: &Self) {
        for (number, other_pr) in &other.pull_requests {
            match self.pull_requests.get_mut(number) {
                None => {
                    // New PR from remote — import as-is.
                    self.pull_requests.insert(*number, other_pr.clone());
                }
                Some(local_pr) => {
                    // Merge reviews by ID — import any remote reviews not in local.
                    let local_review_ids: std::collections::HashSet<u64> =
                        local_pr.reviews.iter().map(|r| r.id).collect();
                    for review in &other_pr.reviews {
                        if !local_review_ids.contains(&review.id) {
                            local_pr.reviews.push(review.clone());
                        }
                    }
                    // Keep reviews sorted by ID for stable ordering.
                    local_pr.reviews.sort_by_key(|r| r.id);

                    // Merge comments by ID — import any remote comments not in local.
                    let local_comment_ids: std::collections::HashSet<u64> =
                        local_pr.comments.iter().map(|c| c.id).collect();
                    for comment in &other_pr.comments {
                        if !local_comment_ids.contains(&comment.id) {
                            local_pr.comments.push(comment.clone());
                        }
                    }
                    local_pr.comments.sort_by_key(|c| c.id);

                    // For metadata (title, description, state), use the more
                    // recently updated version to reduce surprise.
                    if other_pr.updated_at > local_pr.updated_at {
                        local_pr.title.clone_from(&other_pr.title);
                        local_pr.description.clone_from(&other_pr.description);
                        local_pr.state = other_pr.state;
                        local_pr.updated_at.clone_from(&other_pr.updated_at);
                        local_pr.merged_at.clone_from(&other_pr.merged_at);
                        local_pr.merge_commit.clone_from(&other_pr.merge_commit);
                        local_pr.checks.clone_from(&other_pr.checks);
                    }
                }
            }
        }
        if other.next_number > self.next_number {
            self.next_number = other.next_number;
        }
    }
}
