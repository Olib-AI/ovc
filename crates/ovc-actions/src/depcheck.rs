//! Dependency update checker — scans manifests and queries public registries.
//!
//! Supports Cargo, npm, `PyPI`, Go modules, `RubyGems`, Composer, Maven, Pub,
//! `CocoaPods`, `NuGet`, and Hex (Elixir).
//!
//! Network requests are bounded by a semaphore (max 5 concurrent) and a
//! per-request timeout. The action degrades gracefully when offline.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;
use regex::Regex;
use serde::Deserialize;
use tokio::sync::Semaphore;
use walkdir::WalkDir;

use crate::runner::ActionStatus;

// ── Configuration ────────────────────────────────────────────────────────────

struct CheckConfig {
    check_dev: bool,
    ignore: Vec<String>,
    level: UpdateLevel,
    timeout_secs: u64,
}

impl CheckConfig {
    fn from_yaml(config: &serde_yaml::Value) -> Self {
        let check_dev = config
            .get("check_dev")
            .and_then(serde_yaml::Value::as_bool)
            .unwrap_or(true);

        let ignore: Vec<String> = config
            .get("ignore")
            .and_then(serde_yaml::Value::as_sequence)
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();

        let level = config
            .get("level")
            .and_then(serde_yaml::Value::as_str)
            .and_then(|s| match s {
                "major" => Some(UpdateLevel::Major),
                "minor" => Some(UpdateLevel::Minor),
                "patch" => Some(UpdateLevel::Patch),
                _ => None,
            })
            .unwrap_or(UpdateLevel::Minor);

        let timeout_secs = config
            .get("timeout_secs")
            .and_then(serde_yaml::Value::as_u64)
            .unwrap_or(30);

        Self {
            check_dev,
            ignore,
            level,
            timeout_secs,
        }
    }
}

/// Minimum update severity to report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum UpdateLevel {
    Patch = 0,
    Minor = 1,
    Major = 2,
}

// ── Core types ───────────────────────────────────────────────────────────────

/// A single dependency parsed from a manifest file.
struct Dependency {
    name: String,
    current_version: String,
    source: String,
    dev: bool,
    ecosystem: Ecosystem,
}

/// Supported package manager ecosystems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Ecosystem {
    Cargo,
    Npm,
    PyPi,
    Go,
    RubyGems,
    Composer,
    Maven,
    Pub,
    CocoaPods,
    NuGet,
    Hex,
}

/// Classification of an available update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateType {
    Major,
    Minor,
    Patch,
    /// Could not parse one or both versions as semver.
    Unknown,
    /// Already at the latest version.
    UpToDate,
}

impl UpdateType {
    /// Human-readable label for this update type.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Major => "major",
            Self::Minor => "minor",
            Self::Patch => "patch",
            Self::Unknown => "unknown",
            Self::UpToDate => "up-to-date",
        }
    }
}

/// A resolved dependency with latest-version information.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DependencyStatus {
    pub name: String,
    pub current_version: String,
    pub latest_version: String,
    pub update_type: UpdateType,
    pub dev: bool,
}

/// All statuses for a single manifest file.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ManifestReport {
    pub file: String,
    pub package_manager: String,
    pub dependencies: Vec<DependencyStatus>,
}

/// Structured summary of the full dependency check.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DependencyReport {
    pub manifests: Vec<ManifestReport>,
    pub total_updates: usize,
    pub major_updates: usize,
    pub minor_updates: usize,
    pub patch_updates: usize,
}

// ── Entry points ─────────────────────────────────────────────────────────────

/// Run the dependency update check and return the `(ActionStatus, human-readable output)` pair.
///
/// Called from `builtin.rs` via `tokio::task::block_in_place` + `block_on`.
pub async fn check_dependencies(
    repo_root: &Path,
    config: &serde_yaml::Value,
) -> (ActionStatus, String) {
    let cfg = CheckConfig::from_yaml(config);
    let report = run_check(repo_root, &cfg).await;

    let has_updates = report.total_updates > 0;
    let output = render_text_report(&report, cfg.level);
    let status = if has_updates {
        ActionStatus::Failed
    } else {
        ActionStatus::Passed
    };

    (status, output)
}

/// Build a structured `DependencyReport` for the REST API endpoint.
///
/// Uses the default config (`check_dev=true`, `level=minor`, `timeout=30s`).
pub async fn build_report(repo_root: &Path, config: &serde_yaml::Value) -> DependencyReport {
    let cfg = CheckConfig::from_yaml(config);
    run_check(repo_root, &cfg).await
}

// ── Core check logic ─────────────────────────────────────────────────────────

async fn run_check(repo_root: &Path, cfg: &CheckConfig) -> DependencyReport {
    let manifests = discover_manifests(repo_root);
    if manifests.is_empty() {
        return empty_report();
    }

    let all_deps: Vec<Dependency> = manifests
        .iter()
        .flat_map(|path| parse_manifest(path, repo_root))
        .filter(|dep| !cfg.ignore.contains(&dep.name))
        .filter(|dep| cfg.check_dev || !dep.dev)
        .collect();

    if all_deps.is_empty() {
        return empty_report();
    }

    let client = match reqwest::Client::builder()
        .user_agent("ovc/0.1.0 (dependency-update-checker)")
        .timeout(Duration::from_secs(cfg.timeout_secs))
        .build()
    {
        Ok(c) => Arc::new(c),
        Err(_) => return empty_report(),
    };

    let source_order = manifest_source_order(&manifests, repo_root);
    let resolved = resolve_versions(all_deps, &client).await;
    assemble_report(resolved, &source_order)
}

/// Fetch all latest versions from registries, returning `(source, DependencyStatus)` pairs.
async fn resolve_versions(
    deps: Vec<Dependency>,
    client: &Arc<reqwest::Client>,
) -> Vec<(String, DependencyStatus)> {
    let sem = Arc::new(Semaphore::new(5));
    futures::stream::iter(deps)
        .map(|dep| {
            let client = Arc::clone(client);
            let sem = Arc::clone(&sem);
            async move {
                let _permit = sem.acquire().await.ok();
                let latest = fetch_latest_version(&client, &dep).await;
                let latest_version = latest.unwrap_or_default();
                let update_type = if latest_version.is_empty() {
                    UpdateType::Unknown
                } else {
                    classify_update(&dep.current_version, &latest_version)
                };
                (
                    dep.source,
                    DependencyStatus {
                        name: dep.name,
                        current_version: dep.current_version,
                        latest_version,
                        update_type,
                        dev: dep.dev,
                    },
                )
            }
        })
        .buffer_unordered(5)
        .collect()
        .await
}

/// Return manifest source paths in discovery order (deduped).
fn manifest_source_order(manifests: &[PathBuf], repo_root: &Path) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for path in manifests {
        let rel = rel_path(path, repo_root);
        if !seen.contains(&rel) {
            seen.push(rel);
        }
    }
    seen
}

/// Group resolved statuses by source file and build the final report.
fn assemble_report(
    resolved: Vec<(String, DependencyStatus)>,
    source_order: &[String],
) -> DependencyReport {
    let mut by_source: HashMap<String, Vec<DependencyStatus>> = HashMap::new();
    let mut seen_triples: HashSet<(String, String, String)> = HashSet::new();

    for (src, status) in resolved {
        let triple = (
            src.clone(),
            status.name.clone(),
            status.current_version.clone(),
        );
        if seen_triples.insert(triple) {
            by_source.entry(src).or_default().push(status);
        }
    }

    let mut report_manifests: Vec<ManifestReport> = Vec::new();
    let mut total_updates = 0usize;
    let mut major_updates = 0usize;
    let mut minor_updates = 0usize;
    let mut patch_updates = 0usize;

    for src in source_order {
        let Some(deps) = by_source.get(src.as_str()) else {
            continue;
        };
        for s in deps {
            match s.update_type {
                UpdateType::Major => {
                    total_updates += 1;
                    major_updates += 1;
                }
                UpdateType::Minor => {
                    total_updates += 1;
                    minor_updates += 1;
                }
                UpdateType::Patch => {
                    total_updates += 1;
                    patch_updates += 1;
                }
                UpdateType::Unknown | UpdateType::UpToDate => {}
            }
        }
        report_manifests.push(ManifestReport {
            file: src.clone(),
            package_manager: ecosystem_for_source(src).to_owned(),
            dependencies: deps.clone(),
        });
    }

    DependencyReport {
        manifests: report_manifests,
        total_updates,
        major_updates,
        minor_updates,
        patch_updates,
    }
}

