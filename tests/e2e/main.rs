//! End-to-end tests: drive the `graft` binary as a user would.
//!
//! Every test points `$GRAFT_HOME` at a temporary directory; none may touch
//! the real one.

// Test helpers outside #[test] fns: panicking on setup failure is the point.
#![allow(clippy::unwrap_used)]

mod cli;
mod doctor;
mod repo_policy;

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
