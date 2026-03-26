//! Intelligent context building for LLM requests.
//!
//! The [`ContextBuilder`] assembles chat messages tailored to each LLM feature,
//! applying token budgets, filtering irrelevant files, and using a multi-pass
//! map-reduce strategy for large diffs that don't fit in a single request.
//!
//! ## Multi-pass pipeline
//!
//! For diffs that exceed the token budget:
//! 1. **Partition** — files are grouped into batches that each fit the context.
//! 2. **Map** — each batch is sent to the LLM for a short bullet-point summary.
//! 3. **Reduce** — all summaries are combined in a final request that generates
//!    the commit message / review / explanation.

use std::fmt::Write;

use crate::client::ChatMessage;
use crate::prompts;

/// File patterns to strip from diffs before sending to the LLM.
const FILTERED_PATTERNS: &[&str] = &[
    ".lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    ".min.js",
    ".min.css",
    ".map",
    ".pb.go",
    "_generated.rs",
    "_generated.go",
    ".d.ts",
];

/// Path prefixes to strip from diffs.
const FILTERED_PREFIXES: &[&str] = &["dist/", "build/", "vendor/", "node_modules/", ".git/"];

// ── Structured diff input ───────────────────────────────────────────────

/// A lightweight representation of a single file's diff, passed into the
/// context builder from the API layer.  This avoids coupling `ovc-llm` to
/// `ovc-api`'s model types.
#[derive(Debug, Clone)]
pub struct FileDiffEntry {
    /// File path (e.g. `src/main.rs`).
    pub path: String,
    /// Change status: `"added"`, `"modified"`, `"deleted"`.
    pub status: String,
    /// Lines added.
    pub additions: u64,
    /// Lines deleted.
    pub deletions: u64,
    /// Full unified-diff text for this file (header + hunks).
    /// Empty when the file was filtered or is binary.
    pub diff_text: String,
}

// ── Multi-pass types ────────────────────────────────────────────────────

/// A batch of file diffs that fits within the token budget for a single
/// LLM request, used during the map phase of multi-pass processing.
#[derive(Debug, Clone)]
pub struct DiffBatch {
    /// Chat messages for this batch (system + user with packed diffs).
    pub messages: Vec<ChatMessage>,
    /// Number of files in this batch.
    pub file_count: usize,
    /// File paths included (for progress reporting to the frontend).
    pub paths: Vec<String>,
}

/// Describes whether a set of diffs needs multi-pass processing.
#[derive(Debug)]
pub enum PassPlan {
    /// Everything fits in one request — use these messages directly.
    SinglePass(Vec<ChatMessage>),
    /// Diffs are split into batches for map-reduce processing.
    MultiPass {
        /// Batches for the map phase (summarisation requests).
        batches: Vec<DiffBatch>,
        /// Compact manifest of ALL files (including filtered) so the
        /// reduce prompt knows the full scope of changes.
        file_manifest: Vec<FileDiffEntry>,
    },
}

// ── Builder ─────────────────────────────────────────────────────────────

/// Builds context messages for LLM requests, managing token budgets and
/// filtering irrelevant content.
///
/// For small diffs, builds a single request. For large diffs, produces a
/// [`PassPlan::MultiPass`] that the caller uses to run the map-reduce
/// pipeline.
pub struct ContextBuilder {
    max_tokens: usize,
}

impl ContextBuilder {
    /// Creates a new context builder with the given token budget.
    #[must_use]
    pub const fn new(max_tokens: usize) -> Self {
        Self { max_tokens }
    }

    /// Estimates the token count for a string.
    ///
    /// Uses ~2.5 chars per token, which is conservative for code-heavy
    /// content.  Real-world measurements with Qwen/Llama tokenizers on
    /// unified diffs showed ~2.2 chars/token; 2.5 gives a small safety
    /// margin while avoiding gross over-allocation.
    #[must_use]
    fn estimate_tokens(text: &str) -> usize {
        // len * 2 / 5 = len / 2.5
        (text.len() * 2 / 5).max(1)
    }

    /// Returns the available token budget after reserving `reserved` tokens,
    /// converted to an approximate character count.
    ///
    /// Applies an additional 15% safety margin so that estimation errors
    /// don't push the request over the model's actual context window.
    const fn available_chars(&self, reserved_tokens: usize) -> usize {
        let usable = self.max_tokens.saturating_sub(reserved_tokens);
        // 85% safety margin: usable * 85 / 100, then convert to chars (* 5 / 2)
        usable * 85 / 100 * 5 / 2
    }

