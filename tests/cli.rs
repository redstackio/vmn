use std::fs;
use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempfile::TempDir;

struct TestState {
    _temp: TempDir,
    config_dir: std::path::PathBuf,
    data_dir: std::path::PathBuf,
}

impl TestState {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let data_dir = temp.path().join("data");
        Self {
            _temp: temp,
            config_dir,
            data_dir,
        }
    }

    fn cmd(&self) -> Command {
        let mut cmd = Command::cargo_bin("vmn").unwrap();
        cmd.env("VMN_CONFIG_DIR", &self.config_dir)
            .env("VMN_DATA_DIR", &self.data_dir);
        cmd
    }
}

fn fake_env(path: &Path) {
    fs::create_dir_all(path.join("bin")).unwrap();
    fs::write(path.join("pyvenv.cfg"), "home = /usr/bin\n").unwrap();
    fs::write(path.join("bin").join("activate"), "# activate\n").unwrap();
}

#[test]
fn init_prints_zsh_snippet_and_creates_state() {
    let state = TestState::new();

    state
        .cmd()
        .args(["init", "--shell", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("VMN zsh integration"))
        .stdout(predicate::str::contains("vmn list --fzf"));

    assert!(state.config_dir.join("config.toml").is_file());
    assert!(state.data_dir.join("vmn.db").is_file());
}

#[test]
fn init_prints_bash_snippet() {
    let state = TestState::new();

    state
        .cmd()
        .args(["init", "--shell", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("VMN bash integration"))
        .stdout(predicate::str::contains("bind -x"))
        .stdout(predicate::str::contains("vmn list --fzf"));
}

#[test]
fn scan_lists_and_activates_fake_project_env() {
    let state = TestState::new();
    let project = state._temp.path().join("project");
    fake_env(&project.join(".venv"));

    state
        .cmd()
        .arg("scan")
        .arg(&project)
        .assert()
        .success()
        .stdout(predicate::str::contains("registered 1 environment"));

    state
        .cmd()
        .args(["list", "--fzf"])
        .assert()
        .success()
        .stdout(predicate::str::contains("project"))
        .stdout(predicate::str::contains("active"));

    state
        .cmd()
        .args(["activate-path", "project"])
        .assert()
        .success()
        .stdout(predicate::str::contains(".venv/bin/activate"));

    state
        .cmd()
        .args(["project-dir", "project"])
        .assert()
        .success()
        .stdout(predicate::str::contains(project.to_string_lossy().as_ref()));
}

#[test]
fn create_info_refresh_and_remove_managed_env() {
    let state = TestState::new();

    let output = state
        .cmd()
        .args(["create", "demo", "--python", "python3"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let activate_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(activate_path.ends_with("/bin/activate"));
    assert!(Path::new(&activate_path).is_file());

    state
        .cmd()
        .args(["info", "demo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Name: demo"))
        .stdout(predicate::str::contains("Python: Python"));

    state
        .cmd()
        .args(["refresh", "demo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("refreshed 1 environment"));

    state
        .cmd()
        .args(["remove", "demo", "--delete-files", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed"));

    assert!(!Path::new(&activate_path).exists());
}
