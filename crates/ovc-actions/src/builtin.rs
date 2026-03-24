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
                !(name.starts_with('.')
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
