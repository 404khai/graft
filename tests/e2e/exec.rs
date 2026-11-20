//! One-shot commands in a graft's working directory.

use std::os::unix::fs::symlink;
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};

use predicates::prelude::*;

use crate::{git_repo, graft};

fn new(home: &Path, root: &Path) -> PathBuf {
    graft(home).current_dir(root).arg("init").assert().success();
    graft(home)
        .current_dir(root)
        .args(["new", "auth", "--backend", "reflink"])
        .assert()
        .success();
    let out = graft(home)
        .current_dir(root)
        .args(["path", "auth"])
        .assert()
        .success();
    PathBuf::from(
        std::str::from_utf8(&out.get_output().stdout)
            .unwrap()
            .trim_end(),
    )
}

#[test]
fn exec_starts_at_graft_top_and_preserves_arguments_and_streams() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("Application Support");
    let repo = git_repo();
    let dir = new(&home, repo.path()).canonicalize().unwrap();
    let nested = dir.join("nested");
    std::fs::create_dir(&nested).unwrap();
    graft(&home)
        .current_dir(&nested)
        .args([
            "exec",
            "auth",
            "--",
            "sh",
            "-c",
            r#"IFS= read -r line; printf '%s\n' "$line" "$1" "$2"; printf '%s\n' "$PWD" >&2; pwd -P; touch written; exit 23"#,
            "sh",
            "space and ; shell characters",
            "--root",
        ])
        .write_stdin("input\n")
        .assert()
        .code(23)
        .stdout(format!("input\nspace and ; shell characters\n--root\n{}\n", dir.display()))
        .stderr(format!("{}\n", dir.display()));
    assert!(dir.join("written").is_file());
    assert!(!nested.join("written").exists());
    assert!(!repo.path().join("written").exists());
}

#[test]
fn exec_can_select_a_registered_root_from_any_directory() {
    let outside = tempfile::tempdir().unwrap();
    let home = outside.path().join("home");
    let first = git_repo();
    let second = git_repo();
    let first_dir = new(&home, first.path()).canonicalize().unwrap();
    let second_dir = new(&home, second.path()).canonicalize().unwrap();

    for cwd in [outside.path(), second.path()] {
        graft(&home)
            .current_dir(cwd)
            .args(["exec", "--root"])
            .arg(first.path())
            .args(["auth", "--", "pwd", "-P"])
            .assert()
            .success()
            .stdout(format!("{}\n", first_dir.display()));
    }
    graft(&home)
        .current_dir(second.path())
        .args(["exec", "auth", "--", "pwd", "-P"])
        .assert()
        .success()
        .stdout(format!("{}\n", second_dir.display()));
}

#[test]
fn exec_preserves_signal_termination() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    new(home.path(), repo.path());
    let out = graft(home.path())
        .current_dir(repo.path())
        .args(["exec", "auth", "--", "sh", "-c", "kill -s TERM $$"])
        .output()
        .unwrap();
    assert_eq!(out.status.signal(), Some(15));
}

#[test]
fn exec_refuses_missing_or_redirected_grafts() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    let dir = new(home.path(), repo.path());
    let cmd = || {
        let mut cmd = graft(home.path());
        cmd.current_dir(repo.path());
        cmd
    };
    cmd()
        .args(["exec", "absent", "--", "printf", "ran"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("no graft named absent"));

    std::fs::remove_dir_all(&dir).unwrap();
    cmd()
        .args(["exec", "auth", "--", "printf", "ran"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("resolving graft auth's directory"));

    symlink(repo.path(), &dir).unwrap();
    cmd()
        .args(["exec", "auth", "--", "printf", "ran"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("graft auth is not a directory at"));
}