const fn empty_report() -> DependencyReport {
    DependencyReport {
        manifests: Vec::new(),
        total_updates: 0,
        major_updates: 0,
        minor_updates: 0,
        patch_updates: 0,
    }
}

fn rel_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

// ── Manifest discovery ───────────────────────────────────────────────────────

const MANIFEST_NAMES: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "requirements.txt",
    "pyproject.toml",
    "go.mod",
    "Gemfile",
    "composer.json",
    "pom.xml",
    "pubspec.yaml",
    "mix.exs",
    "Podfile",
];

fn discover_manifests(repo_root: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();

    WalkDir::new(repo_root)
        .follow_links(false)
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
        .for_each(|entry| {
            let file_name = entry.file_name().to_string_lossy();
            if MANIFEST_NAMES.contains(&file_name.as_ref()) || file_name.ends_with(".csproj") {
                paths.push(entry.into_path());
            }
        });

    paths
}

fn ecosystem_for_source(src: &str) -> &'static str {
    let base = src.rsplit(['/', '\\']).next().unwrap_or(src);
    match base {
        "Cargo.toml" => "Cargo",
        "package.json" => "npm",
        "requirements.txt" | "pyproject.toml" => "PyPI",
        "go.mod" => "Go modules",
        "Gemfile" => "RubyGems",
        "composer.json" => "Composer",
        "pom.xml" => "Maven",
        "pubspec.yaml" => "Pub",
        "mix.exs" => "Hex (Elixir)",
        "Podfile" => "CocoaPods",
        _ if base.ends_with(".csproj") => "NuGet",
        _ => "Unknown",
    }
}

// ── Manifest parsers ─────────────────────────────────────────────────────────

fn parse_manifest(path: &Path, repo_root: &Path) -> Vec<Dependency> {
    let source = rel_path(path, repo_root);

    let base = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };

    match base.as_str() {
        "Cargo.toml" => parse_cargo_toml(&contents, &source),
        "package.json" => parse_package_json(&contents, &source),
        "requirements.txt" => parse_requirements_txt(&contents, &source),
        "pyproject.toml" => parse_pyproject_toml(&contents, &source),
        "go.mod" => parse_go_mod(&contents, &source),
        "Gemfile" => parse_gemfile(&contents, &source),
        "composer.json" => parse_composer_json(&contents, &source),
        "pom.xml" => parse_pom_xml(&contents, &source),
        "pubspec.yaml" => parse_pubspec_yaml(&contents, &source),
        "mix.exs" => parse_mix_exs(&contents, &source),
        "Podfile" => parse_podfile(&contents, &source),
        name if name.ends_with(".csproj") => parse_csproj(&contents, &source),
        _ => Vec::new(),
    }
}

// ── Cargo.toml ───────────────────────────────────────────────────────────────

/// `Cargo.toml` fields that contain metadata strings rather than dependency versions.
const TOML_META_KEYS: &[&str] = &[
    "edition",
    "version",
    "name",
    "authors",
    "license",
    "description",
    "repository",
    "homepage",
    "documentation",
    "readme",
    "keywords",
    "categories",
    "rust-version",
    "build",
    "links",
    "publish",
    "exclude",
    "include",
    "default-run",
    "autobins",
    "autoexamples",
    "autotests",
    "autobenches",
    "resolver",
];

#[derive(Clone, Copy)]
enum CargoSection {
    None,
    Deps { dev: bool },
}

fn parse_cargo_toml(contents: &str, source: &str) -> Vec<Dependency> {
    // Matches inline table with version: `name = { version = "1.0", ... }`
    let re_table = Regex::new(r#"^\s*([A-Za-z0-9_\-]+)\s*=\s*\{[^}]*version\s*=\s*"([^"]+)""#)
        .expect("static regex");
    // Matches bare string: `name = "1.0"`
    let re_bare = Regex::new(r#"^\s*([A-Za-z0-9_\-]+)\s*=\s*"([^"]+)""#).expect("static regex");
    // Matches workspace-inherited: `name = { workspace = true ... }`
    let re_workspace =
        Regex::new(r"^\s*[A-Za-z0-9_\-]+\s*=\s*\{[^}]*workspace\s*=\s*true").expect("static regex");

    let mut deps = Vec::new();
    let mut section = CargoSection::None;

    for line in contents.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') {
            section = if trimmed.starts_with("[dependencies]")
                || trimmed.starts_with("[workspace.dependencies]")
                || trimmed.starts_with("[build-dependencies]")
            {
                CargoSection::Deps { dev: false }
            } else if trimmed.starts_with("[dev-dependencies]") {
                CargoSection::Deps { dev: true }
            } else {
                CargoSection::None
            };
            continue;
        }

        let CargoSection::Deps { dev } = section else {
            continue;
        };

        if re_workspace.is_match(trimmed) {
            continue;
        }

        if let Some(cap) = re_table.captures(trimmed) {
            let name = cap[1].to_owned();
            let version = cap[2].to_owned();
            if !version.is_empty() && !name.starts_with('#') {
                deps.push(Dependency {
                    name,
                    current_version: version,
                    source: source.to_owned(),
                    dev,
                    ecosystem: Ecosystem::Cargo,
                });
            }
            continue;
        }

        if let Some(cap) = re_bare.captures(trimmed) {
            let name = cap[1].to_owned();
            let version = cap[2].to_owned();
            if !name.starts_with('#') && !TOML_META_KEYS.contains(&name.as_str()) {
                deps.push(Dependency {
                    name,
                    current_version: version,
                    source: source.to_owned(),
                    dev,
                    ecosystem: Ecosystem::Cargo,
                });
            }
        }
    }

    deps
}

// ── package.json ─────────────────────────────────────────────────────────────

fn parse_package_json(contents: &str, source: &str) -> Vec<Dependency> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(contents) else {
        return Vec::new();
    };

    let mut deps = Vec::new();

    let extract = |section: Option<&serde_json::Value>, dev: bool, out: &mut Vec<Dependency>| {
        let Some(map) = section.and_then(|v| v.as_object()) else {
            return;
        };
        for (name, ver_val) in map {
            if let Some(ver) = ver_val.as_str() {
                if ver.starts_with("file:") || ver.starts_with("git+") {
                    continue;
                }
                out.push(Dependency {
                    name: name.clone(),
                    current_version: ver.to_owned(),
                    source: source.to_owned(),
                    dev,
                    ecosystem: Ecosystem::Npm,
                });
            }
        }
    };

    if let Some(o) = json.as_object() {
        extract(o.get("dependencies"), false, &mut deps);
        extract(o.get("devDependencies"), true, &mut deps);
        extract(o.get("peerDependencies"), true, &mut deps);
    }

    deps
}

// ── requirements.txt ─────────────────────────────────────────────────────────

fn parse_requirements_txt(contents: &str, source: &str) -> Vec<Dependency> {
    let re = Regex::new(r"^([A-Za-z0-9_\-\.]+)\s*(?:==|>=|~=|<=|!=|>|<)\s*([0-9][^\s;#]*)")
        .expect("static regex");
    let mut deps = Vec::new();

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
            continue;
        }
        if let Some(cap) = re.captures(line) {
            deps.push(Dependency {
                name: normalize_pypi_name(&cap[1]),
                current_version: cap[2].trim().to_owned(),
                source: source.to_owned(),
                dev: false,
                ecosystem: Ecosystem::PyPi,
            });
        }
    }

    deps
}