    // ── Structured diff packing ─────────────────────────────────────────

    /// Takes structured per-file diffs and packs them into a single string
    /// that fits within the token budget.
    ///
    /// Files that pass filtering are sorted by priority, and a greedy
    /// algorithm includes full diffs until the budget is exhausted.
    /// Remaining files are appended as one-line stat summaries.
    fn pack_diff(&self, files: &[FileDiffEntry], reserved_tokens: usize) -> String {
        let max_chars = self.available_chars(reserved_tokens);

        // Filter and sort.
        let mut eligible: Vec<&FileDiffEntry> = files
            .iter()
            .filter(|f| !should_filter_path(&f.path))
            .collect();
        eligible.sort_by_key(|a| file_priority(a));

        let mut out = String::new();
        let mut remaining_chars = max_chars;
        let mut summarised: Vec<&FileDiffEntry> = Vec::new();

        for file in &eligible {
            if file.diff_text.is_empty() || is_binary_diff(&file.diff_text) {
                // No diff content or binary — always summarise.
                summarised.push(file);
                continue;
            }

            let file_len = file.diff_text.len();
            if file_len <= remaining_chars {
                out.push_str(&file.diff_text);
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                remaining_chars -= file_len;
            } else {
                summarised.push(file);
            }
        }

        // Append stat summaries for files that didn't fit.
        if !summarised.is_empty() {
            let header = format!(
                "\n[{} file(s) summarised to fit context limit:]\n",
                summarised.len()
            );
            if header.len() < remaining_chars {
                out.push_str(&header);
                remaining_chars -= header.len();

                for file in &summarised {
                    let line = format!(
                        "  {} {} (+{}/-{})\n",
                        file.path, file.status, file.additions, file.deletions
                    );
                    if line.len() <= remaining_chars {
                        out.push_str(&line);
                        remaining_chars -= line.len();
                    } else {
                        let _ = writeln!(out, "  ... and more files (budget exhausted)");
                        break;
                    }
                }
            }
        }

        // Also count filtered-out files so the LLM knows they exist.
        let filtered_count = files.len() - eligible.len();
        if filtered_count > 0 {
            let note = format!(
                "\n[{filtered_count} file(s) excluded: lock files, generated code, build artefacts]\n"
            );
            if note.len() <= remaining_chars {
                out.push_str(&note);
            }
        }

        out
    }

    // ── Multi-pass pipeline ───────────────────────────────────────────

    /// Plans how to process a set of file diffs for commit message generation.
    ///
    /// Returns [`PassPlan::SinglePass`] if everything fits in one request,
    /// or [`PassPlan::MultiPass`] with batches for map-reduce processing.
    #[must_use]
    pub fn plan_commit_message(&self, files: &[FileDiffEntry], languages: &[String]) -> PassPlan {
        self.plan_diff(
            files,
            prompts::COMMIT_MSG_SYSTEM,
            Self::for_commit_message_structured,
            languages,
        )
    }

    /// Plans how to process diffs for PR review.
    #[must_use]
    pub fn plan_pr_review(
        &self,
        files: &[FileDiffEntry],
        pr_title: &str,
        pr_description: &str,
    ) -> PassPlan {
        let title = pr_title.to_owned();
        let desc = pr_description.to_owned();
        self.plan_diff(
            files,
            prompts::PR_REVIEW_SYSTEM,
            move |ctx, f, _| ctx.for_pr_review_structured(f, &title, &desc),
            &[],
        )
    }

    /// Plans how to process diffs for explanation.
    #[must_use]
    pub fn plan_explain_diff(&self, files: &[FileDiffEntry], languages: &[String]) -> PassPlan {
        self.plan_diff(
            files,
            prompts::EXPLAIN_DIFF_SYSTEM,
            Self::for_explain_diff_structured,
            languages,
        )
    }

