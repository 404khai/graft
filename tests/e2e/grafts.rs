//! `graft new`, `ls`, `path`, `status`, `diff` and `prune`.

use std::path::{Path, PathBuf};
use std::process::Command;

use graft_core::GraftHome;
use predicates::prelude::*;

use crate::{git, git_repo, graft};

struct Setup {
    home: tempfile::TempDir,
    repo: tempfile::TempDir,
}

impl Setup {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let repo = git_repo();
        std::fs::create_dir(repo.path().join("src")).unwrap();
        std::fs::write(repo.path().join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        git(repo.path(), &["add", "-A"]);
        git(repo.path(), &["commit", "-q", "-m", "src"]);
        let s = Self { home, repo };
        s.run(&["init"]).success();
        s
    }

    fn cmd(&self, args: &[&str]) -> assert_cmd::Command {
        let mut c = graft(self.home.path());
        c.args(args).current_dir(self.repo.path());
        c
    }

    fn run(&self, args: &[&str]) -> assert_cmd::assert::Assert {
        self.cmd(args).assert()
    }

    fn stdout(&self, args: &[&str]) -> String {
        String::from_utf8(self.run(args).success().get_output().stdout.clone()).unwrap()
    }

    fn path(&self, name: &str) -> PathBuf {
        PathBuf::from(self.stdout(&["path", name]).trim_end())
    }

    fn store(&self) -> graft_store::Store {
        let home = GraftHome::from_path(self.home.path()).unwrap();
        let mut roots = graft_store::registry::list_roots(&home).unwrap();
        assert_eq!(roots.len(), 1);
        roots.pop().unwrap().1
    }
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn new_creates_a_clean_working_tree() {
    let s = Setup::new();
    let out = s.stdout(&["new", "auth"]);
    assert!(out.starts_with("Created graft auth at "), "{out}");
    let path = s.path("auth");
    assert!(
        path.starts_with(s.home.path().canonicalize().unwrap()) || path.starts_with(s.home.path())
    );
    assert_eq!(
        std::fs::read_to_string(path.join("src/lib.rs")).unwrap(),
        "pub fn a() {}\n"
    );
    assert_eq!(git_out(&path, &["status", "--porcelain"]), "");
    let ls = s.stdout(&["ls"]);
    assert!(ls.lines().next().unwrap().starts_with("NAME  BASE"), "{ls}");
    assert!(ls.lines().nth(1).unwrap().contains(" live "), "{ls}");
}

#[test]
fn new_from_a_ref() {
    let s = Setup::new();
    let first = git_out(s.repo.path(), &["rev-parse", "HEAD~1"]);
    s.run(&["new", "old", "--from", "HEAD~1"]).success();
    let path = s.path("old");
    assert_eq!(git_out(&path, &["rev-parse", "HEAD"]), first);
    assert!(!path.join("src").exists());
    s.run(&["new", "bad", "--from", "no-such-ref"])
        .failure()
        .stderr(predicate::str::contains("cannot resolve \"no-such-ref\""));
}

#[test]
fn new_validates_and_reserves_names() {
    let s = Setup::new();
    s.run(&["new", "Auth"])
        .failure()
        .stderr(predicate::str::contains("invalid graft name \"Auth\""));
    s.run(&["new", "auth"]).success();
    s.run(&["new", "auth"])
        .failure()
        .stderr(predicate::str::contains(
            "a graft named auth already exists",
        ));
    assert_eq!(s.store().grafts().unwrap().len(), 1);
}

#[test]
fn new_requires_a_registered_root() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    graft(home.path())
        .args(["new", "x"])
        .current_dir(repo.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("run `graft init` first"));
}

/// AGENTS.md Phase 2: two grafts can be created, edited and pruned
/// independently.
#[test]
fn two_grafts_are_independent() {
    let s = Setup::new();
    s.run(&["new", "one"]).success();
    s.run(&["new", "two"]).success();
    let (one, two) = (s.path("one"), s.path("two"));
    std::fs::write(one.join("README.md"), "from one\n").unwrap();
    std::fs::write(two.join("two.txt"), "two\n").unwrap();

    assert_eq!(
        std::fs::read_to_string(two.join("README.md")).unwrap(),
        "hello\n"
    );
    assert_eq!(
        std::fs::read_to_string(s.repo.path().join("README.md")).unwrap(),
        "hello\n"
    );
    let status = s.stdout(&["status"]);
    assert!(
        status.contains("graft one (live)") && status.contains("  M README.md"),
        "{status}"
    );
    assert!(
        status.contains("graft two (live)") && status.contains("  A two.txt"),
        "{status}"
    );

    s.run(&["prune", "one", "--force"]).success();
    assert!(!one.exists());
    assert_eq!(
        std::fs::read_to_string(two.join("two.txt")).unwrap(),
        "two\n"
    );
    s.run(&["prune", "two", "--force"]).success();
    assert!(!two.exists());
}