fn normalize_pypi_name(name: &str) -> String {
    name.to_lowercase().replace(['_', '.'], "-")
}

// ── pyproject.toml ───────────────────────────────────────────────────────────

fn parse_pyproject_toml(contents: &str, source: &str) -> Vec<Dependency> {
    // Capture `package>=1.0` or `"package>=1.0"` patterns inside dependency arrays.
    let re =
        Regex::new(r#"['""]?([A-Za-z0-9_\-\.]+)\s*(?:==|>=|~=|<=)\s*([0-9][^"',\s\]]*)['""]?"#)
            .expect("static regex");
    let mut deps = Vec::new();
    let mut in_deps = false;

    for line in contents.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("dependencies") || trimmed == "[project.dependencies]" {
            in_deps = true;
            continue;
        }
        if trimmed.starts_with('[') && !trimmed.starts_with("[project") {
            in_deps = false;
        }

        if in_deps {
            for cap in re.captures_iter(trimmed) {
                let name = normalize_pypi_name(&cap[1]);
                let ver = cap[2].trim().to_owned();
                if !ver.is_empty() && !name.is_empty() {
                    deps.push(Dependency {
                        name,
                        current_version: ver,
                        source: source.to_owned(),
                        dev: false,
                        ecosystem: Ecosystem::PyPi,
                    });
                }
            }
        }
    }

    deps
}

// ── go.mod ───────────────────────────────────────────────────────────────────

fn parse_go_mod(contents: &str, source: &str) -> Vec<Dependency> {
    let re_single = Regex::new(r"^require\s+(\S+)\s+(v[0-9][^\s]*)").expect("static regex");
    let re_block = Regex::new(r"^\t(\S+)\s+(v[0-9][^\s]*)").expect("static regex");

    let mut deps = Vec::new();
    let mut in_block = false;

    for line in contents.lines() {
        let trimmed = line.trim();

        if trimmed == "require (" {
            in_block = true;
            continue;
        }
        if trimmed == ")" {
            in_block = false;
            continue;
        }

        if let Some(cap) = re_single.captures(trimmed) {
            deps.push(Dependency {
                name: cap[1].to_owned(),
                current_version: cap[2].trim_start_matches('v').to_owned(),
                source: source.to_owned(),
                dev: false,
                ecosystem: Ecosystem::Go,
            });
        } else if in_block && let Some(cap) = re_block.captures(line) {
            deps.push(Dependency {
                name: cap[1].to_owned(),
                current_version: cap[2].trim_start_matches('v').to_owned(),
                source: source.to_owned(),
                dev: false,
                ecosystem: Ecosystem::Go,
            });
        }
    }

    deps
}

// ── Gemfile ──────────────────────────────────────────────────────────────────

fn parse_gemfile(contents: &str, source: &str) -> Vec<Dependency> {
    let re = Regex::new(r#"^\s*gem\s+['"]([^'"]+)['"]\s*,\s*['"][~><=\s]*([0-9][^'"]*)['""]"#)
        .expect("static regex");
    let re_no_ver = Regex::new(r#"^\s*gem\s+['"]([^'"]+)['"]"#).expect("static regex");

    let mut deps = Vec::new();

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(cap) = re.captures(trimmed) {
            deps.push(Dependency {
                name: cap[1].to_owned(),
                current_version: cap[2].trim().to_owned(),
                source: source.to_owned(),
                dev: false,
                ecosystem: Ecosystem::RubyGems,
            });
        } else if let Some(cap) = re_no_ver.captures(trimmed) {
            deps.push(Dependency {
                name: cap[1].to_owned(),
                current_version: String::new(),
                source: source.to_owned(),
                dev: false,
                ecosystem: Ecosystem::RubyGems,
            });
        }
    }

    deps
}

// ── composer.json ────────────────────────────────────────────────────────────

fn parse_composer_json(contents: &str, source: &str) -> Vec<Dependency> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(contents) else {
        return Vec::new();
    };

    let mut deps = Vec::new();

    let extract = |section: Option<&serde_json::Value>, dev: bool, out: &mut Vec<Dependency>| {
        let Some(map) = section.and_then(|v| v.as_object()) else {
            return;
        };
        for (name, ver_val) in map {
            if name == "php" || name.starts_with("ext-") {
                continue;
            }
            if let Some(ver) = ver_val.as_str() {
                out.push(Dependency {
                    name: name.clone(),
                    current_version: ver.to_owned(),
                    source: source.to_owned(),
                    dev,
                    ecosystem: Ecosystem::Composer,
                });
            }
        }
    };

    if let Some(obj) = json.as_object() {
        extract(obj.get("require"), false, &mut deps);
        extract(obj.get("require-dev"), true, &mut deps);
    }

    deps
}

// ── pom.xml ──────────────────────────────────────────────────────────────────

fn parse_pom_xml(contents: &str, source: &str) -> Vec<Dependency> {
    let re_group = Regex::new(r"<groupId>([^<]+)</groupId>").expect("static regex");
    let re_artifact = Regex::new(r"<artifactId>([^<]+)</artifactId>").expect("static regex");
    let re_version = Regex::new(r"<version>([^<$\{]+)</version>").expect("static regex");

    let mut deps = Vec::new();
    let mut in_dep = false;
    let mut group = String::new();
    let mut artifact = String::new();
    let mut version = String::new();

    for line in contents.lines() {
        let trimmed = line.trim();

        if trimmed.contains("<dependency>") {
            in_dep = true;
            group.clear();
            artifact.clear();
            version.clear();
            continue;
        }
        if trimmed.contains("</dependency>") {
            if in_dep && !artifact.is_empty() && !version.is_empty() {
                let name = if group.is_empty() {
                    artifact.clone()
                } else {
                    format!("{group}:{artifact}")
                };
                deps.push(Dependency {
                    name,
                    current_version: version.clone(),
                    source: source.to_owned(),
                    dev: false,
                    ecosystem: Ecosystem::Maven,
                });
            }
            in_dep = false;
            continue;
        }

        if !in_dep {
            continue;
        }

        if let Some(cap) = re_group.captures(trimmed) {
            cap[1].trim().clone_into(&mut group);
        }
        if let Some(cap) = re_artifact.captures(trimmed) {
            cap[1].trim().clone_into(&mut artifact);
        }
        if let Some(cap) = re_version.captures(trimmed) {
            cap[1].trim().clone_into(&mut version);
        }
    }

    deps
}

// ── pubspec.yaml ─────────────────────────────────────────────────────────────

fn parse_pubspec_yaml(contents: &str, source: &str) -> Vec<Dependency> {
    let Ok(yaml) = serde_yaml::from_str::<serde_yaml::Value>(contents) else {
        return Vec::new();
    };

    let mut deps = Vec::new();

    let extract = |section: Option<&serde_yaml::Value>, dev: bool, out: &mut Vec<Dependency>| {
        let Some(map) = section.and_then(|v| v.as_mapping()) else {
            return;
        };
        for (k, v) in map {
            let Some(name) = k.as_str() else { continue };
            // Value can be a bare version string or a mapping with `version:`.
            let version: String = match v {
                serde_yaml::Value::String(s) => strip_version_prefix(s).to_owned(),
                serde_yaml::Value::Mapping(m) => m
                    .get("version")
                    .and_then(|vv| vv.as_str())
                    .map(|s| strip_version_prefix(s).to_owned())
                    .unwrap_or_default(),
                _ => String::new(),
            };
            if !version.is_empty() {
                out.push(Dependency {
                    name: name.to_owned(),
                    current_version: version,
                    source: source.to_owned(),
                    dev,
                    ecosystem: Ecosystem::Pub,
                });
            }
        }
    };

    extract(yaml.get("dependencies"), false, &mut deps);
    extract(yaml.get("dev_dependencies"), true, &mut deps);

    deps
}

// ── mix.exs ──────────────────────────────────────────────────────────────────

