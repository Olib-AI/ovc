//! YAML-based actions configuration parsing and validation.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::ActionsResult;

/// Top-level actions configuration, typically read from `.ovc/actions.yml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActionsConfig {
    /// Default values applied to every action unless overridden.
    #[serde(default)]
    pub defaults: ActionDefaults,
    /// Named action definitions.
    #[serde(default)]
    pub actions: BTreeMap<String, ActionDefinition>,
    /// Branch protection rules.
    #[serde(default)]
    pub branch_protection: Vec<BranchProtectionRule>,
}

/// A branch protection rule that enforces checks before merge/push.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct BranchProtectionRule {
    /// Glob pattern matching branch names (e.g. "main", "release/*").
    pub pattern: String,
    /// Action names that must pass before merging into this branch.
    #[serde(default)]
    pub required_checks: Vec<String>,
    /// If true, require the latest commit to be signed.
    #[serde(default)]
    pub require_signed_commits: bool,
    /// If true, block direct pushes (force merge via PR).
    #[serde(default)]
    pub require_pull_request: bool,
    /// Minimum number of approvals required before merge (0 = no requirement).
    #[serde(default)]
    pub required_approvals: u32,
    /// If true, block force-pushes to this branch.
    #[serde(default = "default_true")]
    pub block_force_push: bool,
    /// If true, block branch deletion.
    #[serde(default = "default_true")]
    pub block_deletion: bool,
}

const fn default_true() -> bool {
    true
}

/// Default values inherited by all actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionDefaults {
    /// Shell to use for commands (e.g. "/bin/sh").
    #[serde(default = "default_shell")]
    pub shell: String,
    /// Default timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    /// Default working directory (relative to repo root).
    #[serde(default)]
    pub working_dir: Option<String>,
    /// Default environment variables.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Docker execution configuration.
    #[serde(default)]
    pub docker: DockerConfig,
}

impl Default for ActionDefaults {
    fn default() -> Self {
        Self {
            shell: default_shell(),
            timeout: default_timeout(),
            working_dir: None,
            env: BTreeMap::new(),
            docker: DockerConfig::default(),
        }
    }
}

/// Docker execution configuration for running actions inside a container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerConfig {
    /// Enable Docker execution for shell command actions.
    /// Built-in actions always run natively, ignoring this flag.
    #[serde(default)]
    pub enabled: bool,
    /// Docker image to use.
    #[serde(default = "default_docker_image")]
    pub image: String,
    /// Pull policy: "always", "if-not-present" (default), "never".
    #[serde(default = "default_pull_policy")]
    pub pull_policy: String,
    /// Additional `docker run` flags (e.g., `["--network=host"]`).
    #[serde(default)]
    pub extra_flags: Vec<String>,
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            image: default_docker_image(),
            pull_policy: default_pull_policy(),
            extra_flags: Vec::new(),
        }
    }
}

fn default_docker_image() -> String {
    "ghcr.io/olib-ai/ovc-actions:latest".to_owned()
}

fn default_pull_policy() -> String {
    "if-not-present".to_owned()
}

/// Allowed pull policy values.
const ALLOWED_PULL_POLICIES: &[&str] = &["always", "if-not-present", "never"];

/// Allowlist of safe `docker run` flag prefixes.
///
/// Only flags whose canonical form starts with one of these prefixes are
/// permitted.  Everything else is rejected to prevent privilege escalation via
/// `--privileged`, host-filesystem mounts, capability grants, etc.
///
/// The prefixes include both long-form (`--memory`) and short-form (`-e`)
/// variants where applicable.
const SAFE_DOCKER_FLAG_PREFIXES: &[&str] = &[
    "--memory",
    "--cpus",
    "--cpu-shares",
    "--pids-limit",
    "--read-only",
    "--network=none",
    "--tmpfs",
    "--env=",
    "--env ",
    "-e",
    "--label",
    "--name",
    "--rm",
    "--workdir",
    "-w",
];