/// AGENTS.md Phase 2: a graft never shares a git branch lock with Root or
/// another graft.
#[test]
fn grafts_do_not_take_branch_locks() {
    let s = Setup::new();
    s.run(&["new", "one"]).success();
    s.run(&["new", "two"]).success();
    let (one, two) = (s.path("one"), s.path("two"));
    // Root has `main` checked out; both grafts may check out `main` too.
    for g in [&one, &two] {
        git(g, &["checkout", "-q", "-b", "main"]);
        std::fs::write(g.join("f.txt"), "x").unwrap();
        git(g, &["add", "f.txt"]);
        git(g, &["commit", "-q", "-m", "on main"]);
    }
    assert_eq!(
        git_out(s.repo.path(), &["symbolic-ref", "HEAD"]).trim(),
        "refs/heads/main"
    );
    // Root's branches and worktree list are untouched.
    assert_eq!(
        git_out(s.repo.path(), &["branch", "--format=%(refname)"]).trim(),
        "refs/heads/main"
    );
    assert_eq!(
        git_out(s.repo.path(), &["worktree", "list"])
            .lines()
            .count(),
        1
    );
    // Root can still switch branches freely.
    git(s.repo.path(), &["checkout", "-q", "-b", "other"]);
}

/// AGENTS.md Phase 2: prune leaves no files, mounts or DB rows behind.
#[test]
fn prune_leaves_nothing_behind() {
    let s = Setup::new();
    s.run(&["new", "gone"]).success();
    let path = s.path("gone");
    let id = s.store().grafts().unwrap()[0].id;
    assert_eq!(
        git_out(
            s.repo.path(),
            &["for-each-ref", "--format=%(refname)", "refs/graft"]
        )
        .trim(),
        format!("refs/graft/{id}")
    );
    std::fs::write(path.join("scratch"), "x").unwrap();
    s.run(&["prune", "gone", "--force"])
        .success()
        .stdout("Pruned graft gone\n");

    assert!(!path.exists(), "working tree left behind");
    let mounts = path.parent().unwrap();
    assert_eq!(
        std::fs::read_dir(mounts).unwrap().count(),
        0,
        "mounts dir not empty"
    );
    assert!(
        s.store().grafts().unwrap().is_empty(),
        "DB rows left behind"
    );
    assert_eq!(
        git_out(s.repo.path(), &["for-each-ref", "refs/graft"]),
        "",
        "pin ref left behind"
    );
    s.run(&["ls"])
        .success()
        .stdout(predicate::str::contains("No grafts"));
    s.run(&["prune", "gone"])
        .failure()
        .stderr(predicate::str::contains("no graft named gone"));
}

#[test]
fn prune_refuses_uncommitted_and_unintegrated_work() {
    let s = Setup::new();
    s.run(&["new", "work"]).success();
    let path = s.path("work");

    std::fs::write(path.join("README.md"), "edited\n").unwrap();
    s.run(&["prune", "work"])
        .failure()
        .stderr(predicate::str::contains("uncommitted changes: README.md"))
        .stderr(predicate::str::contains("--force"));

    git(&path, &["commit", "-q", "-am", "edit"]);
    s.run(&["prune", "work"])
        .failure()
        .stderr(predicate::str::contains("is not in Root"));
    assert!(path.exists(), "a refused prune must not touch anything");

    s.run(&["prune", "work", "--force"]).success();
    assert!(!path.exists());
}

#[test]
fn prune_allows_work_that_root_already_has() {
    let s = Setup::new();
    s.run(&["new", "landed"]).success();
    let path = s.path("landed");
    std::fs::write(path.join("README.md"), "landed\n").unwrap();
    git(&path, &["commit", "-q", "-am", "land"]);
    git(
        s.repo.path(),
        &["fetch", "-q", path.to_str().unwrap(), "graft/landed"],
    );
    s.run(&["prune", "landed"]).success();
}

