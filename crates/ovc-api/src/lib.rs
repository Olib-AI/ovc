//! `ovc-api` — REST API server for OVC (Olib Version Control).
//!
//! Provides an Axum-based HTTP server that exposes OVC repository operations
//! as a REST API. Designed for consumption by a React SPA or as a local daemon
//! for desktop use.
//!
//! # Usage
//!
//! ```no_run
//! use ovc_api::ServerConfig;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//! let config = ServerConfig {
//!     bind: "127.0.0.1".to_owned(),
//!     port: 9742,
//!     repos_dir: std::path::PathBuf::from("./"),
//!     jwt_secret: None,
//!     cors_origins: Vec::new(),
//!     workdir_map: Vec::new(),
//!     workdir_scan: Vec::new(),
//! };
//! ovc_api::start_server(config).await?;
//! # Ok(())
//! # }
//! ```

pub mod auth;
pub mod error;
pub mod models;
pub mod routes;
pub mod state;
pub mod static_files;

use std::path::PathBuf;
use std::sync::Arc;

use tokio::net::TcpListener;

use crate::routes::build_router;
use crate::state::AppState;

/// Configuration for starting the API server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Address to bind to (e.g., `"127.0.0.1"`).
    pub bind: String,
    /// Port to listen on.
    pub port: u16,
    /// Directory containing `.ovc` repository files.
    pub repos_dir: PathBuf,
    /// JWT signing secret. Auto-generated if `None`.
    pub jwt_secret: Option<String>,
    /// Custom CORS allowed origins. When empty, falls back to localhost defaults.
    pub cors_origins: Vec<String>,
    /// Explicit workdir mappings: `repo_id → /path/to/workdir`.
    pub workdir_map: Vec<(String, PathBuf)>,
    /// Directories to scan for `.ovc-link` files (auto-discover workdirs).
    pub workdir_scan: Vec<PathBuf>,
}

/// Starts the OVC API server with the given configuration.
///
/// This function blocks until the server is shut down.
pub async fn start_server(
    config: ServerConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let jwt_secret = match config.jwt_secret {
        Some(s) => s,
        None => load_or_create_persisted_secret(&config.repos_dir)?,
    };
    let state = Arc::new(AppState::new(config.repos_dir, jwt_secret));

    // Register explicit workdir mappings from config.
    for (repo_id, workdir) in &config.workdir_map {
        state.set_workdir(repo_id, workdir.clone());
    }

    // Scan directories for .ovc-link files to auto-discover workdirs.
    for scan_dir in &config.workdir_scan {
        if let Ok(entries) = std::fs::read_dir(scan_dir) {
            for entry in entries.flatten() {
                let link_path = entry.path().join(".ovc-link");
                if link_path.is_file()
                    && let Ok(content) = std::fs::read_to_string(&link_path)
                {
                    let target = std::path::PathBuf::from(content.trim());
                    if let Some(stem) = target.file_stem().and_then(|s| s.to_str()) {
                        state.set_workdir(stem, entry.path());
                    }
                }
            }
        }
    }
    let router = build_router(Arc::clone(&state), &config.cors_origins);

    // Spawn the scheduled actions runner in the background.
    let sched_state = Arc::clone(&state);
    tokio::spawn(async move {
        run_scheduled_actions_loop(sched_state).await;
    });

    // Spawn the challenge cache cleanup task. Runs every 60 seconds to evict
    // expired entries regardless of whether POST /auth/key-auth is ever called,
    // preventing unbounded memory growth from unanswered GET /auth/challenge
    // requests.
    let challenge_state = Arc::clone(&state);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        // Skip the immediate first tick so the map isn't locked at startup.
        interval.tick().await;
        loop {
            interval.tick().await;
            challenge_state.cleanup_expired_challenges();
        }
    });

    let addr = format!("{}:{}", config.bind, config.port);
    let listener = TcpListener::bind(&addr).await?;

    tracing::info!("OVC API server listening on http://{addr}");

    axum::serve(listener, router).await?;

    Ok(())
}