fn parse_mix_exs(contents: &str, source: &str) -> Vec<Dependency> {
    // {:phoenix, "~> 1.7"} or {:ecto, "3.11.0"}
    let re = Regex::new(r#"\{:([A-Za-z0-9_]+)\s*,\s*"([^"]+)""#).expect("static regex");
    let mut deps = Vec::new();

    for cap in re.captures_iter(contents) {
        let name = cap[1].to_owned();
        let version = strip_version_prefix(&cap[2]).to_owned();
        if !version.is_empty() {
            deps.push(Dependency {
                name,
                current_version: version,
                source: source.to_owned(),
                dev: false,
                ecosystem: Ecosystem::Hex,
            });
        }
    }

    deps
}

// ── Podfile ──────────────────────────────────────────────────────────────────

fn parse_podfile(contents: &str, source: &str) -> Vec<Dependency> {
    let re = Regex::new(r#"^\s*pod\s+['"]([^'"]+)['"]\s*,\s*['"][~><=\s]*([0-9][^'"]*)['""]"#)
        .expect("static regex");
    let re_no_ver = Regex::new(r#"^\s*pod\s+['"]([^'"]+)['"]"#).expect("static regex");

    let mut deps = Vec::new();

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(cap) = re.captures(trimmed) {
            deps.push(Dependency {
                name: cap[1].to_owned(),
                current_version: cap[2].trim().to_owned(),
                source: source.to_owned(),
                dev: false,
                ecosystem: Ecosystem::CocoaPods,
            });
        } else if let Some(cap) = re_no_ver.captures(trimmed) {
            deps.push(Dependency {
                name: cap[1].to_owned(),
                current_version: String::new(),
                source: source.to_owned(),
                dev: false,
                ecosystem: Ecosystem::CocoaPods,
            });
        }
    }

    deps
}

// ── *.csproj ─────────────────────────────────────────────────────────────────

fn parse_csproj(contents: &str, source: &str) -> Vec<Dependency> {
    // Match both attribute orderings on a single (joined) line.
    let re_inc_ver =
        Regex::new(r#"<PackageReference[^>]+Include\s*=\s*"([^"]+)"[^>]+Version\s*=\s*"([^"]+)""#)
            .expect("static regex");
    let re_ver_inc =
        Regex::new(r#"<PackageReference[^>]+Version\s*=\s*"([^"]+)"[^>]+Include\s*=\s*"([^"]+)""#)
            .expect("static regex");

    let mut deps = Vec::new();
    // Join lines to handle multi-line `<PackageReference>` elements.
    let flat: String = contents.lines().collect::<Vec<_>>().join(" ");

    for cap in re_inc_ver.captures_iter(&flat) {
        deps.push(Dependency {
            name: cap[1].to_owned(),
            current_version: cap[2].to_owned(),
            source: source.to_owned(),
            dev: false,
            ecosystem: Ecosystem::NuGet,
        });
    }
    for cap in re_ver_inc.captures_iter(&flat) {
        deps.push(Dependency {
            name: cap[2].to_owned(),
            current_version: cap[1].to_owned(),
            source: source.to_owned(),
            dev: false,
            ecosystem: Ecosystem::NuGet,
        });
    }

    deps
}

// ── Version utilities ────────────────────────────────────────────────────────

/// Strip common constraint prefixes: `^`, `~`, `>=`, `<=`, `~>`, `>`, `<`, `=`, `v`.
fn strip_version_prefix(ver: &str) -> &str {
    ver.trim_start_matches(['^', '~', '>', '<', '=', ' ', 'v'])
}

#[derive(Debug, Clone, Copy)]
struct SemVer {
    major: u64,
    minor: u64,
    patch: u64,
}

impl SemVer {
    fn parse(ver: &str) -> Option<Self> {
        let stripped = strip_version_prefix(ver);
        let base: String = stripped
            .chars()
            .take_while(|&c| c.is_ascii_digit() || c == '.')
            .collect();
        let mut parts = base.split('.').filter(|s| !s.is_empty());
        let major: u64 = parts.next()?.parse().ok()?;
        let minor: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let patch: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

/// Public re-export so the API layer can classify an update type without
/// duplicating the semver logic.
#[must_use]
pub fn classify_update_pub(current: &str, latest: &str) -> String {
    classify_update(current, latest).label().to_owned()
}

/// Extract the currently declared version of `dep_name` from a manifest file's
/// raw text content.
///
/// Returns `None` when the dependency is not found or the manifest format is
/// not supported. The caller should treat an `None` or empty string as
/// "unknown".
#[must_use]
pub fn extract_version_pub(content: &str, manifest_type: &str, dep_name: &str) -> Option<String> {
    let base = manifest_type
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(manifest_type);
    match base {
        "Cargo.toml" => {
            let re_table = Regex::new(&format!(
                r#"(?m)^\s*{dep}\s*=\s*\{{[^}}]*version\s*=\s*"([^"]+)""#,
                dep = regex::escape(dep_name),
            ))
            .ok()?;
            let re_bare = Regex::new(&format!(
                r#"(?m)^\s*{dep}\s*=\s*"([^"]+)""#,
                dep = regex::escape(dep_name),
            ))
            .ok()?;
            re_table
                .captures(content)
                .or_else(|| re_bare.captures(content))
                .map(|c| c[1].to_owned())
        }
        "package.json" | "composer.json" => {
            let json: serde_json::Value = serde_json::from_str(content).ok()?;
            let sections = [
                "dependencies",
                "devDependencies",
                "peerDependencies",
                "require",
                "require-dev",
            ];
            for sec in &sections {
                if let Some(v) = json
                    .get(*sec)
                    .and_then(|s| s.get(dep_name))
                    .and_then(|v| v.as_str())
                {
                    return Some(v.to_owned());
                }
            }
            None
        }
        "requirements.txt" => {
            let re = Regex::new(&format!(
                r"(?im)^{name}\s*(?:==|>=|~=|<=|!=|>|<)\s*([0-9][^\s;#]*)",
                name = regex::escape(dep_name),
            ))
            .ok()?;
            re.captures(content).map(|c| c[1].trim().to_owned())
        }
        "go.mod" => {
            let re = Regex::new(&format!(
                r"(?m)^\s*{module}\s+v([0-9][^\s]*)",
                module = regex::escape(dep_name),
            ))
            .ok()?;
            re.captures(content).map(|c| c[1].to_owned())
        }
        _ => None,
    }
}

fn classify_update(current: &str, latest: &str) -> UpdateType {
    let Some(cur) = SemVer::parse(current) else {
        return UpdateType::Unknown;
    };
    let Some(lat) = SemVer::parse(latest) else {
        return UpdateType::Unknown;
    };

    if lat.major > cur.major {
        UpdateType::Major
    } else if lat.major == cur.major && lat.minor > cur.minor {
        UpdateType::Minor
    } else if lat.major == cur.major && lat.minor == cur.minor && lat.patch > cur.patch {
        UpdateType::Patch
    } else {
        UpdateType::UpToDate
    }
}

// ── Registry clients ─────────────────────────────────────────────────────────

async fn fetch_latest_version(client: &reqwest::Client, dep: &Dependency) -> Option<String> {
    match dep.ecosystem {
        Ecosystem::Cargo => fetch_crates_io(client, &dep.name).await,
        Ecosystem::Npm => fetch_npmjs(client, &dep.name).await,
        Ecosystem::PyPi => fetch_pypi(client, &dep.name).await,
        Ecosystem::Go => fetch_go_proxy(client, &dep.name).await,
        Ecosystem::RubyGems => fetch_rubygems(client, &dep.name).await,
        Ecosystem::Composer => fetch_packagist(client, &dep.name).await,
        Ecosystem::Maven => fetch_maven_central(client, &dep.name).await,
        Ecosystem::Pub => fetch_pub_dev(client, &dep.name).await,
        Ecosystem::CocoaPods => fetch_cocoapods(client, &dep.name).await,
        Ecosystem::NuGet => fetch_nuget(client, &dep.name).await,
        Ecosystem::Hex => fetch_hex(client, &dep.name).await,
    }
}

// crates.io

#[derive(Deserialize)]
struct CratesIoResponse {
    #[serde(rename = "crate")]
    krate: CratesIoCrate,
}

#[derive(Deserialize)]
struct CratesIoCrate {
    newest_version: String,
}

async fn fetch_crates_io(client: &reqwest::Client, name: &str) -> Option<String> {
    let url = format!("https://crates.io/api/v1/crates/{name}");
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .ok()?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return None;
    }
    let data: CratesIoResponse = resp.json().await.ok()?;
    Some(data.krate.newest_version)
}

// npmjs.org

#[derive(Deserialize)]
struct NpmResponse {
    version: String,
}

async fn fetch_npmjs(client: &reqwest::Client, name: &str) -> Option<String> {
    let encoded = url_encode_npm(name);
    let url = format!("https://registry.npmjs.org/{encoded}/latest");
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .ok()?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return None;
    }
    let data: NpmResponse = resp.json().await.ok()?;
    Some(data.version)
}

/// URL-encode npm scoped package names: `@org/pkg` → `%40org%2Fpkg`.
fn url_encode_npm(name: &str) -> String {
    if name.starts_with('@') {
        name.replace('@', "%40").replace('/', "%2F")
    } else {
        name.to_owned()
    }
}

// PyPI

#[derive(Deserialize)]
struct PyPiResponse {
    info: PyPiInfo,
}

#[derive(Deserialize)]
struct PyPiInfo {
    version: String,
}

async fn fetch_pypi(client: &reqwest::Client, name: &str) -> Option<String> {
    let url = format!("https://pypi.org/pypi/{name}/json");
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .ok()?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return None;
    }
    let data: PyPiResponse = resp.json().await.ok()?;
    Some(data.info.version)
}

// Go proxy

#[derive(Deserialize)]
struct GoProxyResponse {
    #[serde(rename = "Version")]
    version: String,
}

async fn fetch_go_proxy(client: &reqwest::Client, module: &str) -> Option<String> {
    let encoded = go_module_encode(module);
    let url = format!("https://proxy.golang.org/{encoded}/@latest");
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .ok()?;
    if matches!(
        resp.status(),
        reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::GONE
    ) {
        return None;
    }
    let data: GoProxyResponse = resp.json().await.ok()?;
    Some(data.version.trim_start_matches('v').to_owned())
}

/// Encode Go module path per the proxy protocol (uppercase letters → `!lowercase`).
fn go_module_encode(module: &str) -> String {
    let mut out = String::with_capacity(module.len() + 8);
    for ch in module.chars() {
        if ch.is_uppercase() {
            out.push('!');
            for lc in ch.to_lowercase() {
                out.push(lc);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

// RubyGems

#[derive(Deserialize)]
struct RubyGemsResponse {
    version: String,
}

async fn fetch_rubygems(client: &reqwest::Client, name: &str) -> Option<String> {
    let url = format!("https://rubygems.org/api/v1/gems/{name}.json");
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .ok()?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return None;
    }
    let data: RubyGemsResponse = resp.json().await.ok()?;
    Some(data.version)
}

// Packagist (Composer)

async fn fetch_packagist(client: &reqwest::Client, name: &str) -> Option<String> {
    let url = format!("https://repo.packagist.org/p2/{name}.json");
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .ok()?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    let packages = json.get("packages")?.as_object()?;
    let versions_arr = packages.get(name)?.as_array()?;
    let first = versions_arr.first()?;
    let ver = first.get("version")?.as_str()?;
    Some(ver.trim_start_matches('v').to_owned())
}

// Maven Central

async fn fetch_maven_central(client: &reqwest::Client, name: &str) -> Option<String> {
    let (group, artifact) = name.split_once(':')?;
    let url = format!(
        "https://search.maven.org/solrsearch/select?q=g:{group}+AND+a:{artifact}&rows=1&wt=json"
    );
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    let docs = json.get("response")?.get("docs")?.as_array()?;
    Some(docs.first()?.get("latestVersion")?.as_str()?.to_owned())
}

// pub.dev (Dart)

#[derive(Deserialize)]
struct PubDevResponse {
    latest: PubDevLatest,
}

#[derive(Deserialize)]
struct PubDevLatest {
    version: String,
}

async fn fetch_pub_dev(client: &reqwest::Client, name: &str) -> Option<String> {
    let url = format!("https://pub.dev/api/packages/{name}");
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .ok()?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return None;
    }
    let data: PubDevResponse = resp.json().await.ok()?;
    Some(data.latest.version)
}

// CocoaPods trunk

async fn fetch_cocoapods(client: &reqwest::Client, name: &str) -> Option<String> {
    let url = format!("https://trunk.cocoapods.org/api/v1/pods/{name}");
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .ok()?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    // `versions` is an array of objects; last entry is most recent.
    let versions = json.get("versions")?.as_array()?;
    Some(versions.last()?.get("name")?.as_str()?.to_owned())
}

// NuGet

async fn fetch_nuget(client: &reqwest::Client, name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    let url = format!("https://api.nuget.org/v3-flatcontainer/{lower}/index.json");
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .ok()?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    // `{ "versions": ["1.0.0", "1.1.0", ...] }` — filter out pre-releases.
    let versions = json.get("versions")?.as_array()?;
    let stable: Vec<&str> = versions
        .iter()
        .filter_map(|v| v.as_str())
        .filter(|v| !v.contains('-'))
        .collect();
    stable.last().map(|s| (*s).to_owned())
}

// Hex (Elixir)

async fn fetch_hex(client: &reqwest::Client, name: &str) -> Option<String> {
    let url = format!("https://hex.pm/api/packages/{name}");
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .ok()?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    Some(json.get("latest_stable_version")?.as_str()?.to_owned())
}

// ── Text report rendering ────────────────────────────────────────────────────

fn render_text_report(report: &DependencyReport, minimum_level: UpdateLevel) -> String {
    let mut out = String::new();

    let _ = writeln!(out, "Dependency Update Report");
    let _ = writeln!(out, "========================");

    if report.manifests.is_empty() {
        let _ = writeln!(out, "\nNo dependency manifest files found.");
        return out;
    }

    for manifest in &report.manifests {
        let relevant: Vec<&DependencyStatus> = manifest
            .dependencies
            .iter()
            .filter(|s| update_meets_level(s.update_type, minimum_level))
            .collect();

        let _ = writeln!(out);

        if relevant.is_empty() {
            let _ = writeln!(
                out,
                "{} ({}): all dependencies up to date",
                manifest.file, manifest.package_manager
            );
            continue;
        }

        let _ = writeln!(
            out,
            "{} ({}) - {} update{} available:",
            manifest.file,
            manifest.package_manager,
            relevant.len(),
            if relevant.len() == 1 { "" } else { "s" },
        );

        let max_name = relevant
            .iter()
            .map(|s| s.name.len())
            .max()
            .unwrap_or(10)
            .max(10);
        let max_cur = relevant
            .iter()
            .map(|s| s.current_version.len())
            .max()
            .unwrap_or(7)
            .max(7);

        for s in &relevant {
            let icon = match s.update_type {
                UpdateType::Major => "!",
                UpdateType::Minor | UpdateType::Patch => "*",
                _ => "?",
            };
            let dev_tag = if s.dev { " [dev]" } else { "" };
            let _ = writeln!(
                out,
                "  [{icon}] {name:<name_w$}  {cur:<cur_w$} -> {lat}  ({label}){dev}",
                icon = icon,
                name = s.name,
                name_w = max_name,
                cur = s.current_version,
                cur_w = max_cur,
                lat = s.latest_version,
                label = s.update_type.label(),
                dev = dev_tag,
            );
        }
    }

    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Summary: {} update{} available ({} major, {} minor, {} patch)",
        report.total_updates,
        if report.total_updates == 1 { "" } else { "s" },
        report.major_updates,
        report.minor_updates,
        report.patch_updates,
    );

    out
}

const fn update_meets_level(ut: UpdateType, minimum: UpdateLevel) -> bool {
    matches!(
        (ut, minimum),
        (UpdateType::Patch, UpdateLevel::Patch)
            | (UpdateType::Minor, UpdateLevel::Patch | UpdateLevel::Minor)
            | (UpdateType::Major, _)
    )
}

// ── Manifest version updaters ────────────────────────────────────────────────

/// Update a dependency version in a manifest file, returning the modified
/// content, or `None` if the dependency was not found / the format is
/// unrecognised.
///
/// The function preserves every syntactic decoration that the original line
/// carried (operator prefixes like `^`, `~`, `>=`; TOML inline-table fields;
/// XML surrounding elements, etc.) — only the bare version number is
/// substituted.
#[must_use]
pub fn update_manifest_version(
    content: &str,
    manifest_type: &str,
    dep_name: &str,
    new_version: &str,
) -> Option<String> {
    let base = manifest_type
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(manifest_type);
    match base {
        "Cargo.toml" => update_cargo_toml(content, dep_name, new_version),
        "package.json" | "composer.json" => update_json_dep(content, dep_name, new_version),
        "requirements.txt" => update_requirements_txt(content, dep_name, new_version),
        "go.mod" => update_go_mod(content, dep_name, new_version),
        "Gemfile" => update_gemfile(content, dep_name, new_version),
        "pubspec.yaml" => update_pubspec_yaml(content, dep_name, new_version),
        "pom.xml" => update_pom_xml(content, dep_name, new_version),
        "mix.exs" => update_mix_exs(content, dep_name, new_version),
        "Podfile" => update_podfile(content, dep_name, new_version),
        name if name.ends_with(".csproj") => update_csproj(content, dep_name, new_version),
        _ => None,
    }
}

// ── Per-ecosystem updaters ───────────────────────────────────────────────────

/// Update `Cargo.toml`. Handles both:
///   `dep = "1.0"`
///   `dep = { version = "1.0", features = [...] }`
fn update_cargo_toml(content: &str, dep_name: &str, new_version: &str) -> Option<String> {
    // Inline table form: `name = { version = "OLD", ... }`
    let re_table = Regex::new(&format!(
        r#"(?m)^(\s*{dep}\s*=\s*\{{[^}}]*version\s*=\s*")[^"]+("[^}}]*\}})"#,
        dep = regex::escape(dep_name),
    ))
    .expect("static regex");

    // Bare string form: `name = "OLD"`
    let re_bare = Regex::new(&format!(
        r#"(?m)^(\s*{dep}\s*=\s*")[^"]+(")"#,
        dep = regex::escape(dep_name),
    ))
    .expect("static regex");

    let result = if re_table.is_match(content) {
        re_table
            .replace(content, |caps: &regex::Captures<'_>| {
                format!("{}{new_version}{}", &caps[1], &caps[2])
            })
            .into_owned()
    } else if re_bare.is_match(content) {
        re_bare
            .replace(content, |caps: &regex::Captures<'_>| {
                format!("{}{new_version}{}", &caps[1], &caps[2])
            })
            .into_owned()
    } else {
        return None;
    };

    if result == content {
        None
    } else {
        Some(result)
    }
}

/// Update `package.json` or `composer.json`.
/// Preserves operator prefixes (`^`, `~`, `>=`, etc.) on the existing value.
fn update_json_dep(content: &str, dep_name: &str, new_version: &str) -> Option<String> {
    let Ok(mut json) = serde_json::from_str::<serde_json::Value>(content) else {
        return None;
    };

    let dep_sections = [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "require",
        "require-dev",
    ];
    let mut updated = false;

    for section in &dep_sections {
        if let Some(map) = json.get_mut(*section).and_then(|v| v.as_object_mut())
            && let Some(val) = map.get_mut(dep_name)
            && let Some(old_str) = val.as_str()
        {
            // Preserve leading operator prefix characters.
            let prefix: String = old_str
                .chars()
                .take_while(|c| matches!(c, '^' | '~' | '>' | '<' | '=' | ' '))
                .collect();
            *val = serde_json::Value::String(format!("{prefix}{new_version}"));
            updated = true;
        }
    }

    if !updated {
        return None;
    }

    // Re-serialize with pretty-printing to preserve human-readable formatting.
    serde_json::to_string_pretty(&json).ok()
}

/// Update `requirements.txt`. Preserves the operator (`==`, `>=`, `~=`, etc.).
fn update_requirements_txt(content: &str, dep_name: &str, new_version: &str) -> Option<String> {
    let re = Regex::new(&format!(
        r"(?im)^({name}\s*(?:==|>=|~=|<=|!=|>|<)\s*)[0-9][^\s;#]*",
        name = regex::escape(dep_name),
    ))
    .expect("static regex");

    if !re.is_match(content) {
        return None;
    }

    let result = re
        .replace(content, |caps: &regex::Captures<'_>| {
            format!("{}{new_version}", &caps[1])
        })
        .into_owned();

    if result == content {
        None
    } else {
        Some(result)
    }
}

/// Update `go.mod`. Handles both block and single-line `require` forms.
/// The `v` prefix before the version is preserved.
fn update_go_mod(content: &str, dep_name: &str, new_version: &str) -> Option<String> {
    // go.mod always uses `vMAJOR.MINOR.PATCH`.
    let versioned = if new_version.starts_with('v') {
        new_version.to_owned()
    } else {
        format!("v{new_version}")
    };

    let re = Regex::new(&format!(
        r"(?m)^(\s*{module}\s+)v[0-9][^\s]*",
        module = regex::escape(dep_name),
    ))
    .expect("static regex");

    if !re.is_match(content) {
        return None;
    }

    let result = re
        .replace_all(content, |caps: &regex::Captures<'_>| {
            format!("{}{versioned}", &caps[1])
        })
        .into_owned();

    if result == content {
        None
    } else {
        Some(result)
    }
}