    /// Generic planner: checks if files fit in one pass, otherwise partitions.
    fn plan_diff(
        &self,
        files: &[FileDiffEntry],
        system_prompt: &str,
        single_pass_fn: impl FnOnce(&Self, &[FileDiffEntry], &[String]) -> Vec<ChatMessage>,
        languages: &[String],
    ) -> PassPlan {
        let eligible: Vec<&FileDiffEntry> = files
            .iter()
            .filter(|f| !should_filter_path(&f.path))
            .collect();

        let system_tokens = Self::estimate_tokens(system_prompt) + 50;
        let max_chars = self.available_chars(system_tokens);
        let total_diff_chars: usize = eligible.iter().map(|f| f.diff_text.len()).sum();

        if total_diff_chars <= max_chars {
            return PassPlan::SinglePass(single_pass_fn(self, files, languages));
        }

        let batches = self.partition_into_batches(&eligible);
        // Manifest includes ALL files (even filtered) so the reduce prompt
        // shows the full picture.
        let file_manifest: Vec<FileDiffEntry> = files
            .iter()
            .map(|f| FileDiffEntry {
                path: f.path.clone(),
                status: f.status.clone(),
                additions: f.additions,
                deletions: f.deletions,
                diff_text: String::new(), // Don't carry diff text in manifest.
            })
            .collect();

        PassPlan::MultiPass {
            batches,
            file_manifest,
        }
    }

    /// Builds the final reduce-phase messages from batch summaries.
    ///
    /// Called after all map-phase batches have been summarised by the LLM.
    /// The `file_manifest` provides a one-line-per-file overview so the LLM
    /// knows the full scope even if summaries are condensed.
    #[must_use]
    pub fn for_commit_message_from_summaries(
        &self,
        summaries: &[String],
        file_manifest: &[FileDiffEntry],
        languages: &[String],
    ) -> Vec<ChatMessage> {
        let system_tokens = Self::estimate_tokens(prompts::COMMIT_MSG_SYSTEM) + 50;

        let mut user_content = String::new();
        if !languages.is_empty() {
            let _ = write!(
                user_content,
                "Project languages: {}\n\n",
                languages.join(", ")
            );
        }

        // File manifest — compact one-liner per file.
        let _ = writeln!(
            user_content,
            "Changed files ({} total):",
            file_manifest.len()
        );
        for f in file_manifest {
            let _ = writeln!(
                user_content,
                "  {} {} (+{}/-{})",
                f.path, f.status, f.additions, f.deletions
            );
        }

        // Summaries from map phase.
        let _ = write!(
            user_content,
            "\nDetailed summaries ({} groups):\n\n",
            summaries.len()
        );
        for (i, summary) in summaries.iter().enumerate() {
            if i > 0 {
                user_content.push_str("\n---\n\n");
            }
            user_content.push_str(summary.trim());
            user_content.push('\n');
        }
        user_content.push_str("\nGenerate a commit message for all these changes combined.");

        // Safety truncation (summaries should be small, but guard against it).
        let truncated = self.truncate_to_budget(&user_content, system_tokens);

        vec![
            ChatMessage::system(prompts::COMMIT_MSG_SYSTEM),
            ChatMessage::user(truncated),
        ]
    }

