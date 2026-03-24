//! Tests for ovc-actions.

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::config::{
        ActionCategory, ActionCondition, ActionDefinition, ActionsConfig, BuiltinAction, Trigger,
    };
    use crate::detect::detect_languages;
    use crate::history::{ActionHistory, ActionRunRecord};
    use crate::runner::{ActionResult, ActionRunner, ActionStatus};

    // ---- Config parsing ----

    #[test]
    fn parse_valid_config() {
        let yaml = r#"
defaults:
  shell: /bin/bash
  timeout: 60
actions:
  lint:
    category: lint
    command: "echo lint"
    trigger: pre-commit
"#;
        let config = ActionsConfig::from_yaml(yaml).unwrap();
        assert_eq!(config.defaults.shell, "/bin/bash");
        assert_eq!(config.defaults.timeout, 60);
        assert!(config.actions.contains_key("lint"));
        assert_eq!(config.actions["lint"].category, ActionCategory::Lint);
    }

    #[test]
    fn parse_invalid_yaml_returns_error() {
        let result = ActionsConfig::from_yaml("{{invalid yaml}}");
        assert!(result.is_err());
    }

    #[test]
    fn validate_catches_missing_command_and_builtin() {
        let mut config = ActionsConfig::default();
        config.actions.insert(
            "bad-action".to_owned(),
            ActionDefinition {
                category: ActionCategory::Custom,
                display_name: None,
                language: None,
                tool: None,
                command: None,
                fix_command: None,
                trigger: Trigger::Manual,
                timeout: None,
                working_dir: None,
                env: BTreeMap::new(),
                continue_on_error: false,
                condition: None,
                schedule: None,
                builtin: None,
                config: serde_yaml::Value::Null,
                ..Default::default()
            },
        );
        let issues = config.validate();
        assert!(issues.iter().any(|i| i.contains("must specify either")));
    }

    #[test]
    fn validate_catches_zero_timeout() {
        let mut config = ActionsConfig::default();
        config.actions.insert(
            "zero-timeout".to_owned(),
            ActionDefinition {
                category: ActionCategory::Custom,
                display_name: None,
                language: None,
                tool: None,
                command: Some("echo hi".to_owned()),
                fix_command: None,
                trigger: Trigger::Manual,
                timeout: Some(0),
                working_dir: None,
                env: BTreeMap::new(),
                continue_on_error: false,
                condition: None,
                schedule: None,
                builtin: None,
                config: serde_yaml::Value::Null,
                ..Default::default()
            },
        );
        let issues = config.validate();
        assert!(issues.iter().any(|i| i.contains("timeout must be > 0")));
    }

    #[test]
    fn actions_for_trigger_filters() {
        let yaml = r#"
actions:
  a:
    command: "echo a"
    trigger: pre-commit
  b:
    command: "echo b"
    trigger: pre-push
  c:
    command: "echo c"
    trigger: pre-commit
"#;
        let config = ActionsConfig::from_yaml(yaml).unwrap();
        let pre_commit = config.actions_for_trigger(Trigger::PreCommit);
        assert_eq!(pre_commit.len(), 2);
        let pre_push = config.actions_for_trigger(Trigger::PrePush);
        assert_eq!(pre_push.len(), 1);
    }

    // ---- Language detection ----

    #[test]
    fn detect_rust_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let result = detect_languages(dir.path());
        assert!(result.languages.iter().any(|l| l.language == "Rust"));
    }

    #[test]
    fn detect_js_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let result = detect_languages(dir.path());
        assert!(result.languages.iter().any(|l| l.language == "JavaScript"));
    }

    #[test]
    fn detect_multi_language() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        std::fs::write(dir.path().join("go.mod"), "module test").unwrap();
        let result = detect_languages(dir.path());
        assert!(result.languages.len() >= 2);
    }

    // ---- Built-in: secret scan ----

    #[test]
    fn secret_scan_finds_aws_key() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.txt"),
            "aws_key = AKIAIOSFODNN7EXAMPLE\n", // ovc:ignore
        )
        .unwrap();
        let result = crate::builtin::run_builtin(
            BuiltinAction::SecretScan,
            &serde_yaml::Value::Null,
            dir.path(),
            &["config.txt".to_owned()],
            "secret_scan",
            "Secret Scanner",
            false,
        )
        .unwrap();
        assert_eq!(result.status, ActionStatus::Failed);
        assert!(result.stdout.contains("[AWS Access Key]"));
        assert_eq!(result.name, "secret_scan");
        assert_eq!(result.display_name, "Secret Scanner");
    }

    // ---- Built-in: trailing whitespace ----

    #[test]
    fn trailing_whitespace_detected() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello   \nworld\n").unwrap();
        let result = crate::builtin::run_builtin(
            BuiltinAction::TrailingWhitespace,
            &serde_yaml::Value::Null,
            dir.path(),
            &["test.txt".to_owned()],
            "trailing_whitespace",
            "Trailing Whitespace",
            false,
        )
        .unwrap();
        assert_eq!(result.status, ActionStatus::Failed);
        assert!(result.stdout.contains("test.txt:1"));
    }

    // ---- Built-in: file size ----

    #[test]
    fn file_size_check() {
        let dir = tempfile::tempdir().unwrap();
        // Write a file over 10 bytes, set max_bytes to 10
        std::fs::write(dir.path().join("big.txt"), "0123456789ABCDEF").unwrap();
        let config: serde_yaml::Value = serde_yaml::from_str("max_bytes: 10").unwrap();
        let result = crate::builtin::run_builtin(
            BuiltinAction::FileSize,
            &config,
            dir.path(),
            &["big.txt".to_owned()],
            "file_size",
            "File Size Check",
            false,
        )
        .unwrap();
        assert_eq!(result.status, ActionStatus::Failed);
        assert!(result.stdout.contains("big.txt"));
    }

    // ---- Built-in: todo counter ----

    #[test]
    fn todo_counter_finds_markers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("code.rs"),
            "// TODO: fix this\n// FIXME: also this\nfn main() {}\n",
        )
        .unwrap();
        let result = crate::builtin::run_builtin(
            BuiltinAction::TodoCounter,
            &serde_yaml::Value::Null,
            dir.path(),
            &["code.rs".to_owned()],
            "todo_counter",
            "TODO Counter",
            false,
        )
        .unwrap();
        // Always passes (informational)
        assert_eq!(result.status, ActionStatus::Passed);
        assert!(result.stdout.contains("2 TODO/FIXME/HACK/XXX"));
    }

    // ---- Runner: successful command ----

    #[tokio::test]
    async fn runner_successful_command() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = ActionsConfig::default();
        config.actions.insert(
            "echo-test".to_owned(),
            ActionDefinition {
                category: ActionCategory::Custom,
                display_name: Some("Echo Test".to_owned()),
                language: None,
                tool: None,
                command: Some("echo hello".to_owned()),
                fix_command: None,
                trigger: Trigger::Manual,
                timeout: Some(5),
                working_dir: None,
                env: BTreeMap::new(),
                continue_on_error: false,
                condition: None,
                schedule: None,
                builtin: None,
                config: serde_yaml::Value::Null,
                ..Default::default()
            },
        );
        let runner = ActionRunner::new(dir.path(), config);
        let result = runner.run_action("echo-test").await.unwrap();
        assert_eq!(result.status, ActionStatus::Passed);
        assert!(result.stdout.contains("hello"));
    }

    // ---- Runner: failed command ----

    #[tokio::test]
    async fn runner_failed_command() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = ActionsConfig::default();
        config.actions.insert(
            "fail-test".to_owned(),
            ActionDefinition {
                category: ActionCategory::Custom,
                display_name: None,
                language: None,
                tool: None,
                command: Some("exit 1".to_owned()),
                fix_command: None,
                trigger: Trigger::Manual,
                timeout: Some(5),
                working_dir: None,
                env: BTreeMap::new(),
                continue_on_error: false,
                condition: None,
                schedule: None,
                builtin: None,
                config: serde_yaml::Value::Null,
                ..Default::default()
            },
        );
        let runner = ActionRunner::new(dir.path(), config);
        let result = runner.run_action("fail-test").await.unwrap();
        assert_eq!(result.status, ActionStatus::Failed);
        assert_eq!(result.exit_code, Some(1));
    }

    // ---- Runner: timeout ----

    #[tokio::test]
    async fn runner_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = ActionsConfig::default();
        config.actions.insert(
            "slow".to_owned(),
            ActionDefinition {
                category: ActionCategory::Custom,
                display_name: None,
                language: None,
                tool: None,
                command: Some(if cfg!(target_os = "windows") {
                    "ping -n 61 127.0.0.1 >nul".to_owned()
                } else {
                    "sleep 60".to_owned()
                }),
                fix_command: None,
                trigger: Trigger::Manual,
                timeout: Some(1),
                working_dir: None,
                env: BTreeMap::new(),
                continue_on_error: false,
                condition: None,
                schedule: None,
                builtin: None,
                config: serde_yaml::Value::Null,
                ..Default::default()
            },
        );
        let runner = ActionRunner::new(dir.path(), config);
        let result = runner.run_action("slow").await.unwrap();
        assert_eq!(result.status, ActionStatus::TimedOut);
    }

    // ---- History: record and list round-trip ----

    #[test]
    fn history_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let history = ActionHistory::new(dir.path());

        let record = ActionRunRecord {
            run_id: "test-run-123".to_owned(),
            trigger: "manual".to_owned(),
            timestamp: "2024-01-01T00:00:00Z".to_owned(),
            results: vec![ActionResult {
                name: "echo".to_owned(),
                display_name: "Echo".to_owned(),
                category: "custom".to_owned(),
                status: ActionStatus::Passed,
                exit_code: Some(0),
                stdout: "hello\n".to_owned(),
                stderr: String::new(),
                duration_ms: 42,
                started_at: "2024-01-01T00:00:00Z".to_owned(),
                finished_at: "2024-01-01T00:00:00Z".to_owned(),
                continue_on_error: false,
                ..Default::default()
            }],
            overall_status: "passed".to_owned(),
            total_duration_ms: 42,
        };

        history.record_run(&record).unwrap();

        let retrieved = history.get_run("test-run-123").unwrap().unwrap();
        assert_eq!(retrieved.run_id, "test-run-123");
        assert_eq!(retrieved.results.len(), 1);

        let runs = history.list_runs(10).unwrap();
        assert_eq!(runs.len(), 1);
    }

    // ---- Condition matching ----

    #[test]
    fn condition_glob_matching() {
        let condition = ActionCondition {
            paths: vec!["**/*.rs".to_owned()],
        };
        assert!(ActionRunner::matches_condition(
            &condition,
            &["src/main.rs".to_owned()]
        ));
        assert!(!ActionRunner::matches_condition(
            &condition,
            &["readme.md".to_owned()]
        ));
    }

    #[test]
    fn condition_empty_paths_matches_all() {
        let condition = ActionCondition { paths: vec![] };
        assert!(ActionRunner::matches_condition(
            &condition,
            &["anything.txt".to_owned()]
        ));
    }

    // ---- Issue 1: Shell validation ----

    #[test]
    fn shell_validation_rejects_python() {
        let mut config = ActionsConfig::default();
        config.defaults.shell = "/usr/bin/python".to_owned();
        let issues = config.validate();
        assert!(
            issues.iter().any(|i| i.contains("not allowed")),
            "expected validation to reject /usr/bin/python, got: {issues:?}"
        );
    }

    // ---- Issue 2: working_dir path traversal ----

    #[test]
    fn working_dir_dotdot_rejected_by_validate() {
        let mut config = ActionsConfig::default();
        config.actions.insert(
            "traversal".to_owned(),
            ActionDefinition {
                category: ActionCategory::Custom,
                display_name: None,
                language: None,
                tool: None,
                command: Some("echo hi".to_owned()),
                fix_command: None,
                trigger: Trigger::Manual,
                timeout: Some(5),
                working_dir: Some("../etc".to_owned()),
                env: BTreeMap::new(),
                continue_on_error: false,
                condition: None,
                schedule: None,
                builtin: None,
                config: serde_yaml::Value::Null,
                ..Default::default()
            },
        );
        let issues = config.validate();
        assert!(
            issues.iter().any(|i| i.contains("path traversal")),
            "expected validation to catch '..', got: {issues:?}"
        );
    }

    // ---- Issue 3: Output truncation ----

    #[test]
    fn history_truncates_large_output() {
        let dir = tempfile::tempdir().unwrap();
        let history = ActionHistory::new(dir.path());

        // Create output larger than 64 KiB.
        let large_output = "A".repeat(80_000);

        let record = ActionRunRecord {
            run_id: "truncation-test".to_owned(),
            trigger: "manual".to_owned(),
            timestamp: "2024-01-01T00:00:00Z".to_owned(),
            results: vec![ActionResult {
                name: "big-output".to_owned(),
                display_name: "Big Output".to_owned(),
                category: "custom".to_owned(),
                status: ActionStatus::Passed,
                exit_code: Some(0),
                stdout: large_output,
                stderr: String::new(),
                duration_ms: 10,
                started_at: "2024-01-01T00:00:00Z".to_owned(),
                finished_at: "2024-01-01T00:00:00Z".to_owned(),
                continue_on_error: false,
                ..Default::default()
            }],
            overall_status: "passed".to_owned(),
            total_duration_ms: 10,
        };

        history.record_run(&record).unwrap();
        let retrieved = history.get_run("truncation-test").unwrap().unwrap();
        let stdout = &retrieved.results[0].stdout;
        // 64 KiB = 65536 bytes, plus the truncation marker.
        assert!(
            stdout.len() < 80_000,
            "stdout should be truncated, but was {} bytes",
            stdout.len()
        );
        assert!(
            stdout.ends_with("[truncated]"),
            "truncated output should end with [truncated] marker"
        );
    }

    // ---- Docker config parsing ----

    #[test]
    fn docker_config_parses_from_yaml() {
        let yaml = r#"
defaults:
  docker:
    enabled: true
    image: ghcr.io/olib-ai/ovc-actions:latest
    pull_policy: always
    extra_flags:
      - "--network=host"
actions:
  test:
    command: "echo hi"
    trigger: manual
"#;
        let config = ActionsConfig::from_yaml(yaml).unwrap();
        assert!(config.defaults.docker.enabled);
        assert_eq!(
            config.defaults.docker.image,
            "ghcr.io/olib-ai/ovc-actions:latest"
        );
        assert_eq!(config.defaults.docker.pull_policy, "always");
        assert_eq!(config.defaults.docker.extra_flags, vec!["--network=host"]);
    }

    #[test]
    fn docker_config_defaults_when_absent() {
        let yaml = r#"
actions:
  test:
    command: "echo hi"
    trigger: manual
"#;
        let config = ActionsConfig::from_yaml(yaml).unwrap();
        assert!(!config.defaults.docker.enabled);
        assert_eq!(
            config.defaults.docker.image,
            "ghcr.io/olib-ai/ovc-actions:latest"
        );
        assert_eq!(config.defaults.docker.pull_policy, "if-not-present");
        assert!(config.defaults.docker.extra_flags.is_empty());
    }

    #[test]
    fn docker_override_parses() {
        let yaml = r#"
actions:
  native-only:
    command: "echo native"
    trigger: manual
    docker_override: false
  docker-forced:
    command: "echo docker"
    trigger: manual
    docker_override: true
  inherit:
    command: "echo inherit"
    trigger: manual
"#;
        let config = ActionsConfig::from_yaml(yaml).unwrap();
        assert_eq!(config.actions["native-only"].docker_override, Some(false));
        assert_eq!(config.actions["docker-forced"].docker_override, Some(true));
        assert_eq!(config.actions["inherit"].docker_override, None);
    }

    #[test]
    fn validate_rejects_invalid_pull_policy() {
        let yaml = r#"
defaults:
  docker:
    enabled: true
    pull_policy: "invalid-policy"
actions:
  test:
    command: "echo hi"
    trigger: manual
"#;
        let config = ActionsConfig::from_yaml(yaml).unwrap();
        let issues = config.validate();
        assert!(
            issues.iter().any(|i| i.contains("pull_policy")),
            "expected validation to reject invalid pull_policy, got: {issues:?}"
        );
    }

    #[test]
    fn validate_warns_empty_docker_image() {
        let yaml = r#"
defaults:
  docker:
    enabled: true
    image: ""
actions:
  test:
    command: "echo hi"
    trigger: manual
"#;
        let config = ActionsConfig::from_yaml(yaml).unwrap();
        let issues = config.validate();
        assert!(
            issues.iter().any(|i| i.contains("image is empty")),
            "expected validation to warn about empty image, got: {issues:?}"
        );
    }

    // ---- Issue: has_blocking_failures with mixed continue_on_error ----

    #[test]
    fn has_blocking_failures_mixed_continue_on_error() {
        use crate::hooks::has_blocking_failures;

        let now = "2024-01-01T00:00:00Z".to_owned();

        let results = vec![
            ActionResult {
                name: "pass".to_owned(),
                display_name: "Pass".to_owned(),
                category: "custom".to_owned(),
                status: ActionStatus::Passed,
                exit_code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
                duration_ms: 1,
                started_at: now.clone(),
                finished_at: now.clone(),
                continue_on_error: false,
                ..Default::default()
            },
            ActionResult {
                name: "fail-continue".to_owned(),
                display_name: "Fail Continue".to_owned(),
                category: "custom".to_owned(),
                status: ActionStatus::Failed,
                exit_code: Some(1),
                stdout: String::new(),
                stderr: String::new(),
                duration_ms: 1,
                started_at: now.clone(),
                finished_at: now.clone(),
                continue_on_error: true,
                ..Default::default()
            },
        ];
        // Only continue_on_error failures present => no blocking failures.
        assert!(
            !has_blocking_failures(&results),
            "continue_on_error failures should not be blocking"
        );

        let blocking = vec![
            ActionResult {
                name: "fail-block".to_owned(),
                display_name: "Fail Block".to_owned(),
                category: "custom".to_owned(),
                status: ActionStatus::Failed,
                exit_code: Some(1),
                stdout: String::new(),
                stderr: String::new(),
                duration_ms: 1,
                started_at: now.clone(),
                finished_at: now.clone(),
                continue_on_error: false,
                ..Default::default()
            },
            ActionResult {
                name: "fail-continue2".to_owned(),
                display_name: "Fail Continue".to_owned(),
                category: "custom".to_owned(),
                status: ActionStatus::Failed,
                exit_code: Some(1),
                stdout: String::new(),
                stderr: String::new(),
                duration_ms: 1,
                started_at: now.clone(),
                finished_at: now,
                continue_on_error: true,
                ..Default::default()
            },
        ];
        // A non-continue_on_error failure => blocking.
        assert!(
            has_blocking_failures(&blocking),
            "non-continue_on_error failures should be blocking"
        );
    }
}