fn default_shell() -> String {
    if cfg!(target_os = "windows") {
        "cmd".to_owned()
    } else {
        "/bin/sh".to_owned()
    }
}

const fn default_timeout() -> u64 {
    300
}

/// A single action definition.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActionDefinition {
    /// Human-friendly category.
    #[serde(default)]
    pub category: ActionCategory,
    /// Display name (defaults to the map key).
    #[serde(default)]
    pub display_name: Option<String>,
    /// Programming language this action targets.
    #[serde(default)]
    pub language: Option<String>,
    /// External tool name (informational).
    #[serde(default)]
    pub tool: Option<String>,
    /// Shell command to run.
    #[serde(default)]
    pub command: Option<String>,
    /// Shell command that automatically fixes issues.
    #[serde(default)]
    pub fix_command: Option<String>,
    /// When this action should be triggered.
    #[serde(default)]
    pub trigger: Trigger,
    /// Timeout override in seconds.
    #[serde(default)]
    pub timeout: Option<u64>,
    /// Working directory override (relative to repo root).
    #[serde(default)]
    pub working_dir: Option<String>,
    /// Extra environment variables.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// If true, failures won't block the pipeline.
    #[serde(default)]
    pub continue_on_error: bool,
    /// Condition restricting when this action runs.
    #[serde(default)]
    pub condition: Option<ActionCondition>,
    /// Cron-like schedule expression (informational only).
    #[serde(default)]
    pub schedule: Option<String>,
    /// If set, run a built-in action instead of a shell command.
    #[serde(default)]
    pub builtin: Option<BuiltinAction>,
    /// Arbitrary per-action configuration for built-ins.
    #[serde(default)]
    pub config: serde_yaml::Value,
    /// Actions that must complete before this one starts.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Matrix strategy for parameterized runs.
    #[serde(default)]
    pub matrix: Option<MatrixStrategy>,
    /// Number of retry attempts on failure.
    #[serde(default)]
    pub retry: Option<RetryConfig>,
    /// Conditional expression (evaluated as simple expression).
    #[serde(default)]
    pub if_condition: Option<String>,
    /// Output variables from this action (captured from stdout).
    #[serde(default)]
    pub outputs: Vec<OutputCapture>,
    /// Whether to cache the working directory between runs.
    #[serde(default)]
    pub cache: Option<CacheConfig>,
    /// If true, automatically run `fix_command` when the check fails during
    /// pre-commit/pre-push hooks, then re-run the check.  Requires
    /// `fix_command` to be set.
    #[serde(default)]
    pub auto_fix: bool,
    /// Per-action Docker override. `Some(false)` forces native execution even
    /// when Docker is globally enabled. `Some(true)` forces Docker even when
    /// globally disabled (if Docker is available). `None` inherits the global
    /// default.
    #[serde(default)]
    pub docker_override: Option<bool>,
}

/// Action category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ActionCategory {
    Lint,
    Format,
    Build,
    Test,
    Audit,
    Builtin,
    Security,
    Quality,
    #[default]
    Custom,
}

impl std::fmt::Display for ActionCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lint => write!(f, "lint"),
            Self::Format => write!(f, "format"),
            Self::Build => write!(f, "build"),
            Self::Test => write!(f, "test"),
            Self::Audit => write!(f, "audit"),
            Self::Builtin => write!(f, "builtin"),
            Self::Security => write!(f, "security"),
            Self::Quality => write!(f, "quality"),
            Self::Custom => write!(f, "custom"),
        }
    }
}

/// When an action should be triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Trigger {
    PreCommit,
    PostCommit,
    PrePush,
    PreMerge,
    PostMerge,
    OnFail,
    PullRequest,
    #[default]
    Manual,
    Schedule,
}

