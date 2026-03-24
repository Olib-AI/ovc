//! CLI execution context: resolved paths, password handling, repo discovery.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use ovc_core::ignore::IgnoreRules;
use ovc_core::repository::Repository;
use ovc_core::workdir::WorkDir;
use zeroize::Zeroizing;

use crate::app::Cli;

/// Execution context built from CLI arguments.
pub struct CliContext {
    /// Explicit repo path from `--repo` or `OVC_REPO`.
    pub repo_path: Option<PathBuf>,
    /// Current working directory.
    pub cwd: PathBuf,
}

impl CliContext {
    /// Constructs a context from parsed CLI arguments.
    pub fn from_cli(cli: &Cli) -> Result<Self> {
        let cwd =
            std::env::current_dir().context("failed to determine current working directory")?;
        Ok(Self {
            repo_path: cli.repo.clone(),
            cwd,
        })
    }

    /// Discovers the `.ovc` file by walking up from the current directory.
    ///
    /// If `--repo` was specified, uses that path directly.
    pub fn find_ovc_file(&self) -> Result<PathBuf> {
        if let Some(ref explicit) = self.repo_path {
            if explicit.exists() {
                // Canonicalize to resolve symlinks (e.g. /tmp -> /private/tmp on macOS)
                // so that the working directory derived from this path matches the
                // filesystem paths returned by directory scans.
                return explicit
                    .canonicalize()
                    .context("failed to canonicalize repository path");
            }
            bail!(
                "specified repository file does not exist: {}",
                explicit.display()
            );
        }

        let mut dir = self.cwd.clone();
        loop {
            // First look for a .ovc file directly.
            if let Some(found) = find_ovc_in_dir(&dir) {
                // Canonicalize to resolve symlinks.
                return found
                    .canonicalize()
                    .context("failed to canonicalize repository path");
            }

            // Then look for a .ovc-link file that points to a remote .ovc file.
            if let Some(linked) = find_ovc_link_in_dir(&dir) {
                return Ok(linked);
            }

            if !dir.pop() {
                break;
            }
        }
        bail!(
            "not an OVC repository (or any parent up to /): no .ovc file found from {}",
            self.cwd.display()
        );
    }

    /// Returns the working directory root.
    ///
    /// If a `.ovc-link` exists in the current directory (or ancestors), the workdir
    /// is the directory containing the link (not the remote `.ovc` file location).
    /// Otherwise, the workdir is the parent directory of the `.ovc` file.
    pub fn workdir_for_with_cwd(ovc_path: &Path, cwd: &Path) -> Result<WorkDir> {
        // Walk up from cwd looking for .ovc-link — if found, that dir is the workdir
        let mut dir = cwd.to_path_buf();
        loop {
            let link = dir.join(".ovc-link");
            if link.is_file() {
                return Ok(WorkDir::new(dir));
            }
            if !dir.pop() {
                break;
            }
        }
        // No link found — workdir is the parent of the .ovc file
        let parent = ovc_path
            .parent()
            .context("cannot determine parent directory of .ovc file")?;
        Ok(WorkDir::new(parent.to_path_buf()))
    }

    /// Obtains the repository password from the environment or interactive prompt.
    ///
    /// The returned string is wrapped in [`Zeroizing`] to ensure the password
    /// is securely wiped from heap memory when no longer needed.
    pub fn get_password() -> Result<Zeroizing<String>> {
        if let Ok(pw) = std::env::var("OVC_PASSWORD") {
            return Ok(Zeroizing::new(pw));
        }

        let password = dialoguer::Password::new()
            .with_prompt("Repository password")
            .interact()
            .context("failed to read password")?;

        Ok(Zeroizing::new(password))
    }

    /// Opens an existing repository: discovers `.ovc` file, gets password or key, opens.
    ///
    /// Checks `OVC_KEY` env var first for key-based auth. If set, loads the
    /// specified key and opens with it. Otherwise falls back to password.
    pub fn open_repo(&self) -> Result<(Repository, WorkDir)> {
        let ovc_path = self.find_ovc_file()?;
        let workdir = Self::workdir_for_with_cwd(&ovc_path, &self.cwd)?;

        // Check for key-based auth via environment variable.
        //
        // If the key doesn't match any slot in the repo (password-only repo or
        // key not registered), fall through to password-based auth rather than
        // failing hard. This handles the common case where OVC_KEY is set
        // globally but some repos were created with passwords only.
        if let Ok(key_query) = std::env::var("OVC_KEY") {
            let key_attempt = (|| -> Result<Option<Repository>> {
                let Some(pub_path) =
                    ovc_core::keys::find_key(&key_query).context("failed to search for key")?
                else {
                    return Ok(None); // key not found — fall through to password auth
                };

                let priv_path = ovc_core::keys::private_key_path_for(&pub_path);
                if !priv_path.exists() {
                    // Private key absent — fall through to password auth.
                    return Ok(None);
                }

                let passphrase = if let Ok(pw) = std::env::var("OVC_KEY_PASSPHRASE") {
                    pw
                } else {
                    dialoguer::Password::new()
                        .with_prompt("Key passphrase")
                        .interact()
                        .context("failed to read key passphrase")?
                };

                let keypair =
                    ovc_core::keys::OvcKeyPair::load_private(&priv_path, passphrase.as_bytes())
                        .context("failed to load private key")?;

                match Repository::open_with_key(&ovc_path, &keypair) {
                    Ok(repo) => Ok(Some(repo)),
                    Err(e) => {
                        use ovc_core::error::CoreError;
                        match &e {
                            // Repo has no key slots or this key is not in any
                            // slot — fall through to password-based auth.
                            CoreError::DecryptionFailed { .. } => Ok(None),
                            _ => Err(e).context("failed to open repository with key"),
                        }
                    }
                }
            })()?;

            if let Some(repo) = key_attempt {
                return Ok((repo, workdir));
            }
        }

        let password = Self::get_password()?;
        let repo = Repository::open(&ovc_path, password.as_bytes())
            .context("failed to open repository")?;
        Ok((repo, workdir))
    }