/// Update `Gemfile`. Preserves the operator (`~>`, `>=`, etc.).
fn update_gemfile(content: &str, dep_name: &str, new_version: &str) -> Option<String> {
    // gem 'name', '~> 1.2' or gem "name", ">= 1.2"
    let re = Regex::new(&format!(
        r#"(?m)^(\s*gem\s+['"{name}'"]\s*,\s*['"])[~><=\s]*[0-9][^'"]*(['"])"#,
        name = regex::escape(dep_name),
    ))
    .expect("static regex");

    // Alternative: look for the name and capture the quote/operator block.
    let re2 = Regex::new(&format!(
        r#"(?m)^(\s*gem\s+['"]{name}['"]\s*,\s*['"])([\~><=\s]*)[0-9][^'"]*(['"])"#,
        name = regex::escape(dep_name),
    ))
    .expect("static regex");

    if !re2.is_match(content) {
        return None;
    }

    let result = re2
        .replace(content, |caps: &regex::Captures<'_>| {
            format!("{}{}{new_version}{}", &caps[1], &caps[2], &caps[3])
        })
        .into_owned();

    let _ = re; // silence unused warning — re2 covers everything
    if result == content {
        None
    } else {
        Some(result)
    }
}

/// Update `pubspec.yaml`. Handles bare string values and `version:` keys.
fn update_pubspec_yaml(content: &str, dep_name: &str, new_version: &str) -> Option<String> {
    let Ok(mut yaml) = serde_yaml::from_str::<serde_yaml::Value>(content) else {
        return None;
    };

    let sections = ["dependencies", "dev_dependencies"];
    let mut updated = false;

    for section in &sections {
        let Some(mapping) = yaml.get_mut(*section).and_then(|v| v.as_mapping_mut()) else {
            continue;
        };

        let key = serde_yaml::Value::String(dep_name.to_owned());
        if let Some(val) = mapping.get_mut(&key) {
            match val {
                serde_yaml::Value::String(s) => {
                    // Preserve prefix chars like `^`, `>=`, `~`.
                    let prefix: String = s
                        .chars()
                        .take_while(|c| matches!(c, '^' | '~' | '>' | '<' | '=' | ' '))
                        .collect();
                    *s = format!("{prefix}{new_version}");
                    updated = true;
                }
                serde_yaml::Value::Mapping(m) => {
                    let ver_key = serde_yaml::Value::String("version".to_owned());
                    if let Some(v) = m.get_mut(&ver_key)
                        && let serde_yaml::Value::String(s) = v
                    {
                        let prefix: String = s
                            .chars()
                            .take_while(|c| matches!(c, '^' | '~' | '>' | '<' | '=' | ' '))
                            .collect();
                        *s = format!("{prefix}{new_version}");
                        updated = true;
                    }
                }
                _ => {}
            }
        }
    }

    if !updated {
        return None;
    }

    serde_yaml::to_string(&yaml).ok()
}