impl std::fmt::Display for Trigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PreCommit => write!(f, "pre-commit"),
            Self::PostCommit => write!(f, "post-commit"),
            Self::PrePush => write!(f, "pre-push"),
            Self::PreMerge => write!(f, "pre-merge"),
            Self::PostMerge => write!(f, "post-merge"),
            Self::OnFail => write!(f, "on-fail"),
            Self::PullRequest => write!(f, "pull-request"),
            Self::Manual => write!(f, "manual"),
            Self::Schedule => write!(f, "schedule"),
        }
    }
}

/// Built-in action types that don't require external tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinAction {
    SecretScan,
    TrailingWhitespace,
    LineEndings,
    FileSize,
    TodoCounter,
    LicenseHeader,
    DependencyAudit,
    CodeComplexity,
    DeadCode,
    DuplicateCode,
    CommitMessageLint,
    EncodingCheck,
    MergeConflictCheck,
    SymlinkCheck,
    LargeDiffWarning,
    BranchNaming,
    DebugStatements,
    MixedIndentation,
    BomCheck,
    ShellCheck,
    YamlLint,
    JsonLint,
    XmlLint,
    HardcodedIp,
    NonAsciiCheck,
    EofNewline,
    /// Detects supply chain attack patterns: env access, system file reads,
    /// process execution, network calls, and filesystem manipulation.
    SupplyChainScan,
    /// Scans installed dependency packages for obfuscated malicious code patterns.
    PackageScan,
    /// Queries public registries for outdated dependencies (like Dependabot).
    DependencyUpdateCheck,
}

impl std::fmt::Display for BuiltinAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SecretScan => write!(f, "secret_scan"),
            Self::TrailingWhitespace => write!(f, "trailing_whitespace"),
            Self::LineEndings => write!(f, "line_endings"),
            Self::FileSize => write!(f, "file_size"),
            Self::TodoCounter => write!(f, "todo_counter"),
            Self::LicenseHeader => write!(f, "license_header"),
            Self::DependencyAudit => write!(f, "dependency_audit"),
            Self::CodeComplexity => write!(f, "code_complexity"),
            Self::DeadCode => write!(f, "dead_code"),
            Self::DuplicateCode => write!(f, "duplicate_code"),
            Self::CommitMessageLint => write!(f, "commit_message_lint"),
            Self::EncodingCheck => write!(f, "encoding_check"),
            Self::MergeConflictCheck => write!(f, "merge_conflict_check"),
            Self::SymlinkCheck => write!(f, "symlink_check"),
            Self::LargeDiffWarning => write!(f, "large_diff_warning"),
            Self::BranchNaming => write!(f, "branch_naming"),
            Self::DebugStatements => write!(f, "debug_statements"),
            Self::MixedIndentation => write!(f, "mixed_indentation"),
            Self::BomCheck => write!(f, "bom_check"),
            Self::ShellCheck => write!(f, "shell_check"),
            Self::YamlLint => write!(f, "yaml_lint"),
            Self::JsonLint => write!(f, "json_lint"),
            Self::XmlLint => write!(f, "xml_lint"),
            Self::HardcodedIp => write!(f, "hardcoded_ip"),
            Self::NonAsciiCheck => write!(f, "non_ascii_check"),
            Self::EofNewline => write!(f, "eof_newline"),
            Self::SupplyChainScan => write!(f, "supply_chain_scan"),
            Self::PackageScan => write!(f, "package_scan"),
            Self::DependencyUpdateCheck => write!(f, "dependency_update_check"),
        }
    }
}

/// Condition for restricting when an action runs based on changed paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionCondition {
    /// Glob patterns that must match at least one changed path.
    #[serde(default)]
    pub paths: Vec<String>,
}

/// Matrix strategy — runs the action multiple times with different variable combinations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixStrategy {
    /// Variable name to list of values.
    pub variables: BTreeMap<String, Vec<String>>,
    /// Maximum parallel matrix runs.
    #[serde(default = "default_max_parallel")]
    pub max_parallel: usize,
    /// If true, stop all matrix runs on first failure.
    #[serde(default)]
    pub fail_fast: bool,
}

