//! Built-in actions that run without external tools.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;

use regex::Regex;
use walkdir::WalkDir;

use crate::config::BuiltinAction;
use crate::error::{ActionsError, ActionsResult};
use crate::runner::{ActionResult, ActionStatus};

/// Run a built-in action and return the result.
///
/// `name` is the action key from the configuration, and `display_name` is the
/// human-readable label. Both are propagated into the returned `ActionResult`
/// so that callers see consistent naming regardless of the builtin variant.
pub fn run_builtin(
    action: BuiltinAction,
    config: &serde_yaml::Value,
    repo_root: &Path,
    changed_paths: &[String],
    name: &str,
    display_name: &str,
    continue_on_error: bool,
) -> ActionsResult<ActionResult> {
    let started_at = chrono::Utc::now();
    let start = std::time::Instant::now();

    let (status, stdout) = match action {
        BuiltinAction::SecretScan => run_secret_scan(repo_root, changed_paths)?,
        BuiltinAction::TrailingWhitespace => run_trailing_whitespace(repo_root, changed_paths),
        BuiltinAction::LineEndings => run_line_endings(repo_root, changed_paths, config),
        BuiltinAction::FileSize => run_file_size(repo_root, changed_paths, config),
        BuiltinAction::TodoCounter => run_todo_counter(repo_root, changed_paths)?,
        BuiltinAction::LicenseHeader => run_license_header(repo_root, changed_paths, config),
        BuiltinAction::DependencyAudit => run_dependency_audit(repo_root, changed_paths, config),
        BuiltinAction::CodeComplexity => run_code_complexity(repo_root, changed_paths, config),
        BuiltinAction::DeadCode => run_dead_code(repo_root, changed_paths, config),
        BuiltinAction::DuplicateCode => run_duplicate_code(repo_root, changed_paths, config),
        BuiltinAction::CommitMessageLint => run_commit_message_lint(config),
        BuiltinAction::EncodingCheck => run_encoding_check(repo_root, changed_paths, config),
        BuiltinAction::MergeConflictCheck => run_merge_conflict_check(repo_root, changed_paths),
        BuiltinAction::SymlinkCheck => run_symlink_check(repo_root, changed_paths),
        BuiltinAction::LargeDiffWarning => run_large_diff_warning(repo_root, changed_paths, config),
        BuiltinAction::BranchNaming => run_branch_naming(repo_root, config),
        BuiltinAction::DebugStatements => run_debug_statements(repo_root, changed_paths, config)?,
        BuiltinAction::MixedIndentation => run_mixed_indentation(repo_root, changed_paths),
        BuiltinAction::BomCheck => run_bom_check(repo_root, changed_paths, config),
        BuiltinAction::ShellCheck => run_shell_check(repo_root, changed_paths, config),
        BuiltinAction::YamlLint => run_yaml_lint(repo_root, changed_paths, config),
        BuiltinAction::JsonLint => run_json_lint(repo_root, changed_paths, config),
        BuiltinAction::XmlLint => run_xml_lint(repo_root, changed_paths, config),
        BuiltinAction::HardcodedIp => run_hardcoded_ip(repo_root, changed_paths, config)?,
        BuiltinAction::NonAsciiCheck => run_non_ascii_check(repo_root, changed_paths, config)?,
        BuiltinAction::EofNewline => run_eof_newline(repo_root, changed_paths, config),
        BuiltinAction::SupplyChainScan => run_supply_chain_scan(repo_root, changed_paths, config)?,
        BuiltinAction::PackageScan => run_package_scan(repo_root, config)?,
        // DependencyUpdateCheck makes async HTTP requests.
        // Spawn a dedicated runtime so it works regardless of caller context
        // (single-threaded CLI runtime or multi-threaded API server).
        BuiltinAction::DependencyUpdateCheck => {
            let repo = repo_root.to_path_buf();
            let cfg = config.clone();
            std::thread::scope(|s| {
                s.spawn(|| {
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("failed to create tokio runtime")
                        .block_on(crate::depcheck::check_dependencies(&repo, &cfg))
                })
                .join()
                .expect("dependency check thread panicked")
            })
        }
    };

    let elapsed = start.elapsed();
    let finished_at = chrono::Utc::now();

    Ok(ActionResult {
        name: name.to_owned(),
        display_name: display_name.to_owned(),
        category: "builtin".to_owned(),
        status,
        exit_code: None,
        stdout,
        stderr: String::new(),
        duration_ms: saturating_millis(&elapsed),
        started_at: started_at.to_rfc3339(),
        finished_at: finished_at.to_rfc3339(),
        continue_on_error,
        ..ActionResult::default()
    })
}

/// Convert a Duration to milliseconds, saturating at `u64::MAX`.
fn saturating_millis(d: &std::time::Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// Collect files to scan: either the changed paths list or walk the repo.
fn files_to_scan(repo_root: &Path, changed_paths: &[String]) -> Vec<std::path::PathBuf> {
    if changed_paths.is_empty() {
        WalkDir::new(repo_root)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                !(name == ".git"
                    || name == "node_modules"
                    || name == "target"
                    || name == "vendor"
                    || name == "dist"
                    || name == "build"
                    || name == "__pycache__")
            })
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .map(walkdir::DirEntry::into_path)
            .collect()
    } else {
        changed_paths
            .iter()
            .map(|p| repo_root.join(p))
            .filter(|p| p.is_file())
            .collect()
    }
}

/// Read a file and return its contents if it appears to be a text file.
///
/// Performs a single `read()` call and checks the first 8192 bytes for NUL
/// to determine if the file is binary, avoiding double I/O.
fn read_if_text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let check_len = bytes.len().min(8192);
    if bytes[..check_len].contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

// ---------- Secret Scan ----------