/// Update `pom.xml`. Replaces `<version>OLD</version>` inside the correct
/// `<dependency>` block (matched by `<artifactId>`).
fn update_pom_xml(content: &str, dep_name: &str, new_version: &str) -> Option<String> {
    // We parse line-by-line, tracking whether we are inside the right
    // <dependency> block, and replace the version tag when we are.
    // dep_name may be "groupId:artifactId" or just "artifactId".
    let (target_group, target_artifact) = if let Some((g, a)) = dep_name.split_once(':') {
        (Some(g), a)
    } else {
        (None, dep_name)
    };

    let mut result = String::with_capacity(content.len());
    let mut in_dep = false;
    let mut found_artifact = false;
    let mut found_group = target_group.is_none(); // no filter needed if unspecified
    let mut replaced = false;

    // Buffer lines inside a <dependency> block until we know whether to update.
    let re_ver = Regex::new(r"(<version>)[^<]*(</version>)").expect("static regex");
    let mut dep_buf: Vec<String> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.contains("<dependency>") {
            in_dep = true;
            found_artifact = false;
            found_group = target_group.is_none();
            dep_buf.clear();
            dep_buf.push(line.to_owned());
            continue;
        }

        if trimmed.contains("</dependency>") {
            // Flush buffered lines, possibly with version substitution.
            if found_artifact && found_group {
                for buf_line in &dep_buf {
                    if !replaced && re_ver.is_match(buf_line) {
                        let updated_line = re_ver
                            .replace(buf_line, |caps: &regex::Captures<'_>| {
                                format!("{}{new_version}{}", &caps[1], &caps[2])
                            })
                            .into_owned();
                        result.push_str(&updated_line);
                        replaced = true;
                    } else {
                        result.push_str(buf_line);
                    }
                    result.push('\n');
                }
            } else {
                for buf_line in &dep_buf {
                    result.push_str(buf_line);
                    result.push('\n');
                }
            }
            dep_buf.clear();
            result.push_str(line);
            result.push('\n');
            in_dep = false;
            continue;
        }

        if in_dep {
            if trimmed.contains(&format!("<artifactId>{target_artifact}</artifactId>")) {
                found_artifact = true;
            }
            if let Some(tg) = target_group
                && trimmed.contains(&format!("<groupId>{tg}</groupId>"))
            {
                found_group = true;
            }
            dep_buf.push(line.to_owned());
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }

    if replaced {
        // Trim the extra trailing newline added by the loop.
        if result.ends_with('\n') && !content.ends_with('\n') {
            result.truncate(result.len() - 1);
        }
        Some(result)
    } else {
        None
    }
}