    /// Loads ignore rules for the given workdir.
    pub fn load_ignore(workdir: &WorkDir) -> IgnoreRules {
        IgnoreRules::load(workdir.root())
    }

    /// Resolves the author identity from CLI flag, env vars, or repo config.
    pub fn resolve_author(
        flag: Option<&str>,
        repo: &Repository,
    ) -> Result<ovc_core::object::Identity> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));

        if let Some(author_str) = flag {
            let (name, email) = parse_author(author_str)?;
            return Ok(ovc_core::object::Identity {
                name,
                email,
                timestamp: now,
                tz_offset_minutes: 0,
            });
        }

        let env_name = std::env::var("OVC_AUTHOR_NAME").ok();
        let env_email = std::env::var("OVC_AUTHOR_EMAIL").ok();
        if let (Some(name), Some(email)) = (env_name, env_email) {
            return Ok(ovc_core::object::Identity {
                name,
                email,
                timestamp: now,
                tz_offset_minutes: 0,
            });
        }

        let config = repo.config();
        if !config.user_name.is_empty() && !config.user_email.is_empty() {
            return Ok(ovc_core::object::Identity {
                name: config.user_name.clone(),
                email: config.user_email.clone(),
                timestamp: now,
                tz_offset_minutes: 0,
            });
        }

        bail!(
            "cannot determine author identity; use --author, set OVC_AUTHOR_NAME + OVC_AUTHOR_EMAIL, \
             or configure user.name/user.email in the repository"
        );
    }
}

/// Finds the first `.ovc` file in a directory.
fn find_ovc_in_dir(dir: &Path) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "ovc") {
            return Some(path);
        }
    }
    None
}

/// Reads a `.ovc-link` file in a directory and resolves the path it contains.
///
/// Returns `Some(path)` if the link file exists, is readable, and the target
/// `.ovc` file exists. Returns `None` otherwise.
fn find_ovc_link_in_dir(dir: &Path) -> Option<PathBuf> {
    let link_path = dir.join(".ovc-link");
    if !link_path.is_file() {
        return None;
    }
    let content = std::fs::read_to_string(&link_path).ok()?;
    let target = PathBuf::from(content.trim());
    if target.is_file() {
        target.canonicalize().ok()
    } else {
        None
    }
}

/// Resolve a commit specifier to an `ObjectId`.
///
/// Accepts: full 64-char hex, short hex prefix (>= 4 chars), HEAD, HEAD~N,
/// or a branch name.
pub fn resolve_commit(
    spec: &str,
    repo: &ovc_core::repository::Repository,
) -> Result<ovc_core::id::ObjectId> {
    // Full hex ObjectId.
    if let Ok(oid) = spec.parse::<ovc_core::id::ObjectId>() {
        return Ok(oid);
    }

    // HEAD.
    if spec.eq_ignore_ascii_case("HEAD") {
        return repo
            .ref_store()
            .resolve_head()
            .context("cannot resolve HEAD");
    }

    // HEAD~N.
    if let Some(rest) = spec.strip_prefix("HEAD~") {
        let n: usize = rest.parse().context("invalid HEAD~N syntax")?;
        let mut oid = repo
            .ref_store()
            .resolve_head()
            .context("cannot resolve HEAD")?;
        for _ in 0..n {
            let obj = repo
                .get_object(&oid)?
                .ok_or_else(|| anyhow::anyhow!("commit not found: {oid}"))?;
            match obj {
                ovc_core::object::Object::Commit(c) => {
                    if c.parents.is_empty() {
                        bail!("reached root commit before HEAD~{n}");
                    }
                    oid = c.parents[0];
                }
                _ => bail!("object is not a commit: {oid}"),
            }
        }
        return Ok(oid);
    }

    // Branch name.
    let branch_ref = format!("refs/heads/{spec}");
    if let Ok(oid) = repo.ref_store().resolve(&branch_ref) {
        return Ok(oid);
    }

    // Tag name.
    let tag_ref = format!("refs/tags/{spec}");
    if let Ok(oid) = repo.ref_store().resolve(&tag_ref) {
        return Ok(oid);
    }

    // Short hex prefix (>= 4 chars).
    if spec.len() >= 4 && spec.chars().all(|c| c.is_ascii_hexdigit()) {
        return repo
            .object_store()
            .resolve_prefix(spec)
            .context(format!("cannot resolve short hash: {spec}"));
    }

    bail!("cannot resolve commit: {spec}");
}

/// Parses "Name <email>" into (name, email).
fn parse_author(s: &str) -> Result<(String, String)> {
    let s = s.trim();
    if let Some(lt_pos) = s.find('<') {
        let name = s[..lt_pos].trim().to_owned();
        let email = s[lt_pos + 1..].trim_end_matches('>').trim().to_owned();
        if name.is_empty() || email.is_empty() {
            bail!("invalid author format: expected 'Name <email>', got '{s}'");
        }
        Ok((name, email))
    } else {
        bail!("invalid author format: expected 'Name <email>', got '{s}'");
    }
}
