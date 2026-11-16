use predicates::prelude::*;

use crate::graft;

#[test]
fn version_prints_the_package_version() {
    let home = tempfile::tempdir().unwrap();
    graft(home.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("graft {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_lists_doctor() {
    let home = tempfile::tempdir().unwrap();
    graft(home.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("doctor"));
}

#[test]
fn no_arguments_prints_help_and_fails() {
    let home = tempfile::tempdir().unwrap();
    graft(home.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage: graft"));
}

#[test]
fn unknown_subcommand_fails() {
    let home = tempfile::tempdir().unwrap();
    graft(home.path()).arg("worktree").assert().failure();
}

#[test]
fn graftd_reports_its_version() {
    // The daemon binary lives in another package; only check it when built.
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_graft")).with_file_name("graftd");
    if !exe.exists() {
        return;
    }
    assert_cmd::Command::new(exe)
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("graftd "));
}

/// A caller's `$GRAFT_REQUEST_ID` is on every log line of the command
/// (ADR 0031); an unsafe one is ignored with a warning.
#[test]
fn log_lines_carry_the_request_id() {
    let home = tempfile::tempdir().unwrap();
    let repo = crate::git_repo();
    let out = graft(home.path())
        .arg("init")
        .current_dir(repo.path())
        .env("GRAFT_LOG", "debug")
        .env("GRAFT_REQUEST_ID", "script-42")
        .assert()
        .success()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(out).unwrap();
    assert!(stderr.contains("request=script-42"), "{stderr}");

    let out = graft(home.path())
        .arg("root")
        .current_dir(repo.path())
        .env("GRAFT_REQUEST_ID", "bad id")
        .assert()
        .success()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(out).unwrap();
    assert!(stderr.contains("ignoring GRAFT_REQUEST_ID"), "{stderr}");
}