/// Update `mix.exs`. Handles `{:name, "~> 1.2"}` tuples.
fn update_mix_exs(content: &str, dep_name: &str, new_version: &str) -> Option<String> {
    let re = Regex::new(&format!(
        r#"(\{{:{name}\s*,\s*")([~><=\s]*)[0-9][^"]*(")"#,
        name = regex::escape(dep_name),
    ))
    .expect("static regex");

    if !re.is_match(content) {
        return None;
    }

    let result = re
        .replace(content, |caps: &regex::Captures<'_>| {
            format!("{}{}{new_version}{}", &caps[1], &caps[2], &caps[3])
        })
        .into_owned();

    if result == content {
        None
    } else {
        Some(result)
    }
}

/// Update `Podfile`. Preserves operator prefix.
fn update_podfile(content: &str, dep_name: &str, new_version: &str) -> Option<String> {
    let re = Regex::new(&format!(
        r#"(?m)^(\s*pod\s+['"]{name}['"]\s*,\s*['"])([\~><=\s]*)[0-9][^'"]*(['"])"#,
        name = regex::escape(dep_name),
    ))
    .expect("static regex");

    if !re.is_match(content) {
        return None;
    }

    let result = re
        .replace(content, |caps: &regex::Captures<'_>| {
            format!("{}{}{new_version}{}", &caps[1], &caps[2], &caps[3])
        })
        .into_owned();

    if result == content {
        None
    } else {
        Some(result)
    }
}

