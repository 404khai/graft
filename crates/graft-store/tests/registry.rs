//! Registering and finding Roots for real repositories.

// Fixture helpers outside #[test] fns: panicking on setup failure is the point.
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::Command;

use graft_core::GraftHome;
use graft_root::Repo;
use graft_store::registry::{find_root, list_roots, register};
use graft_store::{Registration, StoreError};

fn git(dir: &Path, args: &[&str]) {
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
}

fn init_repo(dir: &Path, content: &str) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("README.md"), content).unwrap();
    git(dir, &["add", "README.md"]);
    git(dir, &["commit", "-q", "-m", content]);
}

fn setup() -> (tempfile::TempDir, GraftHome, tempfile::TempDir) {
    let home_dir = tempfile::tempdir().unwrap();
    let home = GraftHome::from_path(home_dir.path()).unwrap();
    let repo_dir = tempfile::tempdir().unwrap();
    init_repo(repo_dir.path(), "first");
    (home_dir, home, repo_dir)
}

#[test]
fn register_is_idempotent() {
    let (_h, home, repo_dir) = setup();
    let repo = Repo::discover(repo_dir.path()).unwrap();
    let (first, _, reg) = register(&home, &repo).unwrap();
    assert_eq!(reg, Registration::Created);
    let (second, _, reg) = register(&home, &repo).unwrap();
    assert_eq!(reg, Registration::Existing);
    assert_eq!(first, second);
    assert_eq!(first.path, repo.workdir());
    assert_eq!(first.first_commit, repo.first_commit().unwrap());
    assert!(home.root_dir(&first.id).join("graft.db").is_file());
    assert_eq!(list_roots(&home).unwrap().len(), 1);
}

#[test]
fn register_from_a_subdirectory_finds_the_same_root() {
    let (_h, home, repo_dir) = setup();
    let sub = repo_dir.path().join("src");
    std::fs::create_dir(&sub).unwrap();
    let (a, _, _) = register(&home, &Repo::discover(repo_dir.path()).unwrap()).unwrap();
    let (b, _, reg) = register(&home, &Repo::discover(&sub).unwrap()).unwrap();
    assert_eq!(reg, Registration::Existing);
    assert_eq!(a.id, b.id);
}

#[test]
fn moving_head_to_unrelated_history_keeps_the_root() {
    let (_h, home, repo_dir) = setup();
    let (a, _, _) = register(&home, &Repo::discover(repo_dir.path()).unwrap()).unwrap();
    git(repo_dir.path(), &["checkout", "-q", "--orphan", "pages"]);
    std::fs::write(repo_dir.path().join("index.html"), "pages").unwrap();
    git(repo_dir.path(), &["add", "index.html"]);
    git(repo_dir.path(), &["commit", "-q", "-m", "pages"]);
    let repo = Repo::discover(repo_dir.path()).unwrap();
    assert_ne!(repo.first_commit().unwrap(), a.first_commit);
    let (b, _, reg) = register(&home, &repo).unwrap();
    assert_eq!(reg, Registration::Existing);
    assert_eq!(a.id, b.id);
}

#[test]
fn a_different_repository_at_the_same_path_is_a_new_root() {
    let (_h, home, repo_dir) = setup();
    let (a, _, _) = register(&home, &Repo::discover(repo_dir.path()).unwrap()).unwrap();
    std::fs::remove_dir_all(repo_dir.path().join(".git")).unwrap();
    init_repo(repo_dir.path(), "second");
    let repo = Repo::discover(repo_dir.path()).unwrap();
    assert!(find_root(&home, &repo).unwrap().is_none());
    let (b, _, reg) = register(&home, &repo).unwrap();
    assert_eq!(reg, Registration::Created);
    assert_ne!(a.id, b.id);
    assert_eq!(find_root(&home, &repo).unwrap().unwrap().0.id, b.id);
    assert_eq!(list_roots(&home).unwrap().len(), 2);
}

#[test]
fn empty_repositories_cannot_be_registered() {
    let home_dir = tempfile::tempdir().unwrap();
    let home = GraftHome::from_path(home_dir.path()).unwrap();
    let repo_dir = tempfile::tempdir().unwrap();
    git(repo_dir.path(), &["init", "-q"]);
    let err = register(&home, &Repo::discover(repo_dir.path()).unwrap()).unwrap_err();
    assert!(
        matches!(
            err,
            StoreError::Root(graft_root::RootError::NoCommits { .. })
        ),
        "{err:?}"
    );
    assert!(
        !home.roots_dir().exists(),
        "nothing written for a refused init"
    );
}

#[test]
fn list_roots_skips_unrelated_and_damaged_entries() {
    let (_h, home, repo_dir) = setup();
    let (a, _, _) = register(&home, &Repo::discover(repo_dir.path()).unwrap()).unwrap();
    let roots = home.roots_dir();
    std::fs::create_dir(roots.join("not-a-root-id")).unwrap();
    std::fs::write(roots.join("stray-file"), "x").unwrap();
    let damaged = roots.join("ffffffffffffffffffffffffffffffff");
    std::fs::create_dir(&damaged).unwrap();
    std::fs::write(damaged.join("graft.db"), "this is not sqlite").unwrap();
    let listed = list_roots(&home).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].0.id, a.id);
}

#[test]
fn no_roots_before_the_first_registration() {
    let home_dir = tempfile::tempdir().unwrap();
    let home = GraftHome::from_path(home_dir.path().join("missing")).unwrap();
    assert!(list_roots(&home).unwrap().is_empty());
}