/// Background loop that checks for and runs scheduled actions every 60 seconds.
///
/// Scans all `.ovc` repos in the repos directory, loads their actions config,
/// and runs any actions with `trigger: schedule` whose cron-like `schedule`
/// expression matches the current minute.
///
/// Schedule format: a simple `"HH:MM"` or `"*:MM"` or `"HH:*"` expression.
/// For full cron syntax, a `cron` crate dependency would be needed.
async fn run_scheduled_actions_loop(state: Arc<AppState>) {
    use ovc_actions::config::{ActionsConfig, Trigger};
    use ovc_actions::runner::ActionRunner;

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    // The first tick fires immediately — skip it so we don't run at startup.
    interval.tick().await;

    loop {
        interval.tick().await;

        let now = chrono::Local::now();
        let current_hour = now.format("%H").to_string();
        let current_minute = now.format("%M").to_string();

        // Scan repos directory for .ovc files.
        let Ok(entries) = std::fs::read_dir(&state.repos_dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Some(ext) = path.extension() else {
                continue;
            };
            if ext != "ovc" {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let repo_id = stem.to_owned();

            // Derive working directory for this repo.
            let work_dir = state
                .workdir_for(&repo_id)
                .unwrap_or_else(|| state.repos_dir.join(format!("{repo_id}.ovc.d")));

            // Load actions config (non-blocking).
            let wd = work_dir.clone();
            let Ok(Ok(Some(config))) =
                tokio::task::spawn_blocking(move || ActionsConfig::load(&wd)).await
            else {
                continue;
            };

            // Check for scheduled actions.
            let scheduled = config.actions_for_trigger(Trigger::Schedule);
            if scheduled.is_empty() {
                continue;
            }

            // Check each scheduled action's schedule expression.
            let mut should_run = false;
            for (_, action_def) in &scheduled {
                if let Some(ref sched) = action_def.schedule
                    && schedule_matches(sched, &current_hour, &current_minute)
                {
                    should_run = true;
                    break;
                }
            }

            if !should_run {
                continue;
            }

            tracing::info!("running scheduled actions for repo '{repo_id}'");
            let runner = ActionRunner::new_with_docker_probe(&work_dir, config).await;
            let results = runner.run_trigger(Trigger::Schedule, &[]).await;
            for r in &results {
                if r.status == ovc_actions::runner::ActionStatus::Failed {
                    tracing::warn!(
                        "scheduled action '{}' failed in repo '{repo_id}': {}",
                        r.display_name,
                        if r.stderr.is_empty() {
                            &r.stdout
                        } else {
                            &r.stderr
                        }
                    );
                }
            }
        }
    }
}

/// Checks if a simple schedule expression matches the current time.
///
/// Supported formats:
/// - `"HH:MM"` — exact hour and minute (e.g., `"02:30"`)
/// - `"*:MM"` — every hour at minute MM (e.g., `"*:00"` for top of every hour)
/// - `"HH:*"` — every minute during hour HH (rarely useful)
/// - `"*"` or `"*:*"` — every minute
fn schedule_matches(schedule: &str, current_hour: &str, current_minute: &str) -> bool {
    let schedule = schedule.trim();
    if schedule == "*" || schedule == "*:*" {
        return true;
    }
    let Some((hour_part, minute_part)) = schedule.split_once(':') else {
        return false;
    };
    let hour_ok = hour_part == "*" || hour_part == current_hour;
    let minute_ok = minute_part == "*" || minute_part == current_minute;
    hour_ok && minute_ok
}

/// Loads a persisted JWT secret from `<repos_dir>/.ovc-server-secret`, or
/// generates a new one and writes it to disk with restrictive permissions.
///
/// This ensures JWT sessions survive server restarts when no explicit
/// `--jwt-secret` is provided.
fn load_or_create_persisted_secret(
    repos_dir: &std::path::Path,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let secret_path = repos_dir.join(".ovc-server-secret");

    if secret_path.is_file() {
        let secret = std::fs::read_to_string(&secret_path)?.trim().to_owned();
        if !secret.is_empty() {
            tracing::info!("loaded persisted JWT secret from {}", secret_path.display());
            return Ok(secret);
        }
    }

    // Generate a fresh secret.
    let secret = generate_secret();

    // Ensure the repos directory exists.
    std::fs::create_dir_all(repos_dir)?;

    // Write the secret file.
    std::fs::write(&secret_path, &secret)?;

    // Set file permissions to 0600 (owner read/write only) on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&secret_path, std::fs::Permissions::from_mode(0o600))?;
    }

    tracing::info!(
        "generated and persisted new JWT secret to {}",
        secret_path.display()
    );
    Ok(secret)
}

/// Generates a random 32-byte hex secret for JWT signing.
fn generate_secret() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes);
    hex_encode(&bytes)
}