fn run_secret_scan(
    repo_root: &Path,
    changed_paths: &[String],
) -> ActionsResult<(ActionStatus, String)> {
    let patterns: Vec<(&str, Regex)> = vec![
        (
            "AWS Access Key",
            Regex::new(r"(?i)AKIA[0-9A-Z]{16}").map_err(|e| ActionsError::BuiltinError {
                reason: e.to_string(),
            })?,
        ),
        (
            "AWS Secret Key",
            Regex::new(
                r#"(?i)(?:aws_secret_access_key|aws_secret)\s*[=:]\s*["']?[A-Za-z0-9/+=]{40}"#,
            )
            .map_err(|e| ActionsError::BuiltinError {
                reason: e.to_string(),
            })?,
        ),
        (
            "Private Key",
            Regex::new(r"-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----").map_err(|e| {
                ActionsError::BuiltinError {
                    reason: e.to_string(),
                }
            })?,
        ),
        (
            "Generic API Key",
            Regex::new(r#"(?i)(?:api_key|apikey|api_secret)\s*[=:]\s*["']?[A-Za-z0-9_\-]{20,}"#)
                .map_err(|e| ActionsError::BuiltinError {
                    reason: e.to_string(),
                })?,
        ),
        (
            "Generic Token",
            Regex::new(r#"(?i)(?:token|secret|password)\s*[=:]\s*["'][A-Za-z0-9_\-]{16,}["']"#)
                .map_err(|e| ActionsError::BuiltinError {
                    reason: e.to_string(),
                })?,
        ),
        (
            "GitHub Token",
            Regex::new(r"ghp_[A-Za-z0-9]{36}").map_err(|e| ActionsError::BuiltinError {
                reason: e.to_string(),
            })?,
        ),
    ];

    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();

    for path in &files {
        let Some(content) = read_if_text(path) else {
            continue;
        };
        let rel = path.strip_prefix(repo_root).unwrap_or(path);
        for (line_num, line) in content.lines().enumerate() {
            if line.contains("ovc:ignore") {
                continue;
            }
            for (label, re) in &patterns {
                if re.is_match(line) {
                    // Only report file, line number, and pattern name — never
                    // the matched secret value itself.
                    findings.push(format!("  {}:{}: [{}]", rel.display(), line_num + 1, label));
                }
            }
        }
    }

    if findings.is_empty() {
        Ok((ActionStatus::Passed, "No secrets detected.".to_owned()))
    } else {
        let mut out = format!("Found {} potential secret(s):\n", findings.len());
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        Ok((ActionStatus::Failed, out))
    }
}

// ---------- Trailing Whitespace ----------

fn run_trailing_whitespace(repo_root: &Path, changed_paths: &[String]) -> (ActionStatus, String) {
    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();

    for path in &files {
        let Some(content) = read_if_text(path) else {
            continue;
        };
        let rel = path.strip_prefix(repo_root).unwrap_or(path);
        for (line_num, line) in content.lines().enumerate() {
            if line != line.trim_end() {
                findings.push(format!("  {}:{}", rel.display(), line_num + 1));
            }
        }
    }

    if findings.is_empty() {
        (
            ActionStatus::Passed,
            "No trailing whitespace found.".to_owned(),
        )
    } else {
        let mut out = format!("Found trailing whitespace on {} line(s):\n", findings.len());
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        (ActionStatus::Failed, out)
    }
}

// ---------- Line Endings ----------

fn run_line_endings(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> (ActionStatus, String) {
    let expected = config
        .get("ending")
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or("lf");

    let check_crlf = expected == "lf";
    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();

    for path in &files {
        // read_if_text returns a String; we need bytes for line-ending checks.
        let Some(content) = read_if_text(path) else {
            continue;
        };
        let bytes = content.as_bytes();
        let rel = path.strip_prefix(repo_root).unwrap_or(path);
        if check_crlf {
            if bytes.windows(2).any(|w| w == b"\r\n") {
                findings.push(format!("  {}: contains CRLF (expected LF)", rel.display()));
            }
        } else if !bytes.windows(2).any(|w| w == b"\r\n") && bytes.contains(&b'\n') {
            findings.push(format!("  {}: contains LF (expected CRLF)", rel.display()));
        }
    }

    if findings.is_empty() {
        (
            ActionStatus::Passed,
            format!("All files use {expected} line endings."),
        )
    } else {
        let mut out = format!(
            "Found {} file(s) with wrong line endings:\n",
            findings.len()
        );
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        (ActionStatus::Failed, out)
    }
}

// ---------- File Size ----------

fn run_file_size(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> (ActionStatus, String) {
    let max_bytes: u64 = config
        .get("max_bytes")
        .and_then(serde_yaml::Value::as_u64)
        .unwrap_or(1_048_576);

    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();

    for path in &files {
        let Ok(meta) = std::fs::metadata(path) else {
            continue;
        };
        if meta.len() > max_bytes {
            let rel = path.strip_prefix(repo_root).unwrap_or(path);
            findings.push(format!(
                "  {}: {} bytes (max {max_bytes})",
                rel.display(),
                meta.len(),
            ));
        }
    }

    if findings.is_empty() {
        (
            ActionStatus::Passed,
            format!("All files under {max_bytes} bytes."),
        )
    } else {
        let mut out = format!("Found {} file(s) exceeding size limit:\n", findings.len());
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        (ActionStatus::Failed, out)
    }
}

// ---------- TODO Counter ----------

fn run_todo_counter(
    repo_root: &Path,
    changed_paths: &[String],
) -> ActionsResult<(ActionStatus, String)> {
    let re = Regex::new(r"\b(TODO|FIXME|HACK|XXX)\b").map_err(|e| ActionsError::BuiltinError {
        reason: e.to_string(),
    })?;

    let files = files_to_scan(repo_root, changed_paths);
    let mut total = 0u64;
    let mut details = Vec::new();

    for path in &files {
        let Some(content) = read_if_text(path) else {
            continue;
        };
        let rel = path.strip_prefix(repo_root).unwrap_or(path);
        for (line_num, line) in content.lines().enumerate() {
            for cap in re.find_iter(line) {
                total += 1;
                details.push(format!(
                    "  {}:{}: {}",
                    rel.display(),
                    line_num + 1,
                    cap.as_str()
                ));
            }
        }
    }

    let mut out = format!("Found {total} TODO/FIXME/HACK/XXX marker(s).\n");
    if total > 0 {
        for d in &details {
            out.push_str(d);
            out.push('\n');
        }
    }
    Ok((ActionStatus::Passed, out))
}

// ---------- License Header ----------

fn run_license_header(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> (ActionStatus, String) {
    let header = config
        .get("header")
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or("");

    if header.is_empty() {
        return (
            ActionStatus::Passed,
            "No license header configured; skipping.".to_owned(),
        );
    }

    let extensions: Vec<&str> = config
        .get("extensions")
        .and_then(serde_yaml::Value::as_sequence)
        .map_or_else(
            || vec!["rs", "js", "ts", "py", "go", "java", "rb", "c", "cpp", "h"],
            |seq| seq.iter().filter_map(serde_yaml::Value::as_str).collect(),
        );

    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();

    for path in &files {
        let ext = path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("");
        if !extensions.contains(&ext) {
            continue;
        }
        let Some(content) = read_if_text(path) else {
            continue;
        };
        if !content.starts_with(header) {
            let rel = path.strip_prefix(repo_root).unwrap_or(path);
            findings.push(format!("  {}: missing license header", rel.display()));
        }
    }

    if findings.is_empty() {
        (
            ActionStatus::Passed,
            "All files have the required license header.".to_owned(),
        )
    } else {
        let mut out = format!("Found {} file(s) missing license header:\n", findings.len());
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        (ActionStatus::Failed, out)
    }
}

// ---------- Helpers ----------

/// Extract a list of file extensions from a config value, falling back to defaults.
fn extensions_from_config<'a>(config: &'a serde_yaml::Value, defaults: &[&'a str]) -> Vec<&'a str> {
    config
        .get("extensions")
        .and_then(serde_yaml::Value::as_sequence)
        .map_or_else(
            || defaults.to_vec(),
            |seq| seq.iter().filter_map(serde_yaml::Value::as_str).collect(),
        )
}

/// Check if a path has one of the given extensions.
fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| extensions.contains(&ext))
}

// ---------- Dependency Audit ----------

/// Well-known package names used for typosquatting detection.
const POPULAR_PACKAGES: &[&str] = &[
    "serde",
    "tokio",
    "reqwest",
    "rand",
    "clap",
    "log",
    "regex",
    "react",
    "express",
    "lodash",
    "axios",
    "webpack",
    "babel",
    "requests",
    "flask",
    "django",
    "numpy",
    "pandas",
    "tensorflow",
    "gin",
    "echo",
    "cobra",
    "viper",
    "rails",
    "sinatra",
    "nokogiri",
    "devise",
];

/// Check a single line for possible typosquatting of popular package names.
fn check_typosquat(line_lower: &str, rel_display: &str, line_num: usize) -> Vec<String> {
    let mut hits = Vec::new();
    for &popular in POPULAR_PACKAGES {
        if line_lower.contains(popular) {
            continue;
        }
        if popular.len() >= 4 {
            for i in 0..popular.len() {
                let mut variant = popular.to_owned();
                variant.remove(i);
                if variant.len() >= 4 && line_lower.contains(&variant) {
                    hits.push(format!(
                        "  {rel_display}:{}: possible typosquat of '{popular}' (found '{variant}')",
                        line_num + 1,
                    ));
                    break;
                }
            }
        }
    }
    hits
}

fn run_dependency_audit(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> (ActionStatus, String) {
    let check_wildcards = config
        .get("check_wildcards")
        .and_then(serde_yaml::Value::as_bool)
        .unwrap_or(true);

    let dep_files = [
        "Cargo.toml",
        "package.json",
        "requirements.txt",
        "go.mod",
        "Gemfile",
    ];
    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();

    for path in &files {
        let file_name = path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("");
        if !dep_files.contains(&file_name) {
            continue;
        }
        let Some(content) = read_if_text(path) else {
            continue;
        };
        let rel = path.strip_prefix(repo_root).unwrap_or(path);
        let rel_display = rel.display().to_string();

        for (line_num, line) in content.lines().enumerate() {
            if check_wildcards
                && (line.contains("\"*\"") || line.contains("'*'") || line.contains("= \"*\""))
            {
                findings.push(format!(
                    "  {rel_display}:{}: wildcard version dependency",
                    line_num + 1
                ));
            }
            let line_lower = line.to_lowercase();
            findings.extend(check_typosquat(&line_lower, &rel_display, line_num));
        }
    }

    if findings.is_empty() {
        (
            ActionStatus::Passed,
            "No dependency issues found.".to_owned(),
        )
    } else {
        let mut out = format!("Found {} dependency issue(s):\n", findings.len());
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        (ActionStatus::Failed, out)
    }
}

// ---------- Code Complexity ----------

fn run_code_complexity(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> (ActionStatus, String) {
    let max_depth: usize = config
        .get("max_depth")
        .and_then(serde_yaml::Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(5);

    let extensions = extensions_from_config(
        config,
        &[
            "rs", "js", "ts", "jsx", "tsx", "c", "cpp", "h", "java", "go", "cs",
        ],
    );

    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();

    for path in &files {
        if !has_extension(path, &extensions) {
            continue;
        }
        let Some(content) = read_if_text(path) else {
            continue;
        };
        let rel = path.strip_prefix(repo_root).unwrap_or(path);
        let mut depth: usize = 0;
        let mut max_seen: usize = 0;
        let mut max_line: usize = 0;

        for (line_num, line) in content.lines().enumerate() {
            for ch in line.chars() {
                match ch {
                    '{' => {
                        depth += 1;
                        if depth > max_seen {
                            max_seen = depth;
                            max_line = line_num + 1;
                        }
                    }
                    '}' => {
                        depth = depth.saturating_sub(1);
                    }
                    _ => {}
                }
            }
        }

        if max_seen > max_depth {
            findings.push(format!(
                "  {}:{}: nesting depth {} exceeds max {}",
                rel.display(),
                max_line,
                max_seen,
                max_depth
            ));
        }
    }

    if findings.is_empty() {
        (
            ActionStatus::Passed,
            format!("All files within nesting depth limit ({max_depth})."),
        )
    } else {
        let mut out = format!(
            "Found {} file(s) exceeding nesting depth limit:\n",
            findings.len()
        );
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        (ActionStatus::Failed, out)
    }
}

// ---------- Dead Code ----------

fn run_dead_code(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> (ActionStatus, String) {
    let extensions = extensions_from_config(config, &["rs", "py", "js", "ts", "go"]);

    let files = files_to_scan(repo_root, changed_paths);
    let mut definitions: Vec<(String, String, usize)> = Vec::new(); // (name, file, line)
    let mut all_content = String::new();

    // Patterns for function definitions across languages.
    let def_patterns: &[&str] = &[
        r"\bfn\s+([a-zA-Z_][a-zA-Z0-9_]*)",         // Rust
        r"\bdef\s+([a-zA-Z_][a-zA-Z0-9_]*)",        // Python/Ruby
        r"\bfunction\s+([a-zA-Z_$][a-zA-Z0-9_$]*)", // JavaScript
        r"\bfunc\s+([a-zA-Z_][a-zA-Z0-9_]*)",       // Go
    ];

    let regexes: Vec<Regex> = def_patterns
        .iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect();

    for path in &files {
        if !has_extension(path, &extensions) {
            continue;
        }
        let Some(content) = read_if_text(path) else {
            continue;
        };
        let rel = path
            .strip_prefix(repo_root)
            .unwrap_or(path)
            .display()
            .to_string();

        for (line_num, line) in content.lines().enumerate() {
            for re in &regexes {
                if let Some(cap) = re.captures(line)
                    && let Some(name) = cap.get(1)
                {
                    let fn_name = name.as_str().to_owned();
                    // Skip common entry points and test functions.
                    if fn_name != "main"
                        && fn_name != "new"
                        && fn_name != "default"
                        && fn_name != "init"
                        && !fn_name.starts_with("test_")
                    {
                        definitions.push((fn_name, rel.clone(), line_num + 1));
                    }
                }
            }
        }
        all_content.push_str(&content);
        all_content.push('\n');
    }

    let mut findings = Vec::new();
    for (name, file, line) in &definitions {
        // Count occurrences of the function name in all content.
        // If it appears only once, it is likely dead code.
        let count = all_content.matches(name.as_str()).count();
        if count <= 1 {
            findings.push(format!("  {file}:{line}: '{name}' appears to be unused"));
        }
    }

    if findings.is_empty() {
        (
            ActionStatus::Passed,
            "No potentially dead code found.".to_owned(),
        )
    } else {
        let mut out = format!("Found {} potentially unused function(s):\n", findings.len());
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        // Informational only -- dead code detection is heuristic.
        (ActionStatus::Passed, out)
    }
}

// ---------- Duplicate Code ----------

fn run_duplicate_code(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> (ActionStatus, String) {
    let min_lines: usize = config
        .get("min_lines")
        .and_then(serde_yaml::Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(6);

    let extensions = extensions_from_config(config, &["rs", "py", "js", "ts"]);

    let files = files_to_scan(repo_root, changed_paths);
    // Map from hash -> list of (file, start_line).
    let mut hash_map: HashMap<u64, Vec<(String, usize)>> = HashMap::new();

    for path in &files {
        if !has_extension(path, &extensions) {
            continue;
        }
        let Some(content) = read_if_text(path) else {
            continue;
        };
        let rel = path
            .strip_prefix(repo_root)
            .unwrap_or(path)
            .display()
            .to_string();

        let lines: Vec<&str> = content.lines().collect();
        if lines.len() < min_lines {
            continue;
        }

        for start in 0..=(lines.len() - min_lines) {
            // Build a normalized block (trimmed, skip blank lines for hashing).
            let block: String = lines[start..start + min_lines]
                .iter()
                .map(|l| l.trim())
                .collect::<Vec<_>>()
                .join("\n");

            // Skip blocks that are mostly empty.
            if block.chars().filter(|c| !c.is_whitespace()).count() < 20 {
                continue;
            }

            let hash = simple_hash(block.as_bytes());
            hash_map
                .entry(hash)
                .or_default()
                .push((rel.clone(), start + 1));
        }
    }

    let mut findings = Vec::new();
    let mut reported_hashes = HashSet::new();
    for (hash, locations) in &hash_map {
        if locations.len() > 1 && reported_hashes.insert(*hash) {
            // Only report across different files or significantly different locations.
            let first = &locations[0];
            let second = &locations[1];
            if first.0 != second.0 || first.1.abs_diff(second.1) >= min_lines {
                findings.push(format!(
                    "  Duplicate block: {}:{} and {}:{}",
                    first.0, first.1, second.0, second.1
                ));
            }
        }
    }

    if findings.is_empty() {
        (
            ActionStatus::Passed,
            "No duplicate code blocks found.".to_owned(),
        )
    } else {
        let mut out = format!("Found {} duplicate block(s):\n", findings.len());
        // Cap output to avoid overwhelming reports.
        for f in findings.iter().take(50) {
            out.push_str(f);
            out.push('\n');
        }
        if findings.len() > 50 {
            let _ = writeln!(out, "  ... and {} more", findings.len() - 50);
        }
        (ActionStatus::Failed, out)
    }
}

/// Simple non-cryptographic hash for deduplication.
fn simple_hash(data: &[u8]) -> u64 {
    // FNV-1a 64-bit.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

// ---------- Commit Message Lint ----------

fn run_commit_message_lint(config: &serde_yaml::Value) -> (ActionStatus, String) {
    let max_subject: usize = config
        .get("max_subject")
        .and_then(serde_yaml::Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(72);

    let max_body_line: usize = config
        .get("max_body_line")
        .and_then(serde_yaml::Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(100);

    let require_conventional = config
        .get("require_conventional")
        .and_then(serde_yaml::Value::as_bool)
        .unwrap_or(false);

    // Try to get the commit message from env var or file.
    let message = std::env::var("OVC_COMMIT_MSG").ok().or_else(|| {
        config
            .get("message_file")
            .and_then(serde_yaml::Value::as_str)
            .and_then(|p| std::fs::read_to_string(p).ok())
    });

    let Some(message) = message else {
        return (
            ActionStatus::Passed,
            "No commit message provided; skipping.".to_owned(),
        );
    };

    let mut issues = Vec::new();
    let lines: Vec<&str> = message.lines().collect();

    if lines.is_empty() {
        return (ActionStatus::Failed, "Commit message is empty.".to_owned());
    }

    let subject = lines[0];

    if subject.len() > max_subject {
        issues.push(format!(
            "Subject line too long: {} chars (max {max_subject})",
            subject.len()
        ));
    }

    if lines.len() > 1 && !lines[1].is_empty() {
        issues.push("Missing blank line after subject.".to_owned());
    }

    if require_conventional {
        let conventional_prefixes = [
            "feat", "fix", "chore", "docs", "style", "refactor", "test", "perf", "ci", "build",
        ];
        let has_prefix = conventional_prefixes.iter().any(|p| {
            subject.starts_with(&format!("{p}:")) || subject.starts_with(&format!("{p}("))
        });
        if !has_prefix {
            issues.push(format!(
                "Subject does not follow conventional commit format (expected one of: {})",
                conventional_prefixes.join(", ")
            ));
        }
    }

    // Check body line lengths.
    for (i, line) in lines.iter().enumerate().skip(2) {
        if line.len() > max_body_line {
            issues.push(format!(
                "Body line {} too long: {} chars (max {max_body_line})",
                i + 1,
                line.len()
            ));
        }
    }

    if issues.is_empty() {
        (
            ActionStatus::Passed,
            "Commit message passes all checks.".to_owned(),
        )
    } else {
        let mut out = format!("Commit message has {} issue(s):\n", issues.len());
        for issue in &issues {
            out.push_str("  ");
            out.push_str(issue);
            out.push('\n');
        }
        (ActionStatus::Failed, out)
    }
}

// ---------- Encoding Check ----------

fn run_encoding_check(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> (ActionStatus, String) {
    let extensions = extensions_from_config(
        config,
        &["rs", "py", "js", "ts", "go", "rb", "java", "c", "cpp", "h"],
    );

    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();

    for path in &files {
        if !has_extension(path, &extensions) {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        // Skip binary files (contain NUL bytes).
        let check_len = bytes.len().min(8192);
        if bytes[..check_len].contains(&0) {
            continue;
        }
        if String::from_utf8(bytes).is_err() {
            let rel = path.strip_prefix(repo_root).unwrap_or(path);
            findings.push(format!("  {}: invalid UTF-8 encoding", rel.display()));
        }
    }

    if findings.is_empty() {
        (
            ActionStatus::Passed,
            "All files are valid UTF-8.".to_owned(),
        )
    } else {
        let mut out = format!("Found {} file(s) with invalid encoding:\n", findings.len());
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        (ActionStatus::Failed, out)
    }
}

// ---------- Merge Conflict Check ----------

fn run_merge_conflict_check(repo_root: &Path, changed_paths: &[String]) -> (ActionStatus, String) {
    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();
    let markers = ["<<<<<<<", "=======", ">>>>>>>"];

    for path in &files {
        let Some(content) = read_if_text(path) else {
            continue;
        };
        let rel = path.strip_prefix(repo_root).unwrap_or(path);
        for (line_num, line) in content.lines().enumerate() {
            for marker in &markers {
                if line.starts_with(marker) {
                    findings.push(format!(
                        "  {}:{}: unresolved merge conflict marker '{}'",
                        rel.display(),
                        line_num + 1,
                        marker
                    ));
                }
            }
        }
    }

    if findings.is_empty() {
        (
            ActionStatus::Passed,
            "No merge conflict markers found.".to_owned(),
        )
    } else {
        let mut out = format!(
            "Found {} unresolved merge conflict marker(s):\n",
            findings.len()
        );
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        (ActionStatus::Failed, out)
    }
}

// ---------- Symlink Check ----------

fn run_symlink_check(repo_root: &Path, changed_paths: &[String]) -> (ActionStatus, String) {
    let paths: Vec<std::path::PathBuf> = if changed_paths.is_empty() {
        WalkDir::new(repo_root)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                !(name.starts_with('.')
                    || name == "node_modules"
                    || name == "target"
                    || name == "vendor"
                    || name == "dist"
                    || name == "build"
                    || name == "__pycache__")
            })
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_symlink())
            .map(walkdir::DirEntry::into_path)
            .collect()
    } else {
        changed_paths
            .iter()
            .map(|p| repo_root.join(p))
            .filter(|p| p.is_symlink())
            .collect()
    };

    let mut findings = Vec::new();
    for path in &paths {
        // Check if symlink target exists.
        if !path.exists() {
            let rel = path.strip_prefix(repo_root).unwrap_or(path);
            let target = std::fs::read_link(path)
                .map_or_else(|_| "<unknown>".to_owned(), |t| t.display().to_string());
            findings.push(format!("  {}: broken symlink -> {}", rel.display(), target));
        }
    }

    if findings.is_empty() {
        (ActionStatus::Passed, "No broken symlinks found.".to_owned())
    } else {
        let mut out = format!("Found {} broken symlink(s):\n", findings.len());
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        (ActionStatus::Failed, out)
    }
}

// ---------- Large Diff Warning ----------

fn run_large_diff_warning(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> (ActionStatus, String) {
    let max_lines: u64 = config
        .get("max_lines")
        .and_then(serde_yaml::Value::as_u64)
        .unwrap_or(500);

    let files = files_to_scan(repo_root, changed_paths);
    let mut total_lines: u64 = 0;

    for path in &files {
        let Some(content) = read_if_text(path) else {
            continue;
        };
        total_lines += content.lines().count() as u64;
    }

    if total_lines > max_lines {
        (
            ActionStatus::Passed,
            format!(
                "Warning: Large diff detected ({total_lines} lines across {} file(s), threshold {max_lines}).",
                files.len()
            ),
        )
    } else {
        (
            ActionStatus::Passed,
            format!("Diff size OK ({total_lines} lines, threshold {max_lines})."),
        )
    }
}

// ---------- Branch Naming ----------

fn run_branch_naming(repo_root: &Path, config: &serde_yaml::Value) -> (ActionStatus, String) {
    let pattern_str = config
        .get("pattern")
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or(r"^(main|develop|feature/.*|bugfix/.*|hotfix/.*|release/.*)$");

    // Try to get branch name from env var, then from .ovc/HEAD.
    let branch = std::env::var("OVC_BRANCH").ok().or_else(|| {
        let head_path = repo_root.join(".ovc").join("HEAD");
        std::fs::read_to_string(head_path)
            .ok()
            .map(|s| s.trim().to_owned())
    });

    let Some(branch) = branch else {
        return (
            ActionStatus::Passed,
            "No branch name available; skipping.".to_owned(),
        );
    };

    let Ok(re) = Regex::new(pattern_str) else {
        return (
            ActionStatus::Failed,
            format!("Invalid branch naming pattern: {pattern_str}"),
        );
    };

    if re.is_match(&branch) {
        (
            ActionStatus::Passed,
            format!("Branch name '{branch}' matches naming convention."),
        )
    } else {
        (
            ActionStatus::Failed,
            format!("Branch name '{branch}' does not match pattern: {pattern_str}"),
        )
    }
}

// ---------- Debug Statements ----------

fn run_debug_statements(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> ActionsResult<(ActionStatus, String)> {
    let extensions = extensions_from_config(
        config,
        &["js", "ts", "jsx", "tsx", "py", "rs", "rb", "java", "go"],
    );

    let ignore_tests = config
        .get("ignore_tests")
        .and_then(serde_yaml::Value::as_bool)
        .unwrap_or(true);

    let ignore_paths: Vec<String> = config
        .get("ignore_paths")
        .and_then(serde_yaml::Value::as_sequence)
        .map(|seq| {
            seq.iter()
                .filter_map(serde_yaml::Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    #[allow(clippy::needless_raw_string_hashes)]
    let pattern = r"\b(console\.(log|debug|warn|error|info)|debugger\b|dbg!\(|pdb\.set_trace|binding\.pry|System\.out\.print(ln)?|fmt\.Print(ln|f)?)\b"; // ovc:ignore
    let re = Regex::new(pattern).map_err(|e| ActionsError::BuiltinError {
        reason: e.to_string(),
    })?;

    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();

    for path in &files {
        if !has_extension(path, &extensions) {
            continue;
        }

        let path_str = path.to_string_lossy();

        if ignore_tests
            && (path_str.contains("test")
                || path_str.contains("spec")
                || path_str.contains("__tests__"))
        {
            continue;
        }

        if ignore_paths.iter().any(|p| path_str.contains(p.as_str())) {
            continue;
        }

        let Some(content) = read_if_text(path) else {
            continue;
        };
        let rel = path.strip_prefix(repo_root).unwrap_or(path);
        for (line_num, line) in content.lines().enumerate() {
            if line.contains("ovc:ignore") {
                continue;
            }
            if re.is_match(line) {
                findings.push(format!(
                    "  {}:{}: {}",
                    rel.display(),
                    line_num + 1,
                    line.trim()
                ));
            }
        }
    }

    if findings.is_empty() {
        Ok((
            ActionStatus::Passed,
            "No debug statements found.".to_owned(),
        ))
    } else {
        let mut out = format!("Found {} debug statement(s):\n", findings.len());
        for f in findings.iter().take(100) {
            out.push_str(f);
            out.push('\n');
        }
        if findings.len() > 100 {
            let _ = writeln!(out, "  ... and {} more", findings.len() - 100);
        }
        Ok((ActionStatus::Failed, out))
    }
}

// ---------- Mixed Indentation ----------

fn run_mixed_indentation(repo_root: &Path, changed_paths: &[String]) -> (ActionStatus, String) {
    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();

    for path in &files {
        let Some(content) = read_if_text(path) else {
            continue;
        };
        let rel = path.strip_prefix(repo_root).unwrap_or(path);

        let mut has_tab_indent = false;
        let mut has_space_indent = false;

        for line in content.lines() {
            if line.starts_with('\t') {
                has_tab_indent = true;
            } else if line.starts_with("  ") {
                has_space_indent = true;
            }
            if has_tab_indent && has_space_indent {
                findings.push(format!(
                    "  {}: mixes tabs and spaces for indentation",
                    rel.display()
                ));
                break;
            }
        }
    }

    if findings.is_empty() {
        (
            ActionStatus::Passed,
            "No mixed indentation found.".to_owned(),
        )
    } else {
        let mut out = format!("Found {} file(s) with mixed indentation:\n", findings.len());
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        (ActionStatus::Failed, out)
    }
}

// ---------- BOM Check ----------

fn run_bom_check(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> (ActionStatus, String) {
    let reject_bom = config
        .get("reject_bom")
        .and_then(serde_yaml::Value::as_bool)
        .unwrap_or(true);

    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();
    let bom: &[u8] = &[0xEF, 0xBB, 0xBF];

    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if bytes.starts_with(bom) {
            let rel = path.strip_prefix(repo_root).unwrap_or(path);
            findings.push(format!("  {}: contains UTF-8 BOM", rel.display()));
        }
    }

    if findings.is_empty() {
        (
            ActionStatus::Passed,
            "No files with UTF-8 BOM found.".to_owned(),
        )
    } else if reject_bom {
        let mut out = format!("Found {} file(s) with UTF-8 BOM:\n", findings.len());
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        (ActionStatus::Failed, out)
    } else {
        let mut out = format!(
            "Found {} file(s) with UTF-8 BOM (informational):\n",
            findings.len()
        );
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        (ActionStatus::Passed, out)
    }
}

// ---------- Shell Check ----------

fn run_shell_check(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> (ActionStatus, String) {
    let require_set_e = config
        .get("require_set_e")
        .and_then(serde_yaml::Value::as_bool)
        .unwrap_or(true);

    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();

    for path in &files {
        let ext = path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("");
        // Check .sh files or files without extension that might be shell scripts.
        let is_shell = ext == "sh" || ext == "bash";
        if !is_shell {
            continue;
        }
        let Some(content) = read_if_text(path) else {
            continue;
        };
        let rel = path.strip_prefix(repo_root).unwrap_or(path);
        let lines: Vec<&str> = content.lines().collect();

        // Check shebang.
        if lines.is_empty() || !lines[0].starts_with("#!") {
            findings.push(format!("  {}: missing shebang line", rel.display()));
        } else if !lines[0].contains("/bin/bash")
            && !lines[0].contains("/bin/sh")
            && !lines[0].contains("/usr/bin/env")
        {
            findings.push(format!(
                "  {}:1: non-standard shebang: {}",
                rel.display(),
                lines[0]
            ));
        }

        // Check for set -e.
        if require_set_e && !content.contains("set -e") && !content.contains("set -euo") {
            findings.push(format!(
                "  {}: missing 'set -e' (error exit)",
                rel.display()
            ));
        }

        // Check for common issues: unquoted variables in certain patterns.
        for (line_num, line) in lines.iter().enumerate() {
            // Skip comments.
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                continue;
            }
            // Simple heuristic: `$VAR` not inside quotes in common commands.
            // Look for patterns like `rm $VAR` or `cp $VAR` (dangerous without quotes).
            if (trimmed.starts_with("rm ")
                || trimmed.starts_with("cp ")
                || trimmed.starts_with("mv "))
                && trimmed.contains('$')
                && !trimmed.contains('"')
            {
                findings.push(format!(
                    "  {}:{}: potentially unquoted variable in dangerous command",
                    rel.display(),
                    line_num + 1
                ));
            }
        }
    }

    if findings.is_empty() {
        (
            ActionStatus::Passed,
            "Shell scripts pass basic checks.".to_owned(),
        )
    } else {
        let mut out = format!("Found {} shell script issue(s):\n", findings.len());
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        (ActionStatus::Failed, out)
    }
}

// ---------- YAML Lint ----------

fn run_yaml_lint(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> (ActionStatus, String) {
    let extensions = extensions_from_config(config, &["yml", "yaml"]);

    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();

    for path in &files {
        if !has_extension(path, &extensions) {
            continue;
        }
        let Some(content) = read_if_text(path) else {
            continue;
        };
        let rel = path.strip_prefix(repo_root).unwrap_or(path);
        if let Err(e) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
            findings.push(format!("  {}: {e}", rel.display()));
        }
    }

    if findings.is_empty() {
        (ActionStatus::Passed, "All YAML files are valid.".to_owned())
    } else {
        let mut out = format!("Found {} invalid YAML file(s):\n", findings.len());
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        (ActionStatus::Failed, out)
    }
}

// ---------- JSON Lint ----------

fn run_json_lint(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> (ActionStatus, String) {
    let extensions = extensions_from_config(config, &["json"]);

    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();

    for path in &files {
        if !has_extension(path, &extensions) {
            continue;
        }
        // Skip JSONC files (tsconfig, etc.) which allow comments and trailing commas.
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let is_jsonc = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonc"));
        if file_name.to_ascii_lowercase().contains("tsconfig") || is_jsonc {
            continue;
        }
        let Some(content) = read_if_text(path) else {
            continue;
        };
        let rel = path.strip_prefix(repo_root).unwrap_or(path);
        if let Err(e) = serde_json::from_str::<serde_json::Value>(&content) {
            findings.push(format!("  {}: {e}", rel.display()));
        }
    }

    if findings.is_empty() {
        (ActionStatus::Passed, "All JSON files are valid.".to_owned())
    } else {
        let mut out = format!("Found {} invalid JSON file(s):\n", findings.len());
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        (ActionStatus::Failed, out)
    }
}

// ---------- XML Lint ----------

fn run_xml_lint(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> (ActionStatus, String) {
    let extensions = extensions_from_config(config, &["xml", "svg", "html"]);

    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();

    for path in &files {
        if !has_extension(path, &extensions) {
            continue;
        }
        let Some(content) = read_if_text(path) else {
            continue;
        };
        let rel = path.strip_prefix(repo_root).unwrap_or(path);

        // Simple well-formedness check: track tag stack.
        let mut tag_stack: Vec<String> = Vec::new();
        let mut well_formed = true;
        let mut error_line: usize = 0;
        let mut error_msg = String::new();

        for (line_num, line) in content.lines().enumerate() {
            // Skip XML declarations, comments, and processing instructions.
            let trimmed = line.trim();
            if trimmed.starts_with("<?") || trimmed.starts_with("<!") {
                continue;
            }

            let mut chars = trimmed.chars();
            while let Some(ch) = chars.next() {
                if ch == '<' {
                    let tag_content: String = chars.by_ref().take_while(|&c| c != '>').collect();
                    if tag_content.is_empty() {
                        continue;
                    }

                    // Self-closing tag.
                    if tag_content.ends_with('/') {
                        continue;
                    }

                    // Closing tag.
                    if let Some(stripped) = tag_content.strip_prefix('/') {
                        let tag_name = stripped.split_whitespace().next().unwrap_or("");
                        if let Some(open) = tag_stack.pop()
                            && open != tag_name
                        {
                            well_formed = false;
                            error_line = line_num + 1;
                            error_msg = format!(
                                "mismatched closing tag: expected </{open}>, found </{tag_name}>"
                            );
                            break;
                        }
                    } else {
                        // Opening tag - extract name.
                        let tag_name = tag_content
                            .split_whitespace()
                            .next()
                            .unwrap_or("")
                            .to_owned();
                        if !tag_name.is_empty() {
                            tag_stack.push(tag_name);
                        }
                    }
                }
            }
            if !well_formed {
                break;
            }
        }

        if !well_formed {
            findings.push(format!("  {}:{}: {}", rel.display(), error_line, error_msg));
        } else if !tag_stack.is_empty() {
            findings.push(format!(
                "  {}: unclosed tag(s): {}",
                rel.display(),
                tag_stack.join(", ")
            ));
        }
    }

    if findings.is_empty() {
        (
            ActionStatus::Passed,
            "All XML/HTML files appear well-formed.".to_owned(),
        )
    } else {
        let mut out = format!("Found {} XML/HTML issue(s):\n", findings.len());
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        (ActionStatus::Failed, out)
    }
}

// ---------- Hardcoded IP ----------

fn run_hardcoded_ip(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> ActionsResult<(ActionStatus, String)> {
    let allowed_ips: Vec<String> = config
        .get("allowed_ips")
        .and_then(serde_yaml::Value::as_sequence)
        .map_or_else(
            || vec!["127.0.0.1".to_owned(), "0.0.0.0".to_owned()],
            |seq| {
                seq.iter()
                    .filter_map(serde_yaml::Value::as_str)
                    .map(str::to_owned)
                    .collect()
            },
        );

    // Add common broadcast/meta addresses.
    let mut all_allowed = allowed_ips;
    for ip in &["255.255.255.255", "127.0.0.1", "0.0.0.0"] {
        let s = (*ip).to_owned();
        if !all_allowed.contains(&s) {
            all_allowed.push(s);
        }
    }

    let re = Regex::new(r"\b(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})\b").map_err(|e| {
        ActionsError::BuiltinError {
            reason: e.to_string(),
        }
    })?;

    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();

    for path in &files {
        let Some(content) = read_if_text(path) else {
            continue;
        };
        let rel = path.strip_prefix(repo_root).unwrap_or(path);
        for (line_num, line) in content.lines().enumerate() {
            for cap in re.captures_iter(line) {
                let ip = cap.get(1).map_or("", |m| m.as_str());
                if !all_allowed.iter().any(|a| a == ip) {
                    // Validate it looks like a real IP (each octet 0-255).
                    let valid = ip
                        .split('.')
                        .filter_map(|s| s.parse::<u16>().ok())
                        .all(|n| n <= 255);
                    if valid {
                        findings.push(format!(
                            "  {}:{}: hardcoded IP address {}",
                            rel.display(),
                            line_num + 1,
                            ip
                        ));
                    }
                }
            }
        }
    }

    if findings.is_empty() {
        Ok((
            ActionStatus::Passed,
            "No hardcoded IP addresses found.".to_owned(),
        ))
    } else {
        let mut out = format!("Found {} hardcoded IP address(es):\n", findings.len());
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        Ok((ActionStatus::Failed, out))
    }
}

// ---------- Non-ASCII Check ----------

fn run_non_ascii_check(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> ActionsResult<(ActionStatus, String)> {
    let extensions =
        extensions_from_config(config, &["rs", "py", "js", "ts", "go", "c", "cpp", "h"]);

    let allow_comments = config
        .get("allow_comments")
        .and_then(serde_yaml::Value::as_bool)
        .unwrap_or(true);

    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();

    // Patterns for single-line comments in various languages.
    let comment_re = Regex::new(r"(//|#|--)\s*.*$").map_err(|e| ActionsError::BuiltinError {
        reason: e.to_string(),
    })?;

    for path in &files {
        if !has_extension(path, &extensions) {
            continue;
        }
        let Some(content) = read_if_text(path) else {
            continue;
        };
        let rel = path.strip_prefix(repo_root).unwrap_or(path);
        for (line_num, line) in content.lines().enumerate() {
            let check_line = if allow_comments {
                // Strip comment portion before checking.
                comment_re.replace(line, "").to_string()
            } else {
                line.to_owned()
            };

            // Also skip string literals (rough heuristic: content between quotes).
            if !check_line.is_ascii() {
                findings.push(format!(
                    "  {}:{}: contains non-ASCII characters",
                    rel.display(),
                    line_num + 1,
                ));
            }
        }
    }

    if findings.is_empty() {
        Ok((
            ActionStatus::Passed,
            "No non-ASCII characters found in source files.".to_owned(),
        ))
    } else {
        let mut out = format!(
            "Found {} line(s) with non-ASCII characters:\n",
            findings.len()
        );
        for f in findings.iter().take(100) {
            out.push_str(f);
            out.push('\n');
        }
        if findings.len() > 100 {
            let _ = writeln!(out, "  ... and {} more", findings.len() - 100);
        }
        Ok((ActionStatus::Failed, out))
    }
}

// ---------- EOF Newline ----------

fn run_eof_newline(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> (ActionStatus, String) {
    let extensions = extensions_from_config(
        config,
        &["rs", "py", "js", "ts", "go", "rb", "java", "c", "cpp", "h"],
    );

    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();

    for path in &files {
        if !has_extension(path, &extensions) {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if bytes.is_empty() {
            continue;
        }
        // Check binary.
        let check_len = bytes.len().min(8192);
        if bytes[..check_len].contains(&0) {
            continue;
        }
        if bytes.last() != Some(&b'\n') {
            let rel = path.strip_prefix(repo_root).unwrap_or(path);
            findings.push(format!(
                "  {}: missing newline at end of file",
                rel.display()
            ));
        }
    }

    if findings.is_empty() {
        (
            ActionStatus::Passed,
            "All files end with a newline.".to_owned(),
        )
    } else {
        let mut out = format!("Found {} file(s) missing final newline:\n", findings.len());
        for f in &findings {
            out.push_str(f);
            out.push('\n');
        }
        (ActionStatus::Failed, out)
    }
}

// ---------- Supply Chain Scan ----------

/// File names (without extension) that are always scanned regardless of extension.
const SUPPLY_CHAIN_SCAN_NAMES: &[&str] = &[
    "Makefile",
    "Dockerfile",
    "Jenkinsfile",
    "Vagrantfile",
    "Gemfile",
    "Rakefile",
    "package.json",
    "setup.py",
    "setup.cfg",
    "pyproject.toml",
    "build.rs",
    "build.gradle",
    "package-lock.json",
    "yarn.lock",
    "Cargo.lock",
    "poetry.lock",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
    "lerna.json",
    ".cargo/config",
    ".cargo/config.toml",
    "settings.gradle",
    "pom.xml",
];

/// Returns `true` if the path should be scanned by the supply chain scanner.
fn is_supply_chain_scannable(path: &Path, extensions: &[&str]) -> bool {
    if has_extension(path, extensions) {
        return true;
    }
    // Check well-known filenames that may lack a matching extension.
    let file_name = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("");
    SUPPLY_CHAIN_SCAN_NAMES.contains(&file_name)
}

fn run_supply_chain_scan(
    repo_root: &Path,
    changed_paths: &[String],
    config: &serde_yaml::Value,
) -> ActionsResult<(ActionStatus, String)> {
    let severity = config
        .get("severity")
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or("flag");

    let enabled_categories: HashSet<&str> = config
        .get("categories")
        .and_then(serde_yaml::Value::as_sequence)
        .map_or_else(
            || {
                [
                    "env_access",
                    "system_file",
                    "network",
                    "process_exec",
                    "fs_manipulation",
                    "conditional_exec",
                    "lockfile_tampering",
                    "unicode_attacks",
                    "persistence",
                ]
                .into_iter()
                .collect()
            },
            |seq| seq.iter().filter_map(serde_yaml::Value::as_str).collect(),
        );

    let allowed_patterns: Vec<Regex> = config
        .get("allowed_patterns")
        .and_then(serde_yaml::Value::as_sequence)
        .map_or_else(Vec::new, |seq| {
            seq.iter()
                .filter_map(serde_yaml::Value::as_str)
                .filter_map(|s| Regex::new(s).ok())
                .collect()
        });

    let extensions = extensions_from_config(
        config,
        &[
            "rs", "py", "js", "ts", "jsx", "tsx", "go", "rb", "java", "kt", "c", "cpp", "h", "hpp",
            "cs", "swift", "ex", "exs", "php", "sh", "bash", "zsh", "ps1", "bat", "cmd", "yml",
            "yaml", "toml", "json", "xml", "gradle", "mk", "cmake",
        ],
    );

    let category_patterns = build_supply_chain_patterns(&enabled_categories)?;
    let files = files_to_scan(repo_root, changed_paths);
    let mut findings = Vec::new();

    for path in &files {
        if !is_supply_chain_scannable(path, &extensions) {
            continue;
        }
        let Some(content) = read_if_text(path) else {
            continue;
        };
        let rel = path.strip_prefix(repo_root).unwrap_or(path);
        scan_file_for_supply_chain_patterns(
            rel,
            &content,
            &category_patterns,
            &allowed_patterns,
            &mut findings,
        );
    }

    Ok(format_supply_chain_results(&findings, severity))
}

/// Scan a single file's content against all category patterns, appending hits
/// to `findings`.
fn scan_file_for_supply_chain_patterns(
    rel: &Path,
    content: &str,
    category_patterns: &[CategoryPatterns<'_>],
    allowed_patterns: &[Regex],
    findings: &mut Vec<String>,
) {
    let lines: Vec<&str> = content.lines().collect();
    let mut seen = HashSet::new();

    // Pass 1: per-line matching
    for (line_num, line) in lines.iter().enumerate() {
        if line.contains("ovc:ignore") {
            continue;
        }
        for (category, patterns) in category_patterns {
            for (label, re) in patterns {
                if re.is_match(line) && !allowed_patterns.iter().any(|ap| ap.is_match(line)) {
                    let finding = format!(
                        "  {}:{}: [{}] {}",
                        rel.display(),
                        line_num + 1,
                        category,
                        label,
                    );
                    if seen.insert(finding.clone()) {
                        findings.push(finding);
                    }
                }
            }
        }
        check_high_entropy_strings(line, rel, line_num, findings, &mut seen);
    }

    // Pass 2: sliding 3-line window for cross-line pattern detection
    if lines.len() >= 2 {
        for i in 0..lines.len().saturating_sub(2) {
            let end = (i + 3).min(lines.len());
            let window: String = lines[i..end].join(" ");
            if window.contains("ovc:ignore") {
                continue;
            }
            for (category, patterns) in category_patterns {
                for (label, re) in patterns {
                    if re.is_match(&window)
                        && !allowed_patterns.iter().any(|ap| ap.is_match(&window))
                    {
                        let finding = format!(
                            "  {}:{}~{}: [{}] {} (multi-line)",
                            rel.display(),
                            i + 1,
                            end,
                            category,
                            label,
                        );
                        if seen.insert(finding.clone()) {
                            findings.push(finding);
                        }
                    }
                }
            }
        }
    }
}

/// Format the final scan output and determine pass/fail status.
fn format_supply_chain_results(findings: &[String], severity: &str) -> (ActionStatus, String) {
    if findings.is_empty() {
        return (
            ActionStatus::Passed,
            "No supply chain risk patterns detected.".to_owned(),
        );
    }
    let mut out = format!("Found {} supply chain risk pattern(s):\n", findings.len());
    for f in findings.iter().take(200) {
        out.push_str(f);
        out.push('\n');
    }
    if findings.len() > 200 {
        let _ = writeln!(out, "  ... and {} more", findings.len() - 200);
    }
    let status = if severity == "warn" {
        ActionStatus::Passed
    } else {
        ActionStatus::Failed
    };
    (status, out)
}

/// A named pattern: human-readable label paired with compiled regex.
type NamedPattern<'a> = (&'a str, Regex);

/// A category of patterns: category label paired with its pattern list.
type CategoryPatterns<'a> = (&'a str, Vec<NamedPattern<'a>>);

/// Build all enabled category pattern sets for the supply chain scanner.
fn build_supply_chain_patterns<'a>(
    enabled: &HashSet<&str>,
) -> ActionsResult<Vec<CategoryPatterns<'a>>> {
    let mut out = Vec::new();
    if enabled.contains("env_access") {
        out.push(("env_access", build_env_access_patterns()?));
    }
    if enabled.contains("system_file") {
        out.push(("system_file", build_system_file_patterns()?));
    }
    if enabled.contains("network") {
        out.push(("network", build_network_patterns()?));
    }
    if enabled.contains("process_exec") {
        out.push(("process_exec", build_process_exec_patterns()?));
    }
    if enabled.contains("fs_manipulation") {
        out.push(("fs_manipulation", build_fs_manipulation_patterns()?));
    }
    if enabled.contains("conditional_exec") {
        out.push(("conditional_exec", build_conditional_exec_patterns()?));
    }
    if enabled.contains("lockfile_tampering") {
        out.push(("lockfile_tampering", build_lockfile_tampering_patterns()?));
    }
    if enabled.contains("unicode_attacks") {
        out.push(("unicode_attacks", build_unicode_attack_patterns()?));
    }
    if enabled.contains("persistence") {
        out.push(("persistence", build_persistence_patterns()?));
    }
    Ok(out)
}

fn build_env_access_patterns<'a>() -> ActionsResult<Vec<NamedPattern<'a>>> {
    compile_supply_chain_patterns(&[
        ("std::env::var usage", r"std::env::var[s]?\b"),
        ("env::var usage", r"\benv::var[s]?\b"),
        ("env! macro", r"\benv!\("),
        ("os.environ usage", r"\bos\.environ\b"),
        ("os.getenv usage", r"\bos\.getenv\b"),
        ("os.putenv usage", r"\bos\.putenv\b"),
        ("process.env usage", r"\bprocess\.env\b"),
        ("os.Getenv usage", r"\bos\.Getenv\b"),
        ("os.LookupEnv usage", r"\bos\.LookupEnv\b"),
        ("os.Setenv usage", r"\bos\.Setenv\b"),
        ("os.Environ usage", r"\bos\.Environ\(\)"),
        ("ENV[] usage", r"\bENV\["),
        ("ENV.fetch usage", r"\bENV\.fetch\b"),
        ("System.getenv usage", r"\bSystem\.getenv\b"),
        ("System.getProperty usage", r"\bSystem\.getProperty\b"),
        ("getenv() usage", r"\bgetenv\("),
        ("setenv() usage", r"\bsetenv\("),
        ("putenv() usage", r"\bputenv\("),
        ("printenv usage", r"\bprintenv\b"),
        ("$_ENV usage", r"\$_ENV\["),
        ("$_SERVER usage", r"\$_SERVER\["),
        (
            "Environment.GetEnvironmentVariable usage",
            r"\bEnvironment\.GetEnvironmentVariable\b",
        ),
        (
            "Environment.SetEnvironmentVariable usage",
            r"\bEnvironment\.SetEnvironmentVariable\b",
        ),
        (
            "ProcessInfo.processInfo.environment usage",
            r"\bProcessInfo\.processInfo\.environment\b",
        ),
        ("System.get_env usage", r"\bSystem\.get_env\b"),
    ])
}

fn build_system_file_patterns<'a>() -> ActionsResult<Vec<NamedPattern<'a>>> {
    compile_supply_chain_patterns(&[
        ("/etc/passwd reference", r"/etc/passwd\b"),
        ("/etc/shadow reference", r"/etc/shadow\b"),
        ("/etc/hosts reference", r"/etc/hosts\b"),
        ("/etc/sudoers reference", r"/etc/sudoers\b"),
        ("/etc/ssh/ reference", r"/etc/ssh/"),
        ("/etc/crontab reference", r"/etc/crontab\b"),
        ("/etc/cron.d/ reference", r"/etc/cron\.d/"),
        ("/var/spool/cron/ reference", r"/var/spool/cron/"),
        ("/proc/ filesystem access", r"/proc/"),
        ("/sys/ filesystem access", r"/sys/"),
        ("~/.ssh/ reference", r"~/\.ssh/"),
        ("$HOME/.ssh/ reference", r"\$HOME/\.ssh/"),
        ("~/.bashrc reference", r"~/\.bashrc\b"),
        ("~/.zshrc reference", r"~/\.zshrc\b"),
        ("~/.profile reference", r"~/\.profile\b"),
        ("~/.bash_profile reference", r"~/\.bash_profile\b"),
        ("~/.config/ reference", r"~/\.config/"),
        ("~/.local/ reference", r"~/\.local/"),
        ("%APPDATA% reference", r"%APPDATA%"),
        ("%USERPROFILE% reference", r"%USERPROFILE%"),
        ("Windows registry HKEY_ reference", r"\bHKEY_"),
        ("Windows registry HKLM reference", r"\bHKLM\\"),
        ("Windows registry HKCU reference", r"\bHKCU\\"),
        (
            "C:\\Windows\\System32 reference",
            r"(?i)C:\\Windows\\System32",
        ),
        ("C:\\Users\\ reference", r"(?i)C:\\Users\\"),
        ("~/.aws/credentials reference", r"\.aws/credentials\b"),
        (
            "~/.docker/config.json reference",
            r"\.docker/config\.json\b",
        ),
        ("~/.kube/config reference (Kubernetes)", r"\.kube/config\b"),
        (
            "GCP application_default_credentials.json",
            r"application_default_credentials\.json\b",
        ),
        (
            "Terraform state file (plaintext secrets)",
            r"terraform\.tfstate\b",
        ),
        (
            "Chrome Login Data credential store",
            r"(?i)(?:Chrome|Chromium)[/\\].*Login\s*Data",
        ),
        (
            "Firefox credential store",
            r"(?i)(?:Firefox|Mozilla)[/\\].*(?:logins\.json|key4\.db)",
        ),
        (
            ".env file reference",
            r"\.env(?:\.local|\.production|\.staging)?\b",
        ),
    ])
}

fn build_network_patterns<'a>() -> ActionsResult<Vec<NamedPattern<'a>>> {
    compile_supply_chain_patterns(&[
        ("socket.connect usage", r"\bsocket\.connect\b"),
        ("net.Dial usage", r"\bnet\.Dial\b"),
        ("TcpStream::connect usage", r"\bTcpStream::connect\b"),
        ("new Socket( usage", r"\bnew\s+Socket\("),
        ("curl invocation", r"\bcurl\s"),
        ("wget invocation", r"\bwget\s"),
        ("fetch() usage", r"\bfetch\("),
        ("http.get usage", r"\bhttp\.get\b"),
        ("requests.post usage", r"\brequests\.post\b"),
        ("urllib usage", r"\burllib\b"),
        ("httpx usage", r"\bhttpx\b"),
        ("dns.resolve usage", r"\bdns\.resolve\b"),
        ("getaddrinfo usage", r"\bgetaddrinfo\b"),
        ("nslookup invocation", r"\bnslookup\b"),
        ("dig invocation", r"\bdig\s"),
        ("bash -i (reverse shell)", r"\bbash\s+-i\b"),
        ("/dev/tcp/ reference", r"/dev/tcp/"),
        ("nc -e (reverse shell)", r"\bnc\s+-e\b"),
        ("ncat invocation", r"\bncat\b"),
        ("mkfifo usage", r"\bmkfifo\b"),
        (
            "ethers.js blockchain provider/contract (C2 channel)",
            r"\bnew\s+ethers\.(?:JsonRpcProvider|WebSocketProvider|Contract)\b",
        ),
        (
            "web3.js Contract call (blockchain C2)",
            r"\bnew\s+Web3\b|\bnew\s+web3\.eth\.Contract\b",
        ),
        (
            "IPFS gateway fetch",
            r"https?://(?:ipfs\.io|cloudflare-ipfs\.com|gateway\.pinata\.cloud|dweb\.link)/ipfs/",
        ),
        (
            "IPFS CID string literal",
            r"['\x22`](?:Qm[1-9A-HJ-NP-Za-km-z]{44}|bafy[a-z2-7]{55})['\x22`]",
        ),
        (
            "Infura RPC endpoint (blockchain node)",
            r"https?://[a-z-]+\.infura\.io/",
        ),
        (
            "Alchemy RPC endpoint (blockchain node)",
            r"https?://[a-z-]+-mainnet\.g\.alchemy\.com/",
        ),
        (
            "Rust reqwest HTTP call (build-time exfil)",
            r"\breqwest::(?:blocking::)?(?:get|post|Client)\b",
        ),
        (
            "Rust ureq HTTP call (build-time exfil)",
            r"\bureq::(?:get|post|agent)\b",
        ),
        (
            "Go linkname directive (symbol hijacking)",
            r"//go:linkname\s+\w+\s+\w+",
        ),
        ("Go unsafe.Pointer cast", r"\bunsafe\.Pointer\s*\("),
        (
            "Rust include_bytes/include_str from URL context",
            r"\binclude_(?:bytes|str)!\s*\(",
        ),
    ])
}

fn build_process_exec_patterns<'a>() -> ActionsResult<Vec<NamedPattern<'a>>> {
    compile_supply_chain_patterns(&[
        ("Command::new usage", r"\bCommand::new\b"),
        ("std::process::Command usage", r"\bstd::process::Command\b"),
        ("subprocess usage", r"\bsubprocess\."),
        ("os.system() usage", r"\bos\.system\("),
        ("os.popen() usage", r"\bos\.popen\("),
        ("os.exec usage", r"\bos\.exec"),
        ("child_process usage", r"\bchild_process\b"),
        ("exec() usage", r"\bexec\("),
        ("spawn() usage", r"\bspawn\("),
        ("execSync usage", r"\bexecSync\b"),
        ("exec.Command usage", r"\bexec\.Command\b"),
        ("system() usage", r"\bsystem\("),
        ("IO.popen usage", r"\bIO\.popen\b"),
        ("Open3 usage", r"\bOpen3\b"),
        ("Runtime.exec usage", r"\bRuntime\.exec\b"),
        ("ProcessBuilder usage", r"\bProcessBuilder\b"),
        ("shell_exec() usage", r"\bshell_exec\("),
        ("passthru() usage", r"\bpassthru\("),
        ("popen() usage", r"\bpopen\("),
        ("Process.Start usage", r"\bProcess\.Start\b"),
        ("eval usage", r"\beval\s"),
        ("source usage", r"\bsource\s"),
    ])
}

fn build_fs_manipulation_patterns<'a>() -> ActionsResult<Vec<NamedPattern<'a>>> {
    compile_supply_chain_patterns(&[
        ("npm preinstall script", r#""preinstall"\s*:"#),
        ("npm postinstall script", r#""postinstall"\s*:"#),
        ("npm install script", r#""install"\s*:"#),
        ("setup.py cmdclass", r"\bcmdclass\b"),
        ("install_requires with URL", r"install_requires.*https?://"),
        ("curl pipe to shell", r"\bcurl\b.*\|\s*(?:sh|bash)\b"),
        ("wget pipe to shell", r"\bwget\b.*\|\s*(?:sh|bash)\b"),
        (
            "Write to .github/workflows/ directory",
            r"(?:writeFile|writeFileSync|open\s*\([^,]+['\x22]w)\s*.*\.github[/\\]workflows",
        ),
        (
            "GitHub Actions expression injection",
            r"\$\{\{\s*github\.(?:event\.(?:issue|pull_request|comment|review)\.body|head_ref)\s*\}\}",
        ),
        (
            "GitHub Actions branch-ref (not SHA-pinned)",
            r"uses:\s*[a-zA-Z0-9_-]+/[a-zA-Z0-9_-]+@[a-zA-Z][a-zA-Z0-9._-]*$",
        ),
        (
            "Self-hosted runner registration token",
            r"(?:config\.(?:sh|cmd)|actions-runner).*--token",
        ),
    ])
}

fn build_lockfile_tampering_patterns<'a>() -> ActionsResult<Vec<NamedPattern<'a>>> {
    compile_supply_chain_patterns(&[
        (
            "yarn.lock resolved to non-standard registry",
            r#"resolved\s+"https?://[^\s"]+"#,
        ),
        (
            "package-lock resolved to non-standard registry",
            r#""resolved"\s*:\s*"https?://[^\s"]+"#,
        ),
        (
            "Cargo.lock source pointing to non-crates.io registry",
            r#"source\s*=\s*"registry\+https://[^\s"]+"#,
        ),
        (
            "poetry.lock source with non-standard URL",
            r#"url\s*=\s*"https?://[^\s"]+"#,
        ),
        (
            "integrity hash empty or removed in lockfile",
            r#""integrity"\s*:\s*"""#,
        ),
        (
            "Git dependency with SSH URL in lockfile",
            r#"(?:resolved|source)\s*[=:]\s*"?git\+ssh://"#,
        ),
        (
            "Cargo.toml [patch] section overriding crates.io",
            r"\[patch\.crates-io\]",
        ),
        (
            "npm workspaces with path traversal",
            r#""workspaces"\s*:\s*\[[^\]]*"\.\."#,
        ),
        (
            "pip extra-index-url (dependency confusion)",
            r"extra[-_]index[-_]url\s*=\s*https?://",
        ),
        ("pnpm workspace with path traversal", r"packages:.*\.\."),
        (
            "lerna packages with path traversal",
            r#""packages"\s*:\s*\[[^\]]*"\.\."#,
        ),
        (
            "npm config registry override",
            r#""config"\s*:\s*\{[^}]*"registry"\s*:"#,
        ),
        ("Cargo source replacement in config", r"\[source\.[^\]]+\]"),
        (
            "Gradle non-standard Maven repository",
            r"maven\s*\{[^}]*url\s+['\x22]https?://",
        ),
        (
            "Maven non-central repository URL",
            r"<repository>.*<url>https?://",
        ),
        ("pip index-url override", r"index[-_]url\s*=\s*https?://"),
    ])
}

/// Compile a slice of `(label, pattern)` pairs into `(label, Regex)`.
fn compile_supply_chain_patterns<'a>(
    pairs: &[(&'a str, &str)],
) -> ActionsResult<Vec<NamedPattern<'a>>> {
    pairs
        .iter()
        .map(|(label, pat)| {
            Regex::new(pat)
                .map(|re| (*label, re))
                .map_err(|e| ActionsError::BuiltinError {
                    reason: format!("supply_chain_scan regex error for '{label}': {e}"),
                })
        })
        .collect()
}

// ---------- Package Scan ----------

/// Well-known dependency directories to scan for compromised packages.
const PACKAGE_DIRS: &[&str] = &[
    "node_modules",
    "vendor",
    ".vendor",
    "bower_components",
    "jspm_packages",
    "web_modules",
];

/// Virtual environment directories that may contain `site-packages`.
const VENV_DIRS: &[&str] = &[".venv", "venv", "env"];

/// File extensions relevant to package scanning.
const PKG_SCAN_EXTENSIONS: &[&str] = &[
    "js", "mjs", "cjs", "ts", "py", "rb", "php", "sh", "bash", "pl", "lua", "ps1", "bat", "cmd",
    "json", "gyp", "gemspec", "wasm",
];

/// Default maximum walk depth inside dependency directories.
const PKG_SCAN_DEFAULT_DEPTH: u64 = 8;

/// Default maximum file size in bytes (512 KB).
const PKG_SCAN_DEFAULT_FILE_SIZE: u64 = 524_288;

/// Default maximum number of files to scan.
const PKG_SCAN_DEFAULT_MAX_FILES: u64 = 10_000;

/// Maximum number of findings to include in output before truncation.
const PKG_SCAN_MAX_FINDINGS_DISPLAY: usize = 300;

/// A named pattern for package scanning: human-readable label paired with compiled regex.
type PkgNamedPattern<'a> = (&'a str, Regex);

/// A category of patterns for package scanning: category label paired with its pattern list.
type PkgCategoryPatterns<'a> = (&'a str, Vec<PkgNamedPattern<'a>>);

fn run_package_scan(
    repo_root: &Path,
    config: &serde_yaml::Value,
) -> ActionsResult<(ActionStatus, String)> {
    let severity = config
        .get("severity")
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or("flag");

    let max_depth = config
        .get("max_depth")
        .and_then(serde_yaml::Value::as_u64)
        .unwrap_or(PKG_SCAN_DEFAULT_DEPTH);

    let max_file_size = config
        .get("max_file_size")
        .and_then(serde_yaml::Value::as_u64)
        .unwrap_or(PKG_SCAN_DEFAULT_FILE_SIZE);

    let max_files = config
        .get("max_files")
        .and_then(serde_yaml::Value::as_u64)
        .unwrap_or(PKG_SCAN_DEFAULT_MAX_FILES);

    let enabled_categories: HashSet<&str> = config
        .get("categories")
        .and_then(serde_yaml::Value::as_sequence)
        .map_or_else(
            || {
                [
                    "obfuscation",
                    "dynamic_exec",
                    "suspicious_network",
                    "fs_tampering",
                    "install_hooks",
                    "exfiltration",
                    "conditional_exec",
                    "unicode_attacks",
                    "steganography",
                    "persistence",
                ]
                .into_iter()
                .collect()
            },
            |seq| seq.iter().filter_map(serde_yaml::Value::as_str).collect(),
        );

    let extra_dirs: Vec<&str> = config
        .get("scan_dirs")
        .and_then(serde_yaml::Value::as_sequence)
        .map_or_else(Vec::new, |seq| {
            seq.iter().filter_map(serde_yaml::Value::as_str).collect()
        });

    let allowed_packages: Vec<&str> = config
        .get("allowed_packages")
        .and_then(serde_yaml::Value::as_sequence)
        .map_or_else(Vec::new, |seq| {
            seq.iter().filter_map(serde_yaml::Value::as_str).collect()
        });

    let category_patterns = build_package_scan_patterns(&enabled_categories)?;
    let files = collect_package_files(repo_root, max_depth, max_file_size, max_files, &extra_dirs);
    let mut findings = Vec::new();

    for path in &files {
        if is_allowed_package(path, repo_root, &allowed_packages) {
            continue;
        }
        let rel = path.strip_prefix(repo_root).unwrap_or(path);
        if let Some(content) = read_if_text(path) {
            scan_package_file(rel, &content, &category_patterns, &mut findings);
        } else if has_extension(path, &["wasm", "node"]) {
            // Binary file scanning for WASM and native addons
            if let Ok(data) = std::fs::read(path) {
                let max_binary_size = 2_097_152; // 2 MB cap for binary scanning
                if data.len() <= max_binary_size {
                    scan_binary_strings(rel, &data, &mut findings);
                }
            }
        }
    }

    Ok(format_package_scan_results(&findings, severity))
}

/// Check whether a file belongs to an allowed (skipped) package.
fn is_allowed_package(path: &Path, repo_root: &Path, allowed: &[&str]) -> bool {
    if allowed.is_empty() {
        return false;
    }
    let rel = path.strip_prefix(repo_root).unwrap_or(path);
    let components: Vec<_> = rel.components().collect();

    // Find the index of the first dependency-directory component.
    let Some(pkg_dir_idx) = components
        .iter()
        .position(|c| PACKAGE_DIRS.contains(&c.as_os_str().to_string_lossy().as_ref()))
    else {
        return false;
    };

    // The package name immediately follows the dependency dir.
    // Skip @scope prefix if present.
    let mut name_idx = pkg_dir_idx + 1;
    if let Some(c) = components.get(name_idx)
        && c.as_os_str().to_string_lossy().starts_with('@')
    {
        name_idx += 1;
    }
    let Some(pkg_component) = components.get(name_idx) else {
        return false;
    };

    // Check if there is a NESTED dependency directory after the package name.
    // If so, this file belongs to a transitive dependency — do NOT allow.
    for component in &components[name_idx + 1..] {
        if PACKAGE_DIRS.contains(&component.as_os_str().to_string_lossy().as_ref()) {
            return false;
        }
    }

    let pkg_name = pkg_component.as_os_str().to_string_lossy();
    allowed.iter().any(|pattern| {
        pattern.strip_suffix('*').map_or_else(
            || *pkg_name == **pattern,
            |prefix| pkg_name.starts_with(prefix),
        )
    })
}

/// Collect files from well-known dependency directories for scanning.
fn collect_package_files(
    repo_root: &Path,
    max_depth: u64,
    max_file_size: u64,
    max_files: u64,
    extra_dirs: &[&str],
) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let max_files = usize::try_from(max_files).unwrap_or(usize::MAX);
    let max_depth = usize::try_from(max_depth).unwrap_or(usize::MAX);

    // Collect candidate root directories to walk.
    let mut roots: Vec<std::path::PathBuf> = Vec::new();

    // Well-known dependency directories directly under repo root.
    for dir_name in PACKAGE_DIRS {
        let candidate = repo_root.join(dir_name);
        if candidate.is_dir() {
            roots.push(candidate);
        }
    }

    // Virtual environment directories.
    for venv in VENV_DIRS {
        let candidate = repo_root.join(venv);
        if candidate.is_dir() {
            roots.push(candidate);
        }
    }

    // User-specified additional directories.
    for extra in extra_dirs {
        let candidate = repo_root.join(extra);
        if candidate.is_dir() {
            roots.push(candidate);
        }
    }

    // Also find site-packages directories anywhere under repo root (bounded walk).
    collect_site_packages_dirs(repo_root, &mut roots);

    for root in &roots {
        if files.len() >= max_files {
            break;
        }
        walk_package_dir(root, max_depth, max_file_size, max_files, &mut files);
    }

    files
}

/// Search for `site-packages` directories under `repo_root` using a shallow walk.
fn collect_site_packages_dirs(repo_root: &Path, roots: &mut Vec<std::path::PathBuf>) {
    // Walk at most 6 levels deep to find site-packages dirs without excessive traversal.
    for entry in WalkDir::new(repo_root)
        .max_depth(6)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Skip .git and large irrelevant trees, but enter node_modules/vendor to find
            // nested site-packages (unlikely but possible).
            !(name == ".git" || name == "target" || name == "dist" || name == "build")
        })
    {
        let Ok(entry) = entry else { continue };
        if entry.file_type().is_dir() && entry.file_name() == "site-packages" {
            let path = entry.into_path();
            if !roots.contains(&path) {
                roots.push(path);
            }
        }
    }
}

/// Walk a single dependency directory collecting scannable files.
fn walk_package_dir(
    root: &Path,
    max_depth: usize,
    max_file_size: u64,
    max_files: usize,
    files: &mut Vec<std::path::PathBuf>,
) {
    for entry in WalkDir::new(root)
        .max_depth(max_depth)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Only skip .git inside packages — other hidden directories may
            // contain attack code (e.g. .helpers/, .bin/, .npmrc).
            name != ".git"
        })
    {
        if files.len() >= max_files {
            break;
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_file() {
            continue;
        }
        if !has_extension(entry.path(), PKG_SCAN_EXTENSIONS) {
            continue;
        }
        // Skip files exceeding size limit.
        if let Ok(meta) = entry.metadata()
            && meta.len() > max_file_size
        {
            continue;
        }
        files.push(entry.into_path());
    }
}

/// Scan a single file's content against all package scan category patterns.
fn scan_package_file(
    rel: &Path,
    content: &str,
    category_patterns: &[PkgCategoryPatterns<'_>],
    findings: &mut Vec<String>,
) {
    let lines: Vec<&str> = content.lines().collect();
    let mut seen = HashSet::new();

    // Pass 1: per-line matching
    for (line_num, line) in lines.iter().enumerate() {
        for (category, patterns) in category_patterns {
            for (label, re) in patterns {
                if re.is_match(line) {
                    let finding = format!(
                        "  {}:{}: [{}] {}",
                        rel.display(),
                        line_num + 1,
                        category,
                        label,
                    );
                    if seen.insert(finding.clone()) {
                        findings.push(finding);
                    }
                }
            }
        }
        check_high_entropy_strings(line, rel, line_num, findings, &mut seen);
    }

    // Pass 2: sliding 3-line window for cross-line pattern detection
    if lines.len() >= 2 {
        for i in 0..lines.len().saturating_sub(2) {
            let end = (i + 3).min(lines.len());
            let window: String = lines[i..end].join(" ");
            for (category, patterns) in category_patterns {
                for (label, re) in patterns {
                    if re.is_match(&window) {
                        let finding = format!(
                            "  {}:{}~{}: [{}] {} (multi-line)",
                            rel.display(),
                            i + 1,
                            end,
                            category,
                            label,
                        );
                        if seen.insert(finding.clone()) {
                            findings.push(finding);
                        }
                    }
                }
            }
        }
    }
}

/// Format the final package scan output and determine pass/fail status.
fn format_package_scan_results(findings: &[String], severity: &str) -> (ActionStatus, String) {
    if findings.is_empty() {
        return (
            ActionStatus::Passed,
            "No suspicious patterns detected in dependency packages.".to_owned(),
        );
    }
    let mut out = format!(
        "Found {} suspicious pattern(s) in dependency packages:\n",
        findings.len()
    );
    for f in findings.iter().take(PKG_SCAN_MAX_FINDINGS_DISPLAY) {
        out.push_str(f);
        out.push('\n');
    }
    if findings.len() > PKG_SCAN_MAX_FINDINGS_DISPLAY {
        let _ = writeln!(
            out,
            "  ... and {} more",
            findings.len() - PKG_SCAN_MAX_FINDINGS_DISPLAY
        );
    }
    let status = if severity == "warn" {
        ActionStatus::Passed
    } else {
        ActionStatus::Failed
    };
    (status, out)
}

/// Extract printable ASCII runs from binary data and scan for suspicious strings.
fn scan_binary_strings(rel: &Path, data: &[u8], findings: &mut Vec<String>) {
    const MIN_RUN_LEN: usize = 12;
    let suspicious_patterns: &[(&str, &str)] = &[
        ("Embedded URL in binary", r"https?://[^\s'\x22]{10,}"),
        (
            "Embedded shell path in binary",
            r"/bin/(?:sh|bash|zsh|dash)\b",
        ),
        (
            "Embedded /etc/ path in binary",
            r"/etc/(?:passwd|shadow|hosts|sudoers)\b",
        ),
        (
            "Embedded SSH path in binary",
            r"\.ssh/(?:id_rsa|id_ed25519|authorized_keys)\b",
        ),
        ("Embedded reverse shell in binary", r"/dev/tcp/\d"),
        (
            "Embedded eval/exec in binary",
            r"\b(?:eval|exec|system|popen)\s*\(",
        ),
        (
            "Embedded base64 decode in binary",
            r"(?:atob|b64decode|Base64\.decode|Buffer\.from)\s*\(",
        ),
        (
            "Embedded credential env var name in binary",
            r"(?:AWS_SECRET_ACCESS_KEY|AWS_ACCESS_KEY_ID|GITHUB_TOKEN|NPM_TOKEN|PYPI_TOKEN|DATABASE_URL|VAULT_TOKEN)",
        ),
        (
            "Embedded Windows credential path in binary",
            r"(?i)(?:DPAPI|CryptUnprotectData|Microsoft\\\\Credentials)",
        ),
        (
            "Embedded HTTP header in binary (manual request construction)",
            r"(?:Content-Type|Authorization|X-Api-Key)\s*:",
        ),
    ];

    // Compile patterns
    let compiled: Vec<(&str, Regex)> = suspicious_patterns
        .iter()
        .filter_map(|(label, pat)| Regex::new(pat).ok().map(|re| (*label, re)))
        .collect();

    if compiled.is_empty() {
        return;
    }

    // Extract printable ASCII runs and check against patterns
    let mut run_start = None;
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        if b.is_ascii_graphic() || b == b' ' {
            if run_start.is_none() {
                run_start = Some(i);
            }
        } else if let Some(start) = run_start {
            let run_len = i - start;
            if run_len >= MIN_RUN_LEN
                && let Ok(s) = std::str::from_utf8(&data[start..i])
            {
                for (label, re) in &compiled {
                    if re.is_match(s) {
                        findings.push(format!(
                            "  {}:byte_{}: [binary_payload] {}",
                            rel.display(),
                            start,
                            label,
                        ));
                    }
                }
            }
            run_start = None;
        }
        i += 1;
    }
    // Handle trailing run
    if let Some(start) = run_start {
        let run_len = data.len() - start;
        if run_len >= MIN_RUN_LEN
            && let Ok(s) = std::str::from_utf8(&data[start..])
        {
            for (label, re) in &compiled {
                if re.is_match(s) {
                    findings.push(format!(
                        "  {}:byte_{}: [binary_payload] {}",
                        rel.display(),
                        start,
                        label,
                    ));
                }
            }
        }
    }
}

/// Build all enabled category pattern sets for the package scanner.
fn build_package_scan_patterns<'a>(
    enabled: &HashSet<&str>,
) -> ActionsResult<Vec<PkgCategoryPatterns<'a>>> {
    let mut out = Vec::new();
    if enabled.contains("obfuscation") {
        out.push(("obfuscation", build_obfuscation_patterns()?));
    }
    if enabled.contains("dynamic_exec") {
        out.push(("dynamic_exec", build_dynamic_exec_patterns()?));
    }
    if enabled.contains("suspicious_network") {
        out.push(("suspicious_network", build_suspicious_network_patterns()?));
    }
    if enabled.contains("fs_tampering") {
        out.push(("fs_tampering", build_fs_tampering_patterns()?));
    }
    if enabled.contains("install_hooks") {
        out.push(("install_hooks", build_install_hook_patterns()?));
    }
    if enabled.contains("exfiltration") {
        out.push(("exfiltration", build_exfiltration_patterns()?));
    }
    if enabled.contains("conditional_exec") {
        out.push(("conditional_exec", build_conditional_exec_patterns()?));
    }
    if enabled.contains("unicode_attacks") {
        out.push(("unicode_attacks", build_unicode_attack_patterns()?));
    }
    if enabled.contains("steganography") {
        out.push(("steganography", build_steganography_patterns()?));
    }
    if enabled.contains("persistence") {
        out.push(("persistence", build_persistence_patterns()?));
    }
    Ok(out)
}

fn build_conditional_exec_patterns<'a>() -> ActionsResult<Vec<PkgNamedPattern<'a>>> {
    compile_pkg_patterns(&[
        (
            "Date-gated execution (year/month check)",
            r"\bnew\s+Date\(\).*(?:getFullYear|getMonth|getDate)\s*\(\)\s*[><=!]=?\s*\d",
        ),
        (
            "Date.now() threshold gate",
            r"\bDate\.now\(\)\s*[><=]=?\s*\d{10,13}\b",
        ),
        (
            "Python datetime date-gate",
            r"(?:datetime\.now|date\.today)\s*\(\).*[><=!]=",
        ),
        (
            "CI environment absence check",
            r"(?:process\.env\.CI|os\.environ\.get\(['\x22]CI['\x22])\s*[!=]=",
        ),
        (
            "Hostname-conditional execution",
            r"(?:os\.hostname\(\)|socket\.gethostname\(\))\s*[!=]==?",
        ),
        (
            "npm_package_name conditional (dependency confusion)",
            r"process\.env\.npm_package_name\s*[!=]==?",
        ),
        (
            "USER/LOGNAME environment check",
            r"process\.env\.(?:USER|LOGNAME|USERNAME)\s*[!=]==?",
        ),
    ])
}

fn build_unicode_attack_patterns<'a>() -> ActionsResult<Vec<PkgNamedPattern<'a>>> {
    compile_pkg_patterns(&[
        (
            "Bidirectional text override (Trojan Source CVE-2021-42574)",
            r"[\u{202A}-\u{202E}\u{2066}-\u{2069}]",
        ),
        (
            "Zero-width character in code (invisible manipulation)",
            r"[\u{200B}\u{200C}\u{200D}\u{FEFF}]",
        ),
        (
            "Cyrillic homoglyph in code (visual spoofing)",
            r"[\u{0430}\u{0435}\u{043E}\u{0441}\u{0440}\u{0445}\u{0443}]",
        ),
        (
            "Fullwidth Latin characters (keyword evasion via Unicode normalization)",
            r"[\u{FF01}-\u{FF5E}]",
        ),
        (
            "Mathematical Alphanumeric Symbols (transpiler-normalized evasion)",
            r"[\u{1D400}-\u{1D7FF}]",
        ),
    ])
}

fn build_steganography_patterns<'a>() -> ActionsResult<Vec<PkgNamedPattern<'a>>> {
    compile_pkg_patterns(&[
        (
            "PIL/Pillow pixel access (potential steganography)",
            r"\b(?:getpixel|putpixel|img\.load)\s*\(",
        ),
        (
            "LSB bit extraction pattern",
            r"\[\s*[0-3]\s*\]\s*&\s*(?:0x0?1|1\b)|>>\s*7\s*&\s*1",
        ),
        (
            "EXIF metadata extraction for payload",
            r"\b(?:_getexif|exifread\.process_file|piexif\.load)\b",
        ),
        (
            "Steganography library import",
            r"\b(?:from\s+stegano|import\s+stegpy|stegano\.lsb)\b",
        ),
        (
            "Image fetch with binary processing",
            r"(?:requests\.get|urllib\.request\.urlopen|fetch)\s*\(.*\.(?:png|jpg|jpeg|bmp|gif)['\x22?]",
        ),
    ])
}

/// Compile a slice of `(label, pattern)` pairs into `PkgNamedPattern` vec.
fn compile_pkg_patterns<'a>(pairs: &[(&'a str, &str)]) -> ActionsResult<Vec<PkgNamedPattern<'a>>> {
    pairs
        .iter()
        .map(|(label, pat)| {
            Regex::new(pat)
                .map(|re| (*label, re))
                .map_err(|e| ActionsError::BuiltinError {
                    reason: format!("package_scan regex error for '{label}': {e}"),
                })
        })
        .collect()
}

fn build_obfuscation_patterns<'a>() -> ActionsResult<Vec<PkgNamedPattern<'a>>> {
    compile_pkg_patterns(&[
        (
            "atob() with long argument",
            r"\batob\s*\(\s*['\x22][A-Za-z0-9+/=]{50,}",
        ),
        (
            "Buffer.from with base64 decode",
            r"\bBuffer\.from\s*\(.*['\x22]base64['\x22]",
        ),
        ("Python base64.b64decode", r"\bbase64\.b64decode\b"),
        ("Python base64.decodebytes", r"\bbase64\.decodebytes\b"),
        ("Ruby/Java Base64.decode", r"\bBase64\.decode"),
        (
            "Long hex-encoded sequence",
            r"(?:0x[0-9a-fA-F]{2}[,\s]*){10,}",
        ),
        (
            "Repeated hex escape sequences",
            r"(?:\\x[0-9a-fA-F]{2}){10,}",
        ),
        (
            "String.fromCharCode with multiple args",
            r"\bString\.fromCharCode\s*\([\d,\s]{20,}\)",
        ),
        (
            "Multiple chr() calls on same line",
            r"(?:chr\s*\(\s*\d+\s*\).*){4,}",
        ),
        ("Repeated unicode escapes", r"(?:\\u00[0-9a-fA-F]{2}){8,}"),
        (
            "eval with decode/unescape",
            r"\beval\s*\(.*(?:decode|unescape|atob|fromCharCode)",
        ),
        (
            "Function constructor with string",
            r"\bFunction\s*\(\s*['\x22]",
        ),
        (
            "decodeURIComponent with long encoded string",
            r"\bdecodeURIComponent\s*\(\s*['\x22](?:%[0-9a-fA-F]{2}){10,}",
        ),
        ("JSFuck-style obfuscation", r"\[\+\[\]\]\+|\!\!\[\]"),
        (
            "Object.prototype property assignment",
            r"\bObject\.prototype\s*\.\s*\w+\s*=",
        ),
        (
            "__proto__ property assignment",
            r"\[[\s'\x22]*__proto__[\s'\x22]*\]\s*=",
        ),
        (
            "defineProperty on prototype chain",
            r"\bObject\.defineProperty\s*\(\s*(?:Object\.prototype|[A-Za-z_$]\w*\.prototype)\b",
        ),
        (
            "constructor.prototype manipulation",
            r"\bconstructor\s*\.\s*prototype\s*\.\s*\w+\s*=",
        ),
        (
            "String.fromCharCode with spread operator",
            r"\bString\.fromCharCode\s*\(\s*\.\.\.",
        ),
        (
            "Large numeric array (potential char-code table)",
            r"\[\s*(?:\d{1,3}\s*,\s*){15,}\d{1,3}\s*\]",
        ),
        (
            "Array.map with fromCharCode assembly",
            r"\.map\s*\([^)]*fromCharCode",
        ),
        (
            "reduce/join char-code assembly",
            r"\.(?:reduce|join)\s*\(.*fromCharCode",
        ),
        (
            "ROT13 or Caesar cipher pattern",
            r"(?i)(?:charCodeAt|fromCharCode).*(?:\+\s*13|\-\s*13|%\s*26)",
        ),
        (
            "XOR decryption loop pattern",
            r"(?:charCodeAt|charAt).*\^\s*(?:0x[0-9a-fA-F]+|\d+)",
        ),
        (
            "Compressed payload (zlib/gzip inflate)",
            r"\b(?:zlib\.inflate|zlib\.gunzip|pako\.inflate|decompress)\s*\(",
        ),
    ])
}

fn build_dynamic_exec_patterns<'a>() -> ActionsResult<Vec<PkgNamedPattern<'a>>> {
    compile_pkg_patterns(&[
        (
            "eval with variable/concatenation",
            r"\beval\s*\([^'\x22)]+\)",
        ),
        ("new Function() constructor", r"\bnew\s+Function\s*\("),
        (
            "Python exec with compile/decode",
            r"\bexec\s*\(.*(?:compile|decode|b64decode|decompress)",
        ),
        ("Ruby eval with pack/unpack", r"\beval\b.*(?:pack|unpack)"),
        ("Python assert with exec", r"\bassert\b.*\bexec\s*\("),
        (
            "vm.runInNewContext (Node.js sandbox escape)",
            r"\bvm\.runInNewContext\b",
        ),
        (
            "vm.createContext (Node.js sandbox escape)",
            r"\bvm\.createContext\b",
        ),
        ("Reflect.apply with dynamic args", r"\bReflect\.apply\s*\("),
        ("Python __import__ dynamic import", r"\b__import__\s*\("),
        (
            "Python importlib.import_module",
            r"\bimportlib\.import_module\s*\(",
        ),
        (
            "require with non-literal argument",
            r"\brequire\s*\(\s*[^'\x22\s)]",
        ),
        ("dlopen dynamic library loading", r"\bdlopen\s*\("),
        (
            "ctypes.CDLL dynamic library loading",
            r"\bctypes\.CDLL\s*\(",
        ),
        (
            "Native addon require (.node file)",
            r"\brequire\s*\(['\x22][^'\x22]*\.node['\x22]\s*\)",
        ),
        (
            "WebAssembly.instantiate call",
            r"\bWebAssembly\.(?:instantiate|compile|instantiateStreaming)\s*\(",
        ),
        (
            "WebAssembly loaded from file",
            r"(?:readFileSync|readFile)\s*\([^)]*\.wasm",
        ),
        (
            "ffi-napi native binding",
            r"\bffi-napi\b|\brequire\s*\(['\x22]ffi-napi['\x22]\)",
        ),
    ])
}

fn build_suspicious_network_patterns<'a>() -> ActionsResult<Vec<PkgNamedPattern<'a>>> {
    compile_pkg_patterns(&[
        (
            "Node.js http.get/request",
            r"\bhttps?\.(?:get|request)\s*\(",
        ),
        ("XMLHttpRequest in dependency", r"\bXMLHttpRequest\b"),
        (
            "fetch() call in dependency",
            r"\bfetch\s*\(\s*['\x22]https?://",
        ),
        (
            "Python urllib.request.urlopen",
            r"\burllib\.request\.urlopen\b",
        ),
        ("Python urllib2.urlopen", r"\burllib2\.urlopen\b"),
        ("Python requests.get/post", r"\brequests\.(?:get|post)\s*\("),
        ("Ruby Net::HTTP", r"\bNet::HTTP\b"),
        (
            "Python socket.connect",
            r"\bsocket\.(?:connect|create_connection)\s*\(",
        ),
        (
            "Node.js net.connect/createConnection",
            r"\bnet\.(?:connect|createConnection)\s*\(",
        ),
        (
            "Node.js dns.resolve/lookup",
            r"\bdns\.(?:resolve|lookup)\s*\(",
        ),
        (
            "child_process with curl/wget",
            r"\bchild_process\b.*(?:curl|wget)",
        ),
        (
            "IP-based URL in dependency",
            r"https?://\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}",
        ),
        ("Onion domain reference", r"\.onion\b"),
        ("Telegram bot API URL", r"api\.telegram\.org/bot"),
        ("Discord webhook URL", r"discord(?:app)?\.com/api/webhooks"),
        (
            "DNS exfiltration OOB callback domain",
            r"(?i)(?:\.interactsh\.com|\.burpcollaborator\.net|canarytokens\.com|\.dnslog\.cn|\.requestcatcher\.com|pipedream\.net|\.oastify\.com)\b",
        ),
        (
            "DNS tunneling tool reference",
            r"\b(?:dnscat|iodine|heyoka|dns2tcp)\b",
        ),
        (
            "DNS resolve with template literal exfil",
            r"dns\.(?:resolve|lookup)\s*\(`\$\{",
        ),
        (
            "Data encoded into DNS subdomain",
            r"dns\.(?:resolve|lookup)\s*\(.*\+.*\+.*\.\s*['\x22]",
        ),
        (
            "Extremely high version number (dependency confusion signal)",
            r#""version"\s*:\s*"(?:9\d{3}|[1-9]\d{4,})\."#,
        ),
        (
            "ethers.js blockchain provider/contract (C2 channel)",
            r"\bnew\s+ethers\.(?:JsonRpcProvider|WebSocketProvider|Contract)\b",
        ),
        (
            "web3.js Contract call (blockchain C2)",
            r"\bnew\s+Web3\b|\bnew\s+web3\.eth\.Contract\b",
        ),
        (
            "IPFS gateway fetch",
            r"https?://(?:ipfs\.io|cloudflare-ipfs\.com|gateway\.pinata\.cloud|dweb\.link)/ipfs/",
        ),
        (
            "IPFS CID string literal",
            r"['\x22`](?:Qm[1-9A-HJ-NP-Za-km-z]{44}|bafy[a-z2-7]{55})['\x22`]",
        ),
        (
            "Infura RPC endpoint (blockchain node)",
            r"https?://[a-z-]+\.infura\.io/",
        ),
        (
            "Alchemy RPC endpoint (blockchain node)",
            r"https?://[a-z-]+-mainnet\.g\.alchemy\.com/",
        ),
        (
            "Rust reqwest HTTP call in dependency",
            r"\breqwest::(?:blocking::)?(?:get|post|Client)\b",
        ),
        (
            "Rust ureq HTTP call in dependency",
            r"\bureq::(?:get|post|agent)\b",
        ),
        (
            "publishConfig with non-standard registry",
            r#""(?:registry|publishConfig)"\s*:.*"https?://"#,
        ),
    ])
}

fn build_fs_tampering_patterns<'a>() -> ActionsResult<Vec<PkgNamedPattern<'a>>> {
    compile_pkg_patterns(&[
        (
            "fs.writeFile with path traversal",
            r"\bfs\.writeFile(?:Sync)?\s*\(.*\.\./",
        ),
        (
            "os.path.expanduser with write",
            r"\bos\.path\.expanduser\b.*(?:open|write)",
        ),
        (
            "path.join with homedir",
            r"\bpath\.join\s*\(.*(?:os\.homedir|process\.env\.HOME)",
        ),
        (
            "Reading SSH/AWS/NPM credentials",
            r"(?:\.ssh|\.aws|\.npmrc|\.gitconfig)\b.*(?:readFile|readFileSync|open\s*\()",
        ),
        (
            "Sensitive path read via readFileSync",
            r"\bfs\.readFileSync\s*\(.*(?:\.ssh|\.aws|\.npmrc|\.gitconfig)",
        ),
        (
            "Write to /tmp with exec/spawn",
            r"/tmp/.*(?:exec|spawn)|(?:exec|spawn).*?/tmp/",
        ),
        (
            "npm lifecycle hook data access",
            r"\bprocess\.env\.npm_package_",
        ),
        (
            "PATH environment modification",
            r"(?:process\.env\.PATH|os\.environ\[.PATH.\])\s*[+=]",
        ),
        (
            "chmod/chown permission change",
            r"\b(?:chmod|chown|fs\.chmod|fs\.chown)\s*\(",
        ),
        (
            "Write to .github/workflows/ from dependency",
            r"(?:writeFile|writeFileSync|open\s*\([^,]+['\x22]w)\s*.*\.github[/\\]workflows",
        ),
        (
            "GitHub Actions workflow file creation",
            r"\.github[/\\]workflows[/\\].*\.yml",
        ),
        (
            "AWS credentials file read",
            r"(?:readFile|readFileSync|open\s*\()\s*.*\.aws/credentials\b",
        ),
        (
            "Kubernetes kubeconfig read",
            r"(?:readFile|readFileSync|open\s*\()\s*.*\.kube/config\b",
        ),
        (
            "Docker config.json read (registry creds)",
            r"(?:readFile|readFileSync|open\s*\()\s*.*\.docker/config\.json\b",
        ),
        (
            "Terraform state file read",
            r"(?:readFile|readFileSync|open\s*\()\s*.*terraform\.tfstate\b",
        ),
        (
            "GCP default credentials read",
            r"(?:readFile|readFileSync|open\s*\()\s*.*application_default_credentials\.json\b",
        ),
    ])
}

fn build_install_hook_patterns<'a>() -> ActionsResult<Vec<PkgNamedPattern<'a>>> {
    compile_pkg_patterns(&[
        ("npm preinstall script", r#""\s*preinstall\s*"\s*:"#),
        ("npm postinstall script", r#""\s*postinstall\s*"\s*:"#),
        ("npm install script", r#""\s*install\s*"\s*:"#),
        ("setup.py cmdclass override", r"\bcmdclass\s*="),
        (
            "__init__.py with exec/eval on import",
            r"(?:exec|eval)\s*\(.*(?:compile|decode|open|read)",
        ),
        ("npmrc/pypirc credential access", r"(?:\.npmrc|\.pypirc)\b"),
        ("binding.gyp with suspicious actions", r"binding\.gyp\b"),
        (
            "Post-install shell script",
            r"(?:post_install|preinstall|postinstall)\.sh\b",
        ),
        (
            "node-gyp build in scripts",
            r#""(?:build|rebuild)"\s*:.*"node-gyp"#,
        ),
        ("node-pre-gyp native module", r"\bnode-pre-gyp\b"),
        (
            "napi build system",
            r"\b(?:napi-build|cmake-js|prebuild-install)\b",
        ),
        (
            "exports field with non-standard entry redirect",
            r#""exports"\s*:\s*\{[^}]*"\."#,
        ),
        (
            "main/module pointing outside package root",
            r#""(?:main|module)"\s*:\s*"\.\."#,
        ),
        (
            "bin entry pointing to shell script",
            r#""bin"\s*:\s*\{[^}]*:\s*"[^"]*\.sh""#,
        ),
    ])
}

fn build_exfiltration_patterns<'a>() -> ActionsResult<Vec<PkgNamedPattern<'a>>> {
    compile_pkg_patterns(&[
        (
            "os.hostname/userInfo collection",
            r"\bos\.(?:hostname|userInfo)\s*\(",
        ),
        (
            "JSON.stringify(process.env) serialization",
            r"\bJSON\.stringify\s*\(\s*process\.env\s*\)",
        ),
        (
            "whoami/hostname command execution",
            r"(?:exec|spawn|system)\s*\(.*(?:whoami|hostname|uname)\b",
        ),
        (
            "Parent package.json read",
            r"(?:readFile|readFileSync|require)\s*\(.*\.\./.*package\.json",
        ),
        (
            "Clipboard access in Node.js context",
            r"\bnavigator\.clipboard\b",
        ),
        ("Cookie access in Node.js context", r"\bdocument\.cookie\b"),
        (
            "OS keychain/keytar access",
            r"\b(?:keytar|keychain)\b.*(?:get|find|read)",
        ),
        (
            "SSH private key file read",
            r"(?:id_rsa|id_ed25519|id_ecdsa)\b.*(?:readFile|readFileSync|open\s*\()",
        ),
        (
            "SSH key read via file path",
            r"(?:readFile|readFileSync|open\s*\().*(?:id_rsa|id_ed25519|id_ecdsa)\b",
        ),
        (
            "Environment exfil via DNS",
            r"dns\.(?:resolve|lookup)\s*\(.*(?:process\.env|environ|getenv|ENV\[)",
        ),
        ("ngrok tunnel endpoint", r"(?i)\.ngrok\.io\b"),
    ])
}

fn build_persistence_patterns<'a>() -> ActionsResult<Vec<PkgNamedPattern<'a>>> {
    compile_pkg_patterns(&[
        ("crontab write/manipulation", r"\bcrontab\s+-[eli]\b"),
        (
            "Cron directory write",
            r"(?:writeFile|writeFileSync|open\s*\().*(?:/etc/cron|/var/spool/cron)",
        ),
        (
            "Shell RC file write (persistence)",
            r"(?:writeFile|writeFileSync|open\s*\().*(?:\.bashrc|\.zshrc|\.profile|\.bash_profile)",
        ),
        (
            "SSH authorized_keys modification",
            r"(?:writeFile|writeFileSync|open\s*\().*authorized_keys",
        ),
        (
            "Git hooks injection",
            r"(?:writeFile|writeFileSync|open\s*\().*\.git[/\\]hooks[/\\]",
        ),
        (
            "systemd unit installation",
            r"(?:/etc/systemd/system/|/usr/lib/systemd/system/).*\.service|systemctl\s+(?:enable|daemon-reload)",
        ),
        (
            "macOS launchd plist installation",
            r"(?:Library[/\\]LaunchAgents|Library[/\\]LaunchDaemons)[/\\].*\.plist|launchctl\s+(?:load|bootstrap)",
        ),
        ("Windows scheduled task creation", r"\bschtasks\s+/create\b"),
        (
            "Windows registry Run key persistence",
            r"(?i)(?:HKCU|HKLM)\\\\Software\\\\Microsoft\\\\Windows\\\\CurrentVersion\\\\Run",
        ),
    ])
}

/// Compute Shannon entropy (bits per byte) over a byte slice.
fn shannon_entropy(s: &[u8]) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut freq = [0u32; 256];
    for &b in s {
        freq[b as usize] += 1;
    }
    // Precision loss is acceptable: entropy calculation is inherently approximate,
    // and string lengths exceeding 2^52 bytes are not realistic.
    #[allow(clippy::cast_precision_loss)]
    let len = s.len() as f64;
    let mut entropy = 0.0_f64;
    for &count in &freq {
        if count > 0 {
            let p = f64::from(count) / len;
            entropy -= p * p.log2();
        }
    }
    entropy
}

/// Minimum string literal length to consider for entropy analysis.
const ENTROPY_MIN_STRING_LEN: usize = 64;

/// Entropy threshold (bits/byte) above which a string is flagged.
const ENTROPY_THRESHOLD: f64 = 5.2;

/// Scan a line for high-entropy string literals that may contain encrypted payloads.
fn check_high_entropy_strings(
    line: &str,
    rel: &Path,
    line_num: usize,
    findings: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    // Extract string literals (single-quoted, double-quoted)
    for delim in ['"', '\''] {
        let mut chars = line.char_indices();
        while let Some((start, ch)) = chars.next() {
            if ch == delim {
                let content_start = start + 1;
                let mut escaped = false;
                let mut end = None;
                for (idx, c) in chars.by_ref() {
                    if escaped {
                        escaped = false;
                        continue;
                    }
                    if c == '\\' {
                        escaped = true;
                        continue;
                    }
                    if c == delim {
                        end = Some(idx);
                        break;
                    }
                }
                if let Some(end_idx) = end {
                    let literal = &line[content_start..end_idx];
                    if literal.len() >= ENTROPY_MIN_STRING_LEN {
                        let entropy = shannon_entropy(literal.as_bytes());
                        if entropy > ENTROPY_THRESHOLD {
                            let finding = format!(
                                "  {}:{}: [obfuscation] High-entropy string literal ({:.1} bits/byte, {} chars)",
                                rel.display(),
                                line_num + 1,
                                entropy,
                                literal.len(),
                            );
                            if seen.insert(finding.clone()) {
                                findings.push(finding);
                            }
                        }
                    }
                }
            }
        }
    }
}