const fn default_max_parallel() -> usize {
    4
}

/// Retry configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of attempts (including the first).
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    /// Delay between retries in seconds.
    #[serde(default = "default_retry_delay")]
    pub delay_secs: u64,
}

const fn default_max_attempts() -> u32 {
    3
}

const fn default_retry_delay() -> u64 {
    5
}

/// Capture an output variable from action stdout.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OutputCapture {
    /// Name of the output variable.
    pub name: String,
    /// Regex pattern to extract the value from stdout.
    pub pattern: String,
}

/// Cache configuration for action artifacts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Directories to cache (relative to working dir).
    pub paths: Vec<String>,
    /// Cache key template (can reference env vars and matrix vars).
    pub key: String,
}

/// Maximum allowed size for an `actions.yml` file (1 MiB).
///
/// Prevents denial of service via pathologically large YAML files
/// (including billion-laughs-style entity expansion attacks, since the
/// input is capped before it even reaches the YAML parser).
const MAX_ACTIONS_YAML_SIZE: usize = 1024 * 1024;

impl ActionsConfig {
    /// Parse a config from a YAML string.
    ///
    /// Enforces a maximum input size of 1 MiB to guard against YAML
    /// entity expansion (billion laughs) and other denial-of-service
    /// vectors in the parser.
    pub fn from_yaml(yaml: &str) -> ActionsResult<Self> {
        if yaml.len() > MAX_ACTIONS_YAML_SIZE {
            return Err(crate::error::ActionsError::Config {
                reason: format!(
                    "actions.yml is too large ({} bytes); maximum allowed is {} bytes",
                    yaml.len(),
                    MAX_ACTIONS_YAML_SIZE
                ),
            });
        }
        let config: Self = serde_yaml::from_str(yaml)?;
        Ok(config)
    }

    /// Load configuration from `.ovc/actions.yml` relative to `repo_root`.
    /// Returns `Ok(None)` if the file does not exist.
    pub fn load(repo_root: &Path) -> ActionsResult<Option<Self>> {
        let config_path = repo_root.join(".ovc").join("actions.yml");
        if !config_path.is_file() {
            return Ok(None);
        }
        let metadata = std::fs::metadata(&config_path)?;
        if metadata.len() > MAX_ACTIONS_YAML_SIZE as u64 {
            return Err(crate::error::ActionsError::Config {
                reason: format!(
                    "actions.yml is too large ({} bytes); maximum allowed is {} bytes",
                    metadata.len(),
                    MAX_ACTIONS_YAML_SIZE
                ),
            });
        }
        let contents = std::fs::read_to_string(&config_path)?;
        let config = Self::from_yaml(&contents)?;
        Ok(Some(config))
    }

    /// Shells that are allowed for action execution.
    const ALLOWED_SHELLS: &[&str] = &[
        "/bin/sh",
        "/bin/bash",
        "sh",
        "bash",
        "/usr/bin/env",
        "cmd",
        "cmd.exe",
        "powershell",
        "powershell.exe",
        "pwsh",
        "pwsh.exe",
    ];

    /// Validate the configuration and return a list of warning/error messages.
    #[must_use]
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();

        if !Self::ALLOWED_SHELLS.contains(&self.defaults.shell.as_str()) {
            issues.push(format!(
                "defaults.shell '{}' is not allowed; permitted values: {}",
                self.defaults.shell,
                Self::ALLOWED_SHELLS.join(", ")
            ));
        }

        if let Some(ref wd) = self.defaults.working_dir
            && wd.contains("..")
        {
            issues.push(format!(
                "defaults.working_dir '{wd}' contains '..' (path traversal)"
            ));
        }