/// Encodes bytes as a lowercase hex string.
fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{byte:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use crate::routes::build_router;
    use crate::state::AppState;

    /// Creates a test router backed by a temporary directory.
    fn test_router(dir: &std::path::Path) -> axum::Router {
        let app = Arc::new(AppState::new(
            dir.to_path_buf(),
            "test-secret-key-for-jwt".to_owned(),
        ));
        build_router(app, &[])
    }

    /// Creates a valid JWT for test requests.
    fn test_token() -> String {
        let (token, _) = crate::auth::create_jwt("test-secret-key-for-jwt", 1)
            .expect("failed to create test token");
        token
    }

    /// Helper to parse a JSON response body.
    async fn json_body(response: axum::http::Response<Body>) -> serde_json::Value {
        let bytes = http_body_util::BodyExt::collect(response.into_body())
            .await
            .expect("failed to collect body")
            .to_bytes();
        serde_json::from_slice(&bytes).expect("failed to parse JSON")
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let dir = tempfile::tempdir().unwrap();
        let router = test_router(dir.path());

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = json_body(response).await;
        assert_eq!(body["status"], "ok");
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn request_without_token_returns_401() {
        let dir = tempfile::tempdir().unwrap();
        let router = test_router(dir.path());

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn create_and_list_repos() {
        let dir = tempfile::tempdir().unwrap();
        let router = test_router(dir.path());
        let token = test_token();

        // Create a repo.
        let create_body = serde_json::json!({
            "name": "test-repo",
            "password": "secret123"
        });

        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = json_body(response).await;
        assert_eq!(body["id"], "test-repo");

        // List repos.
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        let repos = body.as_array().expect("expected array");
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0]["id"], "test-repo");
    }

    #[tokio::test]
    async fn unlock_repo_and_get_status() {
        let dir = tempfile::tempdir().unwrap();
        let router = test_router(dir.path());
        let token = test_token();

        // Create repo.
        let create_body = serde_json::json!({
            "name": "status-repo",
            "password": "pw123"
        });
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        // Get status (repo already unlocked from create).
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/status-repo/status")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert!(body["branch"].is_string());
    }

    #[tokio::test]
    async fn create_branch_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let router = test_router(dir.path());
        let token = test_token();

        // Create repo with initial commit (so branches have something to point to).
        let create_body = serde_json::json!({
            "name": "branch-repo",
            "password": "pw"
        });
        let _ = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // We need a commit before we can create branches (HEAD must resolve).
        // Stage a file and commit using the core API directly.
        {
            let ovc_path = dir.path().join("branch-repo.ovc");
            let mut repo = ovc_core::repository::Repository::open(&ovc_path, b"pw").unwrap();
            let (index, store) = repo.index_and_store_mut();
            index
                .stage_file(
                    "readme.md",
                    b"# Hello",
                    ovc_core::object::FileMode::Regular,
                    store,
                )
                .unwrap();
            let author = ovc_core::object::Identity {
                name: "Test".into(),
                email: "test@test.com".into(),
                timestamp: 1_700_000_000,
                tz_offset_minutes: 0,
            };
            repo.create_commit("initial", &author).unwrap();
            repo.save().unwrap();
        }

        // Create branch via API.
        let branch_body = serde_json::json!({ "name": "feature" });
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/branch-repo/branches")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&branch_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["name"], "feature");

        // List branches.
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/branch-repo/branches")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        let branches = body.as_array().expect("expected array");
        // Should have "main" and "feature".
        assert!(branches.len() >= 2);
    }

    #[tokio::test]
    async fn auth_token_endpoint_correct_password() {
        let dir = tempfile::tempdir().unwrap();
        let router = test_router(dir.path());

        // The server was initialized with "test-secret-key-for-jwt" as the secret.
        let body = serde_json::json!({ "password": "test-secret-key-for-jwt" });
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert!(body["token"].is_string());
        assert!(body["expires_at"].is_string());
    }

    #[tokio::test]
    async fn auth_token_endpoint_wrong_password_returns_401() {
        let dir = tempfile::tempdir().unwrap();
        let router = test_router(dir.path());

        let body = serde_json::json!({ "password": "wrong-password" });
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    /// Helper to create a repo and its working directory for actions tests.
    fn setup_actions_repo(dir: &std::path::Path, name: &str) {
        // Create the .ovc file so validate_repo_exists passes.
        let ovc_path = dir.join(format!("{name}.ovc"));
        ovc_core::repository::Repository::init(&ovc_path, b"pw").expect("failed to init repo");

        // Create the working directory (simulates the project root).
        let work_dir = dir.join(format!("{name}.ovc.d"));
        std::fs::create_dir_all(work_dir.join(".ovc")).expect("failed to create .ovc dir");
    }

    #[tokio::test]
    async fn actions_detect_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let router = test_router(dir.path());
        let token = test_token();

        setup_actions_repo(dir.path(), "detect-repo");

        // Place a Cargo.toml marker to trigger Rust detection.
        let work_dir = dir.path().join("detect-repo.ovc.d");
        std::fs::write(work_dir.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/detect-repo/actions/detect")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        let languages = body["languages"]
            .as_array()
            .expect("expected languages array");
        assert!(!languages.is_empty(), "should detect at least one language");
        assert_eq!(languages[0]["language"], "Rust");
        assert_eq!(languages[0]["confidence"], "high");
    }

    #[tokio::test]
    async fn actions_init_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let router = test_router(dir.path());
        let token = test_token();

        setup_actions_repo(dir.path(), "init-repo");

        // Place marker files so detection finds something.
        let work_dir = dir.path().join("init-repo.ovc.d");
        std::fs::write(work_dir.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();

        // Init actions config via API.
        let init_body = serde_json::json!({ "force": false });
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/init-repo/actions/init")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&init_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = json_body(response).await;
        // The generated config should have actions.
        assert!(
            body["actions"].is_object(),
            "expected actions object in config"
        );

        // Verify the file was written.
        assert!(
            work_dir.join(".ovc").join("actions.yml").is_file(),
            "actions.yml should have been created"
        );

        // List actions via API.
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/init-repo/actions/list")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        let actions = body["actions"].as_array().expect("expected actions array");
        assert!(
            !actions.is_empty(),
            "should have actions after init with Rust marker"
        );

        // Verify Rust-specific actions are present.
        let action_names: Vec<&str> = actions.iter().filter_map(|a| a["name"].as_str()).collect();
        assert!(
            action_names.contains(&"rust-check"),
            "should contain rust-check action, got: {action_names:?}"
        );
    }
}
