//! `Repo` against real repositories built with the `git` CLI. Fixture setup
//! may shell out; Graft itself never does.

// Fixture helpers outside #[test] fns: panicking on setup failure is the point.
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::Command;

use graft_core::CommitId;
use graft_root::{Repo, RootError};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
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
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn commit(dir: &Path, file: &str, msg: &str) -> CommitId {
    std::fs::write(dir.join(file), msg).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"]).parse().unwrap()
}

fn repo_with_commits(n: usize) -> (tempfile::TempDir, Vec<CommitId>) {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    let ids = (0..n)
        .map(|i| commit(dir.path(), "file.txt", &format!("c{i}")))
        .collect();
    (dir, ids)
}

#[test]
fn discovers_from_a_subdirectory_with_a_canonical_workdir() {
    let (dir, _) = repo_with_commits(1);
    let sub = dir.path().join("a/b");
    std::fs::create_dir_all(&sub).unwrap();
    let repo = Repo::discover(&sub).unwrap();
    assert_eq!(repo.workdir(), dir.path().canonicalize().unwrap());
}

#[test]
fn refuses_a_directory_that_is_not_a_repository() {
    let dir = tempfile::tempdir().unwrap();
    let err = Repo::discover(dir.path()).unwrap_err();
    assert!(matches!(err, RootError::NotARepository { .. }), "{err:?}");
    assert!(err.to_string().starts_with("not a git repository"), "{err}");
}

#[test]
fn refuses_a_bare_repository() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "--bare"]);
    let err = Repo::discover(dir.path()).unwrap_err();
    assert!(matches!(err, RootError::Bare { .. }), "{err:?}");
}

#[test]
fn an_empty_repository_has_no_commits() {
    let (dir, _) = repo_with_commits(0);
    let repo = Repo::discover(dir.path()).unwrap();
    assert!(matches!(
        repo.head_commit(),
        Err(RootError::NoCommits { .. })
    ));
    assert!(matches!(
        repo.first_commit(),
        Err(RootError::NoCommits { .. })
    ));
    assert_eq!(repo.head_ref().unwrap().as_deref(), Some("refs/heads/main"));
}

#[test]
fn head_and_first_commit() {
    let (dir, ids) = repo_with_commits(3);
    let repo = Repo::discover(dir.path()).unwrap();
    assert_eq!(repo.head_commit().unwrap(), ids[2]);
    assert_eq!(repo.first_commit().unwrap(), ids[0]);
}

#[test]
fn first_commit_follows_first_parents_through_unrelated_merges() {
    let (dir, ids) = repo_with_commits(2);
    git(dir.path(), &["checkout", "-q", "--orphan", "other"]);
    git(dir.path(), &["rm", "-q", "-rf", "."]);
    commit(dir.path(), "other.txt", "other root");
    git(dir.path(), &["checkout", "-q", "main"]);
    git(
        dir.path(),
        &[
            "merge",
            "-q",
            "--allow-unrelated-histories",
            "-m",
            "merge",
            "other",
        ],
    );
    let repo = Repo::discover(dir.path()).unwrap();
    assert_eq!(repo.first_commit().unwrap(), ids[0]);
}

#[test]
fn resolves_revisions_to_commits() {
    let (dir, ids) = repo_with_commits(3);
    git(
        dir.path(),
        &["tag", "-a", "v1", "-m", "annotated", &ids[1].to_string()],
    );
    let repo = Repo::discover(dir.path()).unwrap();
    assert_eq!(repo.resolve_commit("HEAD").unwrap(), ids[2]);
    assert_eq!(repo.resolve_commit("HEAD~2").unwrap(), ids[0]);
    assert_eq!(repo.resolve_commit("main").unwrap(), ids[2]);
    assert_eq!(repo.resolve_commit("v1").unwrap(), ids[1]);
    assert_eq!(repo.resolve_commit(ids[0].short(10)).unwrap(), ids[0]);
    for bad in ["nope", "HEAD~9"] {
        let err = repo.resolve_commit(bad).unwrap_err();
        assert!(
            matches!(err, RootError::BadRevision { .. }),
            "{bad}: {err:?}"
        );
    }
}

#[test]
fn detached_head_has_no_ref() {
    let (dir, ids) = repo_with_commits(2);
    git(
        dir.path(),
        &["checkout", "-q", "--detach", &ids[0].to_string()],
    );
    let repo = Repo::discover(dir.path()).unwrap();
    assert_eq!(repo.head_ref().unwrap(), None);
    assert_eq!(repo.head_commit().unwrap(), ids[0]);
}

#[test]
fn has_commit() {
    let (dir, ids) = repo_with_commits(1);
    let repo = Repo::discover(dir.path()).unwrap();
    assert!(repo.has_commit(&ids[0]));
    assert!(!repo.has_commit(&"0".repeat(40).parse().unwrap()));
}

#[test]
fn clones_at_different_paths_get_different_root_ids() {
    let (dir, ids) = repo_with_commits(2);
    let other = tempfile::tempdir().unwrap();
    let clone = other.path().join("clone");
    git(
        other.path(),
        &["clone", "-q", dir.path().to_str().unwrap(), "clone"],
    );
    let (a, first_a) = Repo::discover(dir.path())
        .unwrap()
        .derive_root_id()
        .unwrap();
    let (b, first_b) = Repo::discover(&clone).unwrap().derive_root_id().unwrap();
    assert_eq!(first_a, ids[0]);
    assert_eq!(first_a, first_b);
    assert_ne!(a, b);
}
