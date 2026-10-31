//! End-to-end tests: drive the `graft` binary as a user would.
//!
//! Every test points `$GRAFT_HOME` at a temporary directory; none may touch
//! the real one.

// Test helpers outside #[test] fns: panicking on setup failure is the point.
#![allow(clippy::unwrap_used)]

mod cli;
mod doctor;
mod grafts;
mod repo_policy;
mod root;

use std::path::Path;

/// A `graft` command isolated from the caller's environment.
pub(crate) fn graft(home: &Path) -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::new(env!("CARGO_BIN_EXE_graft"));
    cmd.env("GRAFT_HOME", home).env_remove("GRAFT_LOG");
    cmd
}

/// A temporary directory that looks like the top of a git repository.
pub(crate) fn fake_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    dir
}

/// Runs `git` for fixture setup, isolated from the user's git config.
pub(crate) fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Graft Test")
        .env("GIT_AUTHOR_EMAIL", "test@graft.invalid")
        .env("GIT_COMMITTER_NAME", "Graft Test")
        .env("GIT_COMMITTER_EMAIL", "test@graft.invalid")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A real git repository with one commit.
pub(crate) fn git_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(dir.path().join("README.md"), "hello\n").unwrap();
    git(dir.path(), &["add", "README.md"]);
    git(dir.path(), &["commit", "-q", "-m", "first"]);
    dir
}