#[test]
fn diff_and_status_report_changes_against_the_base() {
    let s = Setup::new();
    s.run(&["new", "d"]).success();
    let path = s.path("d");
    std::fs::write(path.join("README.md"), "hello\nworld\n").unwrap();
    std::fs::remove_file(path.join("src/lib.rs")).unwrap();
    std::fs::write(path.join("new.txt"), "new\n").unwrap();

    let status = s.stdout(&["status", "d"]);
    assert!(
        status.ends_with("  M README.md\n  A new.txt\n  D src/lib.rs\n"),
        "{status}"
    );
    let diff = s.stdout(&["diff", "d"]);
    assert!(diff.contains("+world\n"), "{diff}");
    assert!(
        diff.contains("deleted file\n--- a/src/lib.rs\n+++ /dev/null\n"),
        "{diff}"
    );
    assert!(
        diff.contains("new file\n--- /dev/null\n+++ b/new.txt\n"),
        "{diff}"
    );
    let ls = s.stdout(&["ls"]);
    assert!(ls.lines().nth(1).unwrap().contains(" 3 "), "{ls}");
}

#[test]
fn commands_work_from_inside_a_graft() {
    let s = Setup::new();
    s.run(&["new", "inner"]).success();
    let path = s.path("inner");
    std::fs::write(path.join("README.md"), "x\n").unwrap();
    let status = String::from_utf8(
        graft(s.home.path())
            .arg("status")
            .current_dir(path.join("src"))
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(status.starts_with("graft inner (live)"), "{status}");
    assert!(status.contains("  M README.md"), "{status}");
    // A sibling created from inside a graft branches from Root's HEAD.
    graft(s.home.path())
        .args(["new", "sibling"])
        .current_dir(&path)
        .assert()
        .success();
    assert_eq!(
        std::fs::read_to_string(s.path("sibling").join("README.md")).unwrap(),
        "hello\n"
    );
}

/// AGENTS.md Phase 2: the copy fallback works and prints a single clear
/// warning. On a reflink filesystem there must be no warning at all.
#[test]
fn warns_exactly_once_when_copying_instead_of_reflinking() {
    let s = Setup::new();
    let reflinks = graft_fs::probe_reflink(s.repo.path()).unwrap();
    let out = s.run(&["new", "w"]).success();
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    let warnings: Vec<_> = stderr
        .lines()
        .filter(|l| l.starts_with("graft: warning:"))
        .collect();
    if reflinks {
        assert!(warnings.is_empty(), "{stderr}");
    } else {
        assert_eq!(warnings.len(), 1, "{stderr}");
        assert!(
            warnings[0].contains("does not support reflinks"),
            "{stderr}"
        );
        assert!(warnings[0].contains("full size on disk"), "{stderr}");
        assert_eq!(
            std::fs::read_to_string(s.path("w").join("src/lib.rs")).unwrap(),
            "pub fn a() {}\n"
        );
    }
}

/// `$GRAFT_HOME` on another filesystem forces copies: one warning naming
/// the reason. Runs where /dev/shm is a separate filesystem (Linux CI).
#[test]
fn warns_once_when_graft_home_is_on_another_filesystem() {
    let shm = Path::new("/dev/shm");
    let repo = git_repo();
    if !shm.is_dir() || graft_fs::same_filesystem(shm, repo.path()).unwrap_or(true) {
        return;
    }
    let home = tempfile::tempdir_in(shm).unwrap();
    let run = |args: &[&str]| {
        graft(home.path())
            .args(args)
            .current_dir(repo.path())
            .assert()
    };
    run(&["init"]).success();
    let out = run(&["new", "far"]).success();
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    let warnings: Vec<_> = stderr
        .lines()
        .filter(|l| l.starts_with("graft: warning:"))
        .collect();
    assert_eq!(warnings.len(), 1, "{stderr}");
    assert!(warnings[0].contains("different filesystems"), "{stderr}");
}

#[test]
fn concurrent_new_with_one_name_creates_one_graft() {
    let s = Setup::new();
    let children: Vec<_> = (0..8)
        .map(|_| {
            Command::new(env!("CARGO_BIN_EXE_graft"))
                .args(["new", "race"])
                .current_dir(s.repo.path())
                .env("GRAFT_HOME", s.home.path())
                .env_remove("GRAFT_LOG")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let ok = children
        .into_iter()
        .map(|c| c.wait_with_output().unwrap())
        .filter(|o| o.status.success())
        .count();
    assert_eq!(ok, 1);
    assert_eq!(s.store().grafts().unwrap().len(), 1);
    let path = s.path("race");
    assert_eq!(git_out(&path, &["status", "--porcelain"]), "");
    let mounts: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().collect();
    assert_eq!(
        mounts.len(),
        1,
        "losers must leave no temporary directories: {mounts:?}"
    );
}
