//! Integration tests for the `ovc` CLI binary.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

/// Helper to build an `ovc` command with the password set via env var.
fn ovc_cmd() -> Command {
    let mut cmd = Command::cargo_bin("ovc").expect("binary 'ovc' not found");
    cmd.env("OVC_PASSWORD", "test-password");
    cmd.env("OVC_AUTHOR_NAME", "Test Author");
    cmd.env("OVC_AUTHOR_EMAIL", "test@example.com");
    cmd
}

#[test]
fn init_creates_ovc_file() {
    let dir = TempDir::new().unwrap();

    ovc_cmd()
        .args(["init", dir.path().to_str().unwrap(), "--name", "test.ovc"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized empty OVC repository"));

    assert!(dir.path().join("test.ovc").exists());
}

#[test]
fn full_workflow_init_add_commit_log() {
    let dir = TempDir::new().unwrap();
    let repo_path = dir.path().join("repo.ovc");

    // Init.
    ovc_cmd()
        .args(["init", dir.path().to_str().unwrap()])
        .assert()
        .success();

    assert!(repo_path.exists());

    // Create a file in the workdir.
    std::fs::write(dir.path().join("hello.txt"), b"hello world").unwrap();

    let repo_flag = format!("--repo={}", repo_path.display());

    // Add.
    ovc_cmd()
        .args([&repo_flag, "add", "hello.txt"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("staged 1 file(s)"));

    // Commit.
    ovc_cmd()
        .args([&repo_flag, "commit", "-m", "initial commit"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("initial commit"));

    // Log.
    ovc_cmd()
        .args([&repo_flag, "log", "--oneline"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("initial commit"));
}

#[test]
fn branch_create_and_list() {
    let dir = TempDir::new().unwrap();
    let repo_path = dir.path().join("repo.ovc");

    // Init.
    ovc_cmd()
        .args(["init", dir.path().to_str().unwrap()])
        .assert()
        .success();

    // Create a file and commit so we have a HEAD.
    std::fs::write(dir.path().join("file.txt"), b"data").unwrap();
    let repo_flag = format!("--repo={}", repo_path.display());

    ovc_cmd()
        .args([&repo_flag, "add", "file.txt"])
        .current_dir(dir.path())
        .assert()
        .success();

    ovc_cmd()
        .args([&repo_flag, "commit", "-m", "first"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Create branch.
    ovc_cmd()
        .args([&repo_flag, "branch", "feature"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("created branch 'feature'"));

    // List branches.
    ovc_cmd()
        .args([&repo_flag, "branch"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("feature"))
        .stdout(predicate::str::contains("main"));
}
