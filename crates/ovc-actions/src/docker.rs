//! Docker execution support for OVC actions.
//!
//! Provides Docker availability probing, image pull logic, and
//! `docker run` command construction for executing actions inside
//! a container instead of on the host.

use std::path::Path;
use std::time::Duration;

use crate::error::{ActionsError, ActionsResult};

/// Cached result of a Docker availability probe.
#[derive(Debug, Clone)]
pub struct DockerAvailability {
    /// Whether Docker is usable.
    pub available: bool,
    /// Docker server version (e.g. "24.0.5"), if available.
    pub version: Option<String>,
    /// Human-readable reason Docker is unavailable.
    pub reason: Option<String>,
}

/// Probe whether `docker` is on PATH and the daemon is responsive.
///
/// Runs `docker info --format '{{.ServerVersion}}'` with a 5-second timeout.
/// Returns a [`DockerAvailability`] indicating the result.
pub async fn probe_docker() -> DockerAvailability {
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::process::Command::new("docker")
            .args(["info", "--format", "{{.ServerVersion}}"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await
    })
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            DockerAvailability {
                available: true,
                version: if version.is_empty() {
                    None
                } else {
                    Some(version)
                },
                reason: None,
            }
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            DockerAvailability {
                available: false,
                version: None,
                reason: Some(if stderr.is_empty() {
                    "docker info returned non-zero exit code".to_owned()
                } else {
                    stderr
                }),
            }
        }
        Ok(Err(e)) => DockerAvailability {
            available: false,
            version: None,
            reason: Some(format!("failed to run docker: {e}")),
        },
        Err(_) => DockerAvailability {
            available: false,
            version: None,
            reason: Some("docker info timed out after 5 seconds".to_owned()),
        },
    }
}

/// Parameters for building a `docker run` command.
pub struct DockerRunParams<'a> {
    /// Docker image to run.
    pub image: &'a str,
    /// Host path to the repository root (volume mount source).
    pub repo_root: &'a Path,
    /// Absolute working directory on the host (must be inside `repo_root`).
    pub work_dir: &'a Path,
    /// Shell command to execute inside the container.
    pub command: &'a str,
    /// Shell to use (e.g. "/bin/sh").
    pub shell: &'a str,
    /// Non-secret environment variables to inject via `-e KEY=VALUE`.
    pub env: Vec<(String, String)>,
    /// Secret environment variables injected via parent process env +
    /// `-e KEY` (no value) so Docker inherits them without exposing
    /// values in `/proc/pid/cmdline`.
    pub secret_env: Vec<(String, String)>,
    /// Container name for identifiability and debugging.
    pub container_name: &'a str,
    /// Additional `docker run` flags from config.
    pub extra_flags: &'a [String],
}

/// Build a `tokio::process::Command` for running a shell command inside Docker.
///
/// Produces the equivalent of:
/// ```text
/// docker run --rm \
///   -v <repo_root>:/workspace \
///   -w /workspace/<relative_work_dir> \
///   -e KEY=VALUE ... \
///   <extra_flags> \
///   <image> \
///   /bin/sh -c '<command>'
/// ```
#[must_use]
pub fn build_docker_command(params: &DockerRunParams<'_>) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("docker");
    cmd.arg("run").arg("--rm");

    // Container name for identifiability and debugging.
    cmd.arg("--name").arg(params.container_name);

    // Volume mount: repo root -> /workspace
    let mount = format!("{}:/workspace", params.repo_root.display());
    cmd.arg("-v").arg(&mount);

    // Working directory inside container
    let container_work_dir = params.work_dir.strip_prefix(params.repo_root).map_or_else(
        |_| "/workspace".to_owned(),
        |relative| {
            if relative.as_os_str().is_empty() {
                "/workspace".to_owned()
            } else {
                format!("/workspace/{}", relative.display())
            }
        },
    );
    cmd.arg("-w").arg(&container_work_dir);

    // Non-secret environment variables: passed as `-e KEY=VALUE`.
    for (key, value) in &params.env {
        cmd.arg("-e").arg(format!("{key}={value}"));
    }

    // Secret environment variables: set on the parent process and passed
    // as `-e KEY` (no value) so Docker inherits them without exposing
    // values in `/proc/pid/cmdline`.
    for (key, value) in &params.secret_env {
        cmd.env(key, value);
        cmd.arg("-e").arg(key);
    }

    // Extra flags from config
    for flag in params.extra_flags {
        cmd.arg(flag);
    }

    // Image
    cmd.arg(params.image);

    // Shell + command
    cmd.arg(params.shell).arg("-c").arg(params.command);

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    cmd
}

/// Ensure the Docker image is available locally, pulling if necessary.
///
/// Behavior depends on `pull_policy`:
/// - `"always"`: always pull.
/// - `"if-not-present"`: pull only if the image is not found locally.
/// - `"never"`: never pull; error if image is missing.
pub async fn ensure_image(image: &str, pull_policy: &str) -> ActionsResult<()> {
    match pull_policy {
        "never" => {
            // Check if image exists locally
            let inspect = tokio::process::Command::new("docker")
                .args(["image", "inspect", image])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await
                .map_err(|e| ActionsError::DockerUnavailable {
                    reason: format!("failed to run docker image inspect: {e}"),
                })?;
            if !inspect.success() {
                return Err(ActionsError::DockerPullFailed {
                    image: image.to_owned(),
                    reason: "image not found locally and pull_policy is 'never'".to_owned(),
                });
            }
            Ok(())
        }
        "if-not-present" => {
            // Check if image exists locally first
            let inspect = tokio::process::Command::new("docker")
                .args(["image", "inspect", image])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await
                .map_err(|e| ActionsError::DockerUnavailable {
                    reason: format!("failed to run docker image inspect: {e}"),
                })?;
            if inspect.success() {
                return Ok(());
            }
            pull_image(image).await
        }
        // "always" or any other value
        _ => pull_image(image).await,
    }
}

/// Pull a Docker image.
async fn pull_image(image: &str) -> ActionsResult<()> {
    let output = tokio::process::Command::new("docker")
        .args(["pull", image])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| ActionsError::DockerUnavailable {
            reason: format!("failed to run docker pull: {e}"),
        })?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(ActionsError::DockerPullFailed {
            image: image.to_owned(),
            reason: stderr,
        })
    }
}