        if !ALLOWED_PULL_POLICIES.contains(&self.defaults.docker.pull_policy.as_str()) {
            issues.push(format!(
                "defaults.docker.pull_policy '{}' is not valid; permitted values: {}",
                self.defaults.docker.pull_policy,
                ALLOWED_PULL_POLICIES.join(", ")
            ));
        }

        if self.defaults.docker.enabled && self.defaults.docker.image.is_empty() {
            issues.push(
                "defaults.docker.enabled is true but defaults.docker.image is empty".to_owned(),
            );
        }

        for flag in &self.defaults.docker.extra_flags {
            if !is_safe_docker_flag(flag) {
                issues.push(format!(
                    "defaults.docker.extra_flags: flag '{flag}' is not permitted; \
                     allowed flag prefixes: {}",
                    SAFE_DOCKER_FLAG_PREFIXES.join(", ")
                ));
            }
        }

        for (name, def) in &self.actions {
            if def.command.is_none() && def.builtin.is_none() {
                issues.push(format!(
                    "action '{name}': must specify either 'command' or 'builtin'"
                ));
            }
            if def.command.is_some() && def.builtin.is_some() {
                issues.push(format!(
                    "action '{name}': cannot specify both 'command' and 'builtin'"
                ));
            }
            if def.timeout == Some(0) {
                issues.push(format!("action '{name}': timeout must be > 0"));
            }
            if name.is_empty() {
                issues.push("action name cannot be empty".to_owned());
            }
            if let Some(ref wd) = def.working_dir
                && wd.contains("..")
            {
                issues.push(format!(
                    "action '{name}': working_dir '{wd}' contains '..' (path traversal)"
                ));
            }
            for dep in &def.depends_on {
                if !self.actions.contains_key(dep) {
                    issues.push(format!(
                        "action '{name}': depends_on references unknown action '{dep}'"
                    ));
                }
            }
        }

        // Validate branch protection rules.
        for (i, rule) in self.branch_protection.iter().enumerate() {
            if rule.pattern.is_empty() {
                issues.push(format!("branch_protection[{i}]: pattern cannot be empty"));
            }
            for check in &rule.required_checks {
                if !self.actions.contains_key(check) {
                    issues.push(format!(
                        "branch_protection[{i}] (pattern '{}'): required_check references unknown action '{check}'",
                        rule.pattern
                    ));
                }
            }
        }

        issues
    }

    /// Return actions that match the given trigger.
    #[must_use]
    pub fn actions_for_trigger(&self, trigger: Trigger) -> Vec<(&str, &ActionDefinition)> {
        self.actions
            .iter()
            .filter(|(_, def)| def.trigger == trigger)
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }

    /// Find the branch protection rule that matches a branch name.
    /// Returns the first matching rule (rules are evaluated in order).
    #[must_use]
    pub fn protection_for_branch(&self, branch: &str) -> Option<&BranchProtectionRule> {
        self.branch_protection
            .iter()
            .find(|rule| glob_match(&rule.pattern, branch))
    }
}

/// Returns `true` if `flag` starts with one of the known-safe prefixes.
///
/// Comparison is performed on the raw flag string so that both `--memory=512m`
/// and `-e KEY=VALUE` are accepted.  The check is intentionally strict:
/// any flag not matching a safe prefix is denied, following an allowlist
/// (rather than denylist) security model.
fn is_safe_docker_flag(flag: &str) -> bool {
    SAFE_DOCKER_FLAG_PREFIXES
        .iter()
        .any(|prefix| flag == *prefix || flag.starts_with(prefix))
}

/// Glob pattern matching for branch protection rules.
///
/// Uses [`globset::GlobBuilder`] with `literal_separator(true)` so that `*`
/// matches within a single path segment and does not cross `/` boundaries,
/// while `**` spans multiple segments. This correctly handles patterns like
/// `release/*`, `feature/**`, `*-hotfix`, and literal branch names.
fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == text {
        return true;
    }
    globset::GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .ok()
        .is_some_and(|g| g.compile_matcher().is_match(text))
}