/// Update `*.csproj`. Replaces `Version="OLD"` in the matching
/// `<PackageReference Include="name" ...>` element.
fn update_csproj(content: &str, dep_name: &str, new_version: &str) -> Option<String> {
    // Match the entire PackageReference element (single line) and update Version.
    let re = Regex::new(&format!(
        r#"(<PackageReference[^>]+Include\s*=\s*"{name}"[^>]+Version\s*=\s*")[^"]+(")"#,
        name = regex::escape(dep_name),
    ))
    .expect("static regex");
    let re_rev = Regex::new(&format!(
        r#"(<PackageReference[^>]+Version\s*=\s*")[^"]+("[^>]+Include\s*=\s*"{name}"[^>]*)"#,
        name = regex::escape(dep_name),
    ))
    .expect("static regex");

    // Join lines to handle multi-line elements (same technique as the parser).
    let flat: String = content.lines().collect::<Vec<_>>().join(" ");

    let (matched_re, new_flat) = if re.is_match(&flat) {
        let r = re
            .replace(&flat, |caps: &regex::Captures<'_>| {
                format!("{}{new_version}{}", &caps[1], &caps[2])
            })
            .into_owned();
        (true, r)
    } else if re_rev.is_match(&flat) {
        let r = re_rev
            .replace(&flat, |caps: &regex::Captures<'_>| {
                format!("{}{new_version}{}", &caps[1], &caps[2])
            })
            .into_owned();
        (true, r)
    } else {
        return None;
    };

    if !matched_re || new_flat == flat {
        return None;
    }

    // The flat join was purely for matching. To avoid mangling the file
    // structure we apply the version substitution on a per-line basis
    // by finding the line containing the PackageReference for this dep
    // and substituting its Version attribute in place.
    let ver_in_line_re = Regex::new(&format!(
        r#"(Include\s*=\s*"{name}"[^>]*Version\s*=\s*"|Version\s*=\s*")[^"]+(")"#,
        name = regex::escape(dep_name),
    ))
    .expect("static regex");

    let mut updated = false;
    let result: String = content
        .lines()
        .map(|line| {
            if !updated && line.contains(dep_name) && ver_in_line_re.is_match(line) {
                let new_line = ver_in_line_re
                    .replace(line, |caps: &regex::Captures<'_>| {
                        format!("{}{new_version}{}", &caps[1], &caps[2])
                    })
                    .into_owned();
                updated = true;
                new_line
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Preserve trailing newline if original had one.
    let result = if content.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    };

    if updated { Some(result) } else { None }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_bare() {
        let v = SemVer::parse("1.2.3").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
    }

    #[test]
    fn semver_caret_prefix() {
        let v = SemVer::parse("^1.2.3").unwrap();
        assert_eq!(v.major, 1);
    }

    #[test]
    fn semver_v_prefix() {
        let v = SemVer::parse("v0.14.0").unwrap();
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 14);
    }

    #[test]
    fn semver_two_parts() {
        let v = SemVer::parse("1.2").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 0);
    }

    #[test]
    fn classify_major_bump() {
        assert_eq!(classify_update("1.0.0", "2.0.0"), UpdateType::Major);
    }

    #[test]
    fn classify_minor_bump() {
        assert_eq!(classify_update("1.0.0", "1.1.0"), UpdateType::Minor);
    }

    #[test]
    fn classify_patch_bump() {
        assert_eq!(classify_update("1.0.0", "1.0.1"), UpdateType::Patch);
    }

    #[test]
    fn classify_up_to_date() {
        assert_eq!(classify_update("1.2.3", "1.2.3"), UpdateType::UpToDate);
    }

    #[test]
    fn cargo_bare_string() {
        let toml = "[dependencies]\nserde = \"1.0.190\"\ntokio = \"1.35.0\"\n\n[dev-dependencies]\ntempfile = \"3.8.0\"\n";
        let deps = parse_cargo_toml(toml, "Cargo.toml");
        assert!(
            deps.iter()
                .any(|d| d.name == "serde" && d.current_version == "1.0.190")
        );
        assert!(deps.iter().any(|d| d.name == "tokio"));
        let tf = deps.iter().find(|d| d.name == "tempfile").unwrap();
        assert!(tf.dev);
    }

    #[test]
    fn cargo_inline_table() {
        let toml = "[dependencies]\nserde = { version = \"1.0\", features = [\"derive\"] }\nreqwest = { version = \"0.12\", default-features = false }\n";
        let deps = parse_cargo_toml(toml, "Cargo.toml");
        assert!(
            deps.iter()
                .any(|d| d.name == "serde" && d.current_version == "1.0")
        );
        assert!(
            deps.iter()
                .any(|d| d.name == "reqwest" && d.current_version == "0.12")
        );
    }

    #[test]
    fn cargo_workspace_dep_skipped() {
        let toml = "[dependencies]\novc-core = { workspace = true }\nserde = \"1.0\"\n";
        let deps = parse_cargo_toml(toml, "Cargo.toml");
        assert!(deps.iter().all(|d| d.name != "ovc-core"));
        assert!(deps.iter().any(|d| d.name == "serde"));
    }

    #[test]
    fn package_json_basic() {
        let json = r#"{"dependencies":{"react":"^19.0.0","axios":"1.6.0"},"devDependencies":{"typescript":"~5.3.0"}}"#;
        let deps = parse_package_json(json, "package.json");
        assert!(deps.iter().any(|d| d.name == "react"));
        let ts = deps.iter().find(|d| d.name == "typescript").unwrap();
        assert!(ts.dev);
    }

    #[test]
    fn requirements_txt_basic() {
        let txt = "flask==2.0.1\nrequests>=2.28.0\n# comment\n-r other.txt\n";
        let deps = parse_requirements_txt(txt, "requirements.txt");
        assert!(
            deps.iter()
                .any(|d| d.name == "flask" && d.current_version == "2.0.1")
        );
        assert!(
            deps.iter()
                .any(|d| d.name == "requests" && d.current_version == "2.28.0")
        );
    }

    #[test]
    fn go_mod_block() {
        let go = "module example.com/app\n\ngo 1.21\n\nrequire (\n\tgolang.org/x/text v0.14.0\n\tgithub.com/gin-gonic/gin v1.9.1\n)\n";
        let deps = parse_go_mod(go, "go.mod");
        assert!(
            deps.iter()
                .any(|d| d.name == "golang.org/x/text" && d.current_version == "0.14.0")
        );
    }

    #[test]
    fn mix_exs_basic() {
        let exs =
            "defp deps do\n  [\n    {:phoenix, \"~> 1.7\"},\n    {:ecto, \"3.11.0\"},\n  ]\nend\n";
        let deps = parse_mix_exs(exs, "mix.exs");
        assert!(
            deps.iter()
                .any(|d| d.name == "phoenix" && d.current_version == "1.7")
        );
        assert!(
            deps.iter()
                .any(|d| d.name == "ecto" && d.current_version == "3.11.0")
        );
    }

    #[test]
    fn normalize_pypi_name_basic() {
        assert_eq!(normalize_pypi_name("Flask"), "flask");
        assert_eq!(normalize_pypi_name("my_package"), "my-package");
        assert_eq!(normalize_pypi_name("my.pkg"), "my-pkg");
    }

    #[test]
    fn go_encode_uppercase() {
        assert_eq!(
            go_module_encode("github.com/BurntSushi/toml"),
            "github.com/!burnt!sushi/toml"
        );
    }

    #[test]
    fn update_meets_level_rules() {
        assert!(update_meets_level(UpdateType::Major, UpdateLevel::Minor));
        assert!(update_meets_level(UpdateType::Major, UpdateLevel::Major));
        assert!(update_meets_level(UpdateType::Minor, UpdateLevel::Minor));
        assert!(!update_meets_level(UpdateType::Minor, UpdateLevel::Major));
        assert!(update_meets_level(UpdateType::Patch, UpdateLevel::Patch));
        assert!(!update_meets_level(UpdateType::Patch, UpdateLevel::Minor));
        assert!(!update_meets_level(
            UpdateType::UpToDate,
            UpdateLevel::Patch
        ));
    }

    // ── update_manifest_version tests ───────────────────────────────────────

    #[test]
    fn update_cargo_bare_string() {
        let toml = "[dependencies]\nserde = \"1.0.0\"\ntokio = \"1.35.0\"\n";
        let result = update_manifest_version(toml, "Cargo.toml", "tokio", "1.50.0").unwrap();
        assert!(result.contains("tokio = \"1.50.0\""));
        assert!(
            result.contains("serde = \"1.0.0\""),
            "unrelated dep unchanged"
        );
    }

    #[test]
    fn update_cargo_inline_table() {
        let toml = "[dependencies]\ntokio = { version = \"1.35.0\", features = [\"full\"] }\n";
        let result = update_manifest_version(toml, "Cargo.toml", "tokio", "1.50.0").unwrap();
        assert!(result.contains("version = \"1.50.0\""));
        assert!(
            result.contains("features = [\"full\"]"),
            "features preserved"
        );
    }

    #[test]
    fn update_cargo_not_found_returns_none() {
        let toml = "[dependencies]\nserde = \"1.0.0\"\n";
        assert!(update_manifest_version(toml, "Cargo.toml", "tokio", "1.50.0").is_none());
    }

    #[test]
    fn update_package_json_dep() {
        let json = r#"{"dependencies":{"react":"^18.0.0"},"devDependencies":{"vite":"4.0.0"}}"#;
        let result = update_manifest_version(json, "package.json", "react", "19.1.0").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["dependencies"]["react"], "^19.1.0");
        assert_eq!(
            parsed["devDependencies"]["vite"], "4.0.0",
            "unrelated dep preserved"
        );
    }

    #[test]
    fn update_requirements_txt_exact() {
        let req = "flask==2.3.0\nrequests>=2.28.0\n";
        let result = update_manifest_version(req, "requirements.txt", "flask", "3.0.0").unwrap();
        assert!(result.contains("flask==3.0.0"));
        assert!(
            result.contains("requests>=2.28.0"),
            "unrelated dep unchanged"
        );
    }

    #[test]
    fn update_go_mod_require() {
        let go = "require (\n\tgithub.com/foo/bar v1.2.3\n)\n";
        let result = update_manifest_version(go, "go.mod", "github.com/foo/bar", "1.3.0").unwrap();
        assert!(result.contains("github.com/foo/bar v1.3.0"));
    }

    #[test]
    fn update_mix_exs_tilde() {
        let mix = "  defp deps do\n    [{:phoenix, \"~> 1.7\"}]\n  end\n";
        let result = update_manifest_version(mix, "mix.exs", "phoenix", "1.8.0").unwrap();
        assert!(result.contains("{:phoenix, \"~> 1.8.0\"}"));
    }
}