    /// Generic reduce-phase builder that works for any feature.
    ///
    /// Takes batch summaries + file manifest and a custom system prompt +
    /// final instruction. Used by PR review, diff explanation, etc.
    #[must_use]
    pub fn reduce_from_summaries(
        &self,
        summaries: &[String],
        file_manifest: &[FileDiffEntry],
        languages: &[String],
        system_prompt: &str,
        final_instruction: &str,
    ) -> Vec<ChatMessage> {
        let system_tokens = Self::estimate_tokens(system_prompt) + 50;

        let mut user_content = String::new();
        if !languages.is_empty() {
            let _ = write!(
                user_content,
                "Project languages: {}\n\n",
                languages.join(", ")
            );
        }

        let _ = writeln!(
            user_content,
            "Changed files ({} total):",
            file_manifest.len()
        );
        for f in file_manifest {
            let _ = writeln!(
                user_content,
                "  {} {} (+{}/-{})",
                f.path, f.status, f.additions, f.deletions
            );
        }

        let _ = write!(
            user_content,
            "\nDetailed summaries ({} groups):\n\n",
            summaries.len()
        );
        for (i, summary) in summaries.iter().enumerate() {
            if i > 0 {
                user_content.push_str("\n---\n\n");
            }
            user_content.push_str(summary.trim());
            user_content.push('\n');
        }
        let _ = write!(user_content, "\n{final_instruction}");

        let truncated = self.truncate_to_budget(&user_content, system_tokens);

        vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(truncated),
        ]
    }

    /// Partitions filtered files into batches that each fit within the token
    /// budget for a single summarisation request.
    fn partition_into_batches(&self, files: &[&FileDiffEntry]) -> Vec<DiffBatch> {
        let batch_system_tokens = Self::estimate_tokens(prompts::BATCH_SUMMARY_SYSTEM) + 30;
        let max_chars = self.available_chars(batch_system_tokens);

        let mut batches = Vec::new();
        let mut current_text = String::new();
        let mut current_paths = Vec::new();
        let mut current_chars = 0usize;

        // Sort by priority (source code first, small changes first).
        let mut sorted: Vec<&FileDiffEntry> = files.to_vec();
        sorted.sort_by_key(|a| file_priority(a));

        for file in sorted {
            // Skip binary files — they contain non-UTF-8 bytes that would
            // panic on string slicing and are useless to the LLM anyway.
            if is_binary_diff(&file.diff_text) {
                // Include as a stat-only line.
                let stat = format!(
                    "{} {} [binary] (+{}/-{})\n",
                    file.path, file.status, file.additions, file.deletions
                );
                if current_chars + stat.len() > max_chars && !current_paths.is_empty() {
                    batches.push(self.build_batch(&current_text, &current_paths));
                    current_text.clear();
                    current_paths.clear();
                    current_chars = 0;
                }
                current_text.push_str(&stat);
                current_chars += stat.len();
                current_paths.push(file.path.clone());
                continue;
            }

            let text = if file.diff_text.is_empty() {
                format!(
                    "{} {} (+{}/-{})\n",
                    file.path, file.status, file.additions, file.deletions
                )
            } else {
                file.diff_text.clone()
            };

            if current_chars + text.len() > max_chars && !current_paths.is_empty() {
                // Flush current batch.
                batches.push(self.build_batch(&current_text, &current_paths));
                current_text.clear();
                current_paths.clear();
                current_chars = 0;
            }

            // If a single file exceeds the batch budget, truncate it.
            if text.len() > max_chars {
                let cut = safe_truncate_pos(&text, max_chars);
                current_text.push_str(&text[..cut]);
                current_text.push_str("\n[... file truncated ...]\n");
                current_chars += cut + 25;
            } else {
                current_text.push_str(&text);
                current_chars += text.len();
            }
            current_paths.push(file.path.clone());
        }

        // Flush last batch.
        if !current_paths.is_empty() {
            batches.push(self.build_batch(&current_text, &current_paths));
        }

        batches
    }

    /// Creates a [`DiffBatch`] with the summarisation system prompt.
    #[allow(clippy::unused_self)]
    fn build_batch(&self, diff_text: &str, paths: &[String]) -> DiffBatch {
        let user_content = format!(
            "Summarise the changes in these {} file(s):\n\n{}",
            paths.len(),
            diff_text
        );
        DiffBatch {
            messages: vec![
                ChatMessage::system(prompts::BATCH_SUMMARY_SYSTEM),
                ChatMessage::user(user_content),
            ],
            file_count: paths.len(),
            paths: paths.to_vec(),
        }
    }

    // ── Legacy text-based diff support ──────────────────────────────────

    /// Filters diff content to remove irrelevant files (lock files, minified
    /// code, generated code, binary content).
    #[must_use]
    pub fn filter_diff(diff: &str) -> String {
        let mut result = String::with_capacity(diff.len());
        let mut skip_file = false;

        for line in diff.lines() {
            if line.starts_with("diff --git")
                || line.starts_with("--- ")
                || line.starts_with("+++ ")
            {
                if line.starts_with("diff --git") || line.starts_with("+++ ") {
                    skip_file = should_filter_path(line);
                }
                if !skip_file {
                    result.push_str(line);
                    result.push('\n');
                }
                continue;
            }

            if skip_file {
                continue;
            }

            if line.contains('\0') {
                skip_file = true;
                continue;
            }
            result.push_str(line);
            result.push('\n');
        }

        result
    }

    /// Truncates text to fit within the token budget, appending a notice if
    /// truncated.  Uses [`available_chars`] which includes the safety margin.
    fn truncate_to_budget(&self, text: &str, reserved_tokens: usize) -> String {
        let max_chars = self.available_chars(reserved_tokens);

        if text.len() <= max_chars {
            return text.to_owned();
        }

        let byte_pos = safe_truncate_pos(text, max_chars);
        let cut_point = text[..byte_pos].rfind('\n').unwrap_or(byte_pos);
        let remaining_lines = text[cut_point..].lines().count();

        format!(
            "{}\n\n[... {remaining_lines} more lines truncated to fit context limit ...]",
            &text[..cut_point]
        )
    }

    // ── Public API: structured diff input (preferred) ───────────────────

    /// Builds context messages for commit message generation from structured
    /// per-file diffs.
    #[must_use]
    pub fn for_commit_message_structured(
        &self,
        files: &[FileDiffEntry],
        languages: &[String],
    ) -> Vec<ChatMessage> {
        let system_tokens = Self::estimate_tokens(prompts::COMMIT_MSG_SYSTEM) + 50;
        let diff_text = self.pack_diff(files, system_tokens);

        if diff_text.trim().is_empty() {
            // Fallback: nothing survived filtering — produce a minimal summary.
            return self.for_commit_message(&minimal_stat_summary(files), languages);
        }

        let mut user_content = String::new();
        if !languages.is_empty() {
            let _ = write!(
                user_content,
                "Project languages: {}\n\n",
                languages.join(", ")
            );
        }
        user_content.push_str("Generate a commit message for these staged changes:\n\n");
        user_content.push_str(&diff_text);

        vec![
            ChatMessage::system(prompts::COMMIT_MSG_SYSTEM),
            ChatMessage::user(user_content),
        ]
    }

    /// Builds context messages for PR code review from structured diffs.
    #[must_use]
    pub fn for_pr_review_structured(
        &self,
        files: &[FileDiffEntry],
        pr_title: &str,
        pr_description: &str,
    ) -> Vec<ChatMessage> {
        let system_tokens = Self::estimate_tokens(prompts::PR_REVIEW_SYSTEM) + 100;
        let diff_text = self.pack_diff(files, system_tokens);

        let user_content = format!(
            "PR Title: {pr_title}\n\
             PR Description: {pr_description}\n\n\
             Review the following diff:\n\n\
             {diff_text}"
        );

        vec![
            ChatMessage::system(prompts::PR_REVIEW_SYSTEM),
            ChatMessage::user(user_content),
        ]
    }

    /// Builds context messages for explaining a diff from structured diffs.
    #[must_use]
    pub fn for_explain_diff_structured(
        &self,
        files: &[FileDiffEntry],
        languages: &[String],
    ) -> Vec<ChatMessage> {
        let system_tokens = Self::estimate_tokens(prompts::EXPLAIN_DIFF_SYSTEM) + 50;
        let diff_text = self.pack_diff(files, system_tokens);

        let mut user_content = String::new();
        if !languages.is_empty() {
            let _ = write!(
                user_content,
                "Project languages: {}\n\n",
                languages.join(", ")
            );
        }
        user_content.push_str("Explain the following changes:\n\n");
        user_content.push_str(&diff_text);

        vec![
            ChatMessage::system(prompts::EXPLAIN_DIFF_SYSTEM),
            ChatMessage::user(user_content),
        ]
    }

    // ── Public API: plain text input (legacy / simple callers) ──────────

    /// Builds context messages for commit message generation.
    #[must_use]
    pub fn for_commit_message(&self, staged_diff: &str, languages: &[String]) -> Vec<ChatMessage> {
        let filtered = Self::filter_diff(staged_diff);
        let system_tokens = Self::estimate_tokens(prompts::COMMIT_MSG_SYSTEM) + 50;
        let diff_text = self.truncate_to_budget(&filtered, system_tokens);

        let mut user_content = String::new();
        if !languages.is_empty() {
            let _ = write!(
                user_content,
                "Project languages: {}\n\n",
                languages.join(", ")
            );
        }
        user_content.push_str("Generate a commit message for these staged changes:\n\n");
        user_content.push_str(&diff_text);

        vec![
            ChatMessage::system(prompts::COMMIT_MSG_SYSTEM),
            ChatMessage::user(user_content),
        ]
    }

    /// Builds context messages for PR code review.
    #[must_use]
    pub fn for_pr_review(
        &self,
        diff: &str,
        pr_title: &str,
        pr_description: &str,
    ) -> Vec<ChatMessage> {
        let filtered = Self::filter_diff(diff);
        let system_tokens = Self::estimate_tokens(prompts::PR_REVIEW_SYSTEM) + 100;
        let diff_text = self.truncate_to_budget(&filtered, system_tokens);

        let user_content = format!(
            "PR Title: {pr_title}\n\
             PR Description: {pr_description}\n\n\
             Review the following diff:\n\n\
             {diff_text}"
        );

        vec![
            ChatMessage::system(prompts::PR_REVIEW_SYSTEM),
            ChatMessage::user(user_content),
        ]
    }

    /// Builds context messages for explaining a diff.
    #[must_use]
    pub fn for_explain_diff(&self, diff: &str, languages: &[String]) -> Vec<ChatMessage> {
        let filtered = Self::filter_diff(diff);
        let system_tokens = Self::estimate_tokens(prompts::EXPLAIN_DIFF_SYSTEM) + 50;
        let diff_text = self.truncate_to_budget(&filtered, system_tokens);

        let mut user_content = String::new();
        if !languages.is_empty() {
            let _ = write!(
                user_content,
                "Project languages: {}\n\n",
                languages.join(", ")
            );
        }
        user_content.push_str("Explain the following changes:\n\n");
        user_content.push_str(&diff_text);

        vec![
            ChatMessage::system(prompts::EXPLAIN_DIFF_SYSTEM),
            ChatMessage::user(user_content),
        ]
    }

    /// Builds context messages for PR description generation.
    #[must_use]
    pub fn for_pr_description(
        &self,
        commit_messages: &[String],
        diff_summary: &str,
        languages: &[String],
    ) -> Vec<ChatMessage> {
        let system_tokens = Self::estimate_tokens(prompts::PR_DESC_SYSTEM) + 100;

        let commits_text = commit_messages
            .iter()
            .map(|m| format!("- {m}"))
            .collect::<Vec<_>>()
            .join("\n");

        let diff_text = self.truncate_to_budget(diff_summary, system_tokens);

        let mut user_content = String::new();
        if !languages.is_empty() {
            let _ = write!(
                user_content,
                "Project languages: {}\n\n",
                languages.join(", ")
            );
        }
        let _ = write!(
            user_content,
            "Commits:\n{commits_text}\n\n\
             Diff summary:\n{diff_text}\n\n\
             Generate a PR description."
        );

        vec![
            ChatMessage::system(prompts::PR_DESC_SYSTEM),
            ChatMessage::user(user_content),
        ]
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Produces a stats-only summary when all diff content was filtered out.
fn minimal_stat_summary(files: &[FileDiffEntry]) -> String {
    let mut out = String::new();
    for f in files {
        let _ = writeln!(
            out,
            "{} {} (+{}/-{})",
            f.path, f.status, f.additions, f.deletions
        );
    }
    out
}

/// Lower number = higher priority.  Source code files with small, focused
/// changes are prioritised so the LLM sees the most meaningful context first.
fn file_priority(f: &FileDiffEntry) -> u32 {
    let ext_priority = match path_extension(&f.path) {
        // Source code — highest priority
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "kt" | "swift" | "c"
        | "cpp" | "h" | "hpp" | "cs" | "rb" | "ex" | "exs" => 0,
        // Config / build
        "toml" | "yaml" | "yml" | "json" | "xml" | "gradle" | "cmake" => 2,
        // Docs
        "md" | "txt" | "rst" => 3,
        // Other
        _ => 1,
    };
    let size = f.additions + f.deletions;
    // Smaller diffs come first within the same priority tier so we can
    // fit more files before the budget runs out.
    let size_bucket = match size {
        0..=50 => 0,
        51..=200 => 1,
        201..=500 => 2,
        _ => 3,
    };
    ext_priority * 10 + size_bucket
}

/// Extracts the file extension from a path, lowercased.
fn path_extension(path: &str) -> &str {
    path.rsplit_once('.').map_or("", |(_, ext)| ext)
}

/// Checks whether a path should be filtered out of LLM context.
fn should_filter_path(path: &str) -> bool {
    // Handle both raw paths and diff header prefixes.
    let path = path
        .strip_prefix("+++ b/")
        .or_else(|| path.strip_prefix("+++ "))
        .or_else(|| path.split(" b/").nth(1))
        .unwrap_or(path);

    for pattern in FILTERED_PATTERNS {
        if path.ends_with(pattern) {
            return true;
        }
    }

    for prefix in FILTERED_PREFIXES {
        if path.contains(prefix) {
            return true;
        }
    }

    false
}

/// Detects whether a diff text contains binary content (non-UTF-8 safe bytes,
/// PNG/JPEG headers, null bytes, etc.).
fn is_binary_diff(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    // Check first ~1KB for binary indicators (safe char boundary).
    let end = safe_truncate_pos(text, 1024);
    let sample = &text[..end];
    sample.contains('\0')
        || sample.contains('\u{FFFD}')
        || sample.contains("\u{89}PNG")
        || sample.contains("GIF8")
        || sample.contains("\u{FF}\u{D8}\u{FF}") // JPEG
}

/// Finds a safe byte position to truncate a string at, ensuring we don't
/// split a multi-byte UTF-8 character.  Walks backwards from `max_bytes`
/// to the nearest char boundary.
const fn safe_truncate_pos(text: &str, max_bytes: usize) -> usize {
    if max_bytes >= text.len() {
        return text.len();
    }
    // Walk backwards to find a char boundary.
    let mut pos = max_bytes;
    while pos > 0 && !text.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_removes_lock_files() {
        let diff = "\
diff --git a/Cargo.lock b/Cargo.lock
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -1,3 +1,3 @@
-old lock content
+new lock content
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,3 @@
-fn old() {}
+fn new() {}
";
        let filtered = ContextBuilder::filter_diff(diff);
        assert!(!filtered.contains("lock content"));
        assert!(filtered.contains("fn new()"));
    }

    #[test]
    fn filter_removes_dist_files() {
        let diff = "\
diff --git a/dist/bundle.js b/dist/bundle.js
+++ b/dist/bundle.js
@@ -1 +1 @@
-old bundle
+new bundle
diff --git a/src/app.ts b/src/app.ts
+++ b/src/app.ts
@@ -1 +1 @@
-old app
+new app
";
        let filtered = ContextBuilder::filter_diff(diff);
        assert!(!filtered.contains("bundle"));
        assert!(filtered.contains("app"));
    }

    #[test]
    fn truncation_respects_budget() {
        let builder = ContextBuilder::new(100);
        // 100 tokens budget, 50 reserved → 50 usable tokens.
        // 50 * 85% safety = 42 tokens → 42 * 2.5 = 105 chars available.
        let long_text = "a".repeat(500);
        let result = builder.truncate_to_budget(&long_text, 50);
        assert!(result.len() < 500);
        assert!(result.contains("truncated"));
    }

    #[test]
    fn commit_message_context_includes_languages() {
        let builder = ContextBuilder::new(8192);
        let messages = builder.for_commit_message(
            "+fn hello() {}",
            &["Rust".to_owned(), "TypeScript".to_owned()],
        );
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "system");
        assert!(messages[1].content.contains("Rust, TypeScript"));
    }

    #[test]
    fn pack_diff_fits_small_diff() {
        let builder = ContextBuilder::new(8192);
        let files = vec![FileDiffEntry {
            path: "src/main.rs".into(),
            status: "modified".into(),
            additions: 2,
            deletions: 1,
            diff_text: "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1,2 @@\n-old\n+new\n+added\n"
                .into(),
        }];
        let result = builder.pack_diff(&files, 100);
        assert!(result.contains("+new"));
        assert!(!result.contains("summarised"));
    }

    #[test]
    fn pack_diff_summarises_overflow() {
        // Tiny budget: only ~175 chars available after 50 reserved tokens.
        let builder = ContextBuilder::new(100);
        let big_diff = "x\n".repeat(200);
        let files = vec![
            FileDiffEntry {
                path: "small.rs".into(),
                status: "modified".into(),
                additions: 1,
                deletions: 0,
                diff_text: "+one line\n".into(),
            },
            FileDiffEntry {
                path: "huge.rs".into(),
                status: "modified".into(),
                additions: 200,
                deletions: 0,
                diff_text: big_diff,
            },
        ];
        let result = builder.pack_diff(&files, 50);
        assert!(result.contains("+one line"));
        assert!(result.contains("summarised"));
        assert!(result.contains("huge.rs"));
    }

    #[test]
    fn pack_diff_filters_lock_files() {
        let builder = ContextBuilder::new(8192);
        let files = vec![
            FileDiffEntry {
                path: "Cargo.lock".into(),
                status: "modified".into(),
                additions: 500,
                deletions: 400,
                diff_text: "lots of lock content".into(),
            },
            FileDiffEntry {
                path: "src/lib.rs".into(),
                status: "modified".into(),
                additions: 3,
                deletions: 1,
                diff_text: "+real code\n".into(),
            },
        ];
        let result = builder.pack_diff(&files, 100);
        assert!(result.contains("+real code"));
        assert!(!result.contains("lock content"));
        assert!(result.contains("excluded"));
    }

    #[test]
    fn structured_commit_msg_works() {
        let builder = ContextBuilder::new(8192);
        let files = vec![FileDiffEntry {
            path: "src/main.rs".into(),
            status: "modified".into(),
            additions: 1,
            deletions: 1,
            diff_text: "-old\n+new\n".into(),
        }];
        let messages = builder.for_commit_message_structured(&files, &["Rust".into()]);
        assert_eq!(messages.len(), 2);
        assert!(messages[1].content.contains("+new"));
    }

    #[test]
    fn plan_small_diff_is_single_pass() {
        let builder = ContextBuilder::new(8192);
        let files = vec![FileDiffEntry {
            path: "src/main.rs".into(),
            status: "modified".into(),
            additions: 3,
            deletions: 1,
            diff_text: "-old\n+new line 1\n+new line 2\n+new line 3\n".into(),
        }];
        let plan = builder.plan_commit_message(&files, &[]);
        assert!(matches!(plan, PassPlan::SinglePass(_)));
    }

    #[test]
    fn plan_large_diff_is_multi_pass() {
        // Tiny budget forces multi-pass even with a few files.
        let builder = ContextBuilder::new(200);
        let files: Vec<FileDiffEntry> = (0..10)
            .map(|i| FileDiffEntry {
                path: format!("src/mod{i}.rs"),
                status: "modified".into(),
                additions: 50,
                deletions: 20,
                diff_text: format!("+fn func_{i}() {{}}\n").repeat(50),
            })
            .collect();
        let plan = builder.plan_commit_message(&files, &["Rust".into()]);
        match &plan {
            PassPlan::MultiPass {
                batches,
                file_manifest,
            } => {
                // Every file must appear in exactly one batch.
                let files_in_batches: usize = batches.iter().map(|b| b.file_count).sum();
                assert_eq!(files_in_batches, 10, "all files must be in a batch");

                // Manifest must list all files.
                assert_eq!(file_manifest.len(), 10);

                // Each batch message must contain the summarisation prompt.
                for batch in batches {
                    assert_eq!(batch.messages[0].role, "system");
                    assert!(batch.messages[0].content.contains("Summarise"));
                }
            }
            PassPlan::SinglePass(_) => panic!("expected MultiPass for large diff"),
        }
    }

    #[test]
    fn plan_multi_pass_includes_lock_files_in_manifest() {
        let builder = ContextBuilder::new(200);
        let files = vec![
            FileDiffEntry {
                path: "src/main.rs".into(),
                status: "modified".into(),
                additions: 50,
                deletions: 20,
                diff_text: "+code\n".repeat(100),
            },
            FileDiffEntry {
                path: "Cargo.lock".into(),
                status: "modified".into(),
                additions: 500,
                deletions: 400,
                diff_text: "lock stuff".repeat(100),
            },
        ];
        let plan = builder.plan_commit_message(&files, &[]);
        match plan {
            PassPlan::MultiPass { file_manifest, .. } => {
                // Lock file must be in manifest even though it's filtered from batches.
                assert!(file_manifest.iter().any(|f| f.path == "Cargo.lock"));
                assert_eq!(file_manifest.len(), 2);
            }
            PassPlan::SinglePass(_) => panic!("expected MultiPass"),
        }
    }

    #[test]
    fn reduce_from_summaries_includes_manifest() {
        let builder = ContextBuilder::new(8192);
        let summaries = vec![
            "- Updated auth logic in auth.rs".into(),
            "- Added new endpoint in api.rs".into(),
        ];
        let manifest = vec![
            FileDiffEntry {
                path: "src/auth.rs".into(),
                status: "modified".into(),
                additions: 10,
                deletions: 5,
                diff_text: String::new(),
            },
            FileDiffEntry {
                path: "src/api.rs".into(),
                status: "modified".into(),
                additions: 20,
                deletions: 0,
                diff_text: String::new(),
            },
            FileDiffEntry {
                path: "Cargo.lock".into(),
                status: "modified".into(),
                additions: 100,
                deletions: 80,
                diff_text: String::new(),
            },
        ];
        let messages =
            builder.for_commit_message_from_summaries(&summaries, &manifest, &["Rust".into()]);
        let content = &messages[1].content;
        // Must contain the file manifest.
        assert!(content.contains("src/auth.rs"));
        assert!(content.contains("src/api.rs"));
        assert!(content.contains("Cargo.lock"));
        assert!(content.contains("3 total"));
        // Must contain the summaries.
        assert!(content.contains("auth logic"));
        assert!(content.contains("new endpoint"));
        // Must contain the final instruction.
        assert!(content.contains("commit message"));
    }
}
