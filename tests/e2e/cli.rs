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
fn graftd_is_a_stub_that_fails() {
    // The daemon binary lives in another package; only check it when built.
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_graft")).with_file_name("graftd");
    if !exe.exists() {
        return;
    }
    assert_cmd::Command::new(exe)
        .assert()
        .failure()
        .stderr(predicate::str::contains("not implemented"));
}
