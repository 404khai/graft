//! Grafts registered as git worktrees of Root (`[git] worktrees = true`,
//! ADR 0036), driven through the `graft` binary. Reflink grafts, so these
//! run everywhere; registered virtual grafts are in `virtual_grafts`.

use std::path::{Path, PathBuf};
use std::process::Command;

use predicates::prelude::*;

use crate::{git, git_repo, graft};

struct Setup {
    home: tempfile::TempDir,
    repo: tempfile::TempDir,
}

impl Setup {
    /// A Root with `[git] worktrees = true` and an optional graft.toml.
    fn new(toml: Option<&str>) -> Self {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join("config.toml"), "[git]\nworktrees = true\n").unwrap();
        let repo = git_repo();
        git(repo.path(), &["config", "user.name", "Graft Test"]);
        git(repo.path(), &["config", "user.email", "test@graft.invalid"]);
        if let Some(toml) = toml {
            std::fs::write(repo.path().join("graft.toml"), toml).unwrap();
        }
        std::fs::create_dir(repo.path().join("src")).unwrap();
        std::fs::write(repo.path().join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        git(repo.path(), &["add", "-A"]);
        git(repo.path(), &["commit", "-q", "-m", "src"]);
        let s = Self { home, repo };
        s.ok(&["init"]);
        s
    }

    fn cmd(&self, args: &[&str]) -> assert_cmd::Command {
        let mut c = graft(self.home.path());
        c.args(args)
            .current_dir(self.repo.path())
            .env_remove("GRAFT_FS")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        c
    }

    fn ok(&self, args: &[&str]) -> String {
        let o = self.cmd(args).output().unwrap();
        assert!(
            o.status.success(),
            "graft {args:?}: {}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8(o.stdout).unwrap()
    }

    fn new_graft(&self, name: &str) -> PathBuf {
        self.ok(&["new", name, "--backend", "reflink"]);
        PathBuf::from(self.ok(&["path", name]).trim())
    }

    fn root_git(&self, args: &[&str]) -> String {
        out(self.repo.path(), args)
    }
}

fn out(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout).unwrap()
}

#[test]
fn registered_grafts_are_listed_as_git_worktrees() {
    let s = Setup::new(None);
    let a = s.new_graft("a");
    let b = s.new_graft("b");
    let list = s.root_git(&["worktree", "list", "--porcelain"]);
    for (path, branch) in [(&a, "graft-live/a/live"), (&b, "graft-live/b/live")] {
        let path = path.canonicalize().unwrap();
        assert!(
            list.contains(&format!("worktree {}\nHEAD ", path.display())),
            "{list}"
        );
        assert!(
            list.contains(&format!("branch refs/heads/{branch}\n")),
            "{list}"
        );
    }
    assert!(!list.contains("prunable"), "{list}");
    // Inside, a graft is an ordinary worktree with a clean status.
    assert!(a.join(".git").is_file());
    assert_eq!(out(&a, &["status", "--porcelain"]), "");
    assert_eq!(
        out(&a, &["branch", "--show-current"]).trim(),
        "graft-live/a/live"
    );
    // graft sees it too, and knows it is registered.
    let ls: serde_json::Value = serde_json::from_str(&s.ok(&["ls", "--json"])).unwrap();
    let grafts = ls["grafts"].as_array().unwrap();
    assert_eq!(grafts.len(), 2);
    assert!(grafts.iter().all(|g| g["git_worktree"] == true), "{ls}");
    // Graft commands work from inside a registered graft.
    let mut status = graft(s.home.path());
    status
        .current_dir(&a)
        .args(["status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("a"));
}

#[test]
fn status_does_not_treat_the_registered_grafts_own_branch_as_published() {
    let s = Setup::new(None);
    let a = s.new_graft("a");
    let base = out(&a, &["rev-parse", "HEAD"]).trim().to_owned();
    std::fs::write(a.join("private.txt"), "private\n").unwrap();
    git(&a, &["add", "-A"]);
    git(&a, &["commit", "-qm", "private"]);
    let v: serde_json::Value = serde_json::from_str(&s.ok(&["status", "a", "--json"])).unwrap();
    assert_eq!(v["grafts"][0]["base"], base);
    assert_eq!(v["grafts"][0]["recorded_base"], base);
    assert_eq!(v["grafts"][0]["committed"], 1);
    assert_eq!(v["grafts"][0]["uncommitted"], 0);
    let ls: serde_json::Value =
        serde_json::from_str(&s.ok(&["ls", "--json", "--no-disk"])).unwrap();
    assert_eq!(ls["grafts"][0]["changed"], 1);
    git(&a, &["switch", "--detach"]);
    let detached: serde_json::Value =
        serde_json::from_str(&s.ok(&["status", "a", "--json"])).unwrap();
    assert_eq!(detached["grafts"][0]["base"], base);
    assert_eq!(detached["grafts"][0]["committed"], 1);
    let ls: serde_json::Value =
        serde_json::from_str(&s.ok(&["ls", "--json", "--no-disk"])).unwrap();
    assert_eq!(ls["grafts"][0]["changed"], 1);
    s.cmd(&["prune", "a"]).assert().failure();
}

#[test]
fn topic_branches_are_private_until_checked_out_elsewhere() {
    let s = Setup::new(None);
    let a = s.new_graft("a");
    let base = out(&a, &["rev-parse", "HEAD"]).trim().to_owned();
    git(&a, &["switch", "-c", "topic"]);
    std::fs::write(a.join("private.txt"), "private\n").unwrap();
    git(&a, &["add", "-A"]);
    git(&a, &["commit", "-qm", "private topic"]);
    for detached in [false, true] {
        if detached {
            git(&a, &["switch", "--detach"]);
        }
        let v: serde_json::Value = serde_json::from_str(&s.ok(&["status", "a", "--json"])).unwrap();
        assert_eq!(v["grafts"][0]["base"], base);
        assert_eq!(v["grafts"][0]["committed"], 1);
        let ls: serde_json::Value =
            serde_json::from_str(&s.ok(&["ls", "--json", "--no-disk"])).unwrap();
        assert_eq!(ls["grafts"][0]["changed"], 1);
    }
    s.root_git(&["switch", "topic"]);
    let v: serde_json::Value = serde_json::from_str(&s.ok(&["status", "a", "--json"])).unwrap();
    assert_eq!(v["grafts"][0]["committed"], 0);
    assert_eq!(v["grafts"][0]["recorded_base"], base);
    s.root_git(&["switch", "main"]);
    let other = s.repo.path().join("other");
    s.root_git(&["worktree", "add", other.to_str().unwrap(), "topic"]);
    let v: serde_json::Value = serde_json::from_str(&s.ok(&["status", "a", "--json"])).unwrap();
    assert_eq!(v["grafts"][0]["committed"], 0);
    s.root_git(&["worktree", "remove", other.to_str().unwrap()]);
}

#[test]
fn registered_graft_commits_land_on_its_branch_in_root() {
    let s = Setup::new(None);
    let a = s.new_graft("a");
    std::fs::write(a.join("src/lib.rs"), "pub fn b() {}\n").unwrap();
    git(&a, &["commit", "-q", "-am", "feat: b"]);
    assert_eq!(
        s.root_git(&["log", "-1", "--format=%s", "graft-live/a/live"])
            .trim(),
        "feat: b"
    );
    // Root's own checkout, branch and index are untouched.
    assert_eq!(
        s.root_git(&["symbolic-ref", "HEAD"]).trim(),
        "refs/heads/main"
    );
    assert_eq!(s.root_git(&["status", "--porcelain"]), "");
    s.root_git(&["fsck", "--strict", "--no-progress"]);
    // `graft diff` and `status` see the committed change.
    let diff = s.ok(&["diff", "a"]);
    assert!(diff.contains("+pub fn b() {}"), "{diff}");
}

#[test]
fn prune_of_a_registered_graft_leaves_nothing_behind() {
    let s = Setup::new(None);
    let a = s.new_graft("a");
    std::fs::write(a.join("scratch"), "x").unwrap();
    s.ok(&["prune", "a", "--force"]);
    assert!(!a.exists());
    assert_eq!(
        s.root_git(&["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        1
    );
    assert_eq!(
        s.root_git(&["for-each-ref", "--format=%(refname)"]).trim(),
        "refs/heads/main"
    );
    let git_dir = s.repo.path().join(".git");
    assert!(
        !git_dir.join("worktrees").exists(),
        "registration left behind"
    );
    assert!(!git_dir.join("refs/heads/graft-live").exists());
    assert!(!git_dir.join("logs/refs/heads/graft-live").exists());
    s.root_git(&["fsck", "--strict", "--no-progress"]);
}

#[test]
fn prune_refuses_commits_only_on_the_graft_branch() {
    let s = Setup::new(None);
    let a = s.new_graft("a");
    std::fs::write(a.join("src/lib.rs"), "pub fn b() {}\n").unwrap();
    git(&a, &["commit", "-q", "-am", "work"]);
    s.cmd(&["prune", "a"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "on graft-live/a/live is on no other branch in Root",
        ))
        .stderr(predicate::str::contains("--force"));
    assert!(
        a.join("src/lib.rs").is_file(),
        "a refused prune touches nothing"
    );
    // Once another branch has the work, nothing is lost.
    s.root_git(&["branch", "keep", "graft-live/a/live"]);
    s.ok(&["prune", "a"]);
    let branches = s.root_git(&["branch", "--format=%(refname:short)"]);
    assert_eq!(branches, "keep\nmain\n");
}

#[test]
fn git_worktree_remove_then_prune_cleans_up_and_keeps_work() {
    let s = Setup::new(None);
    let a = s.new_graft("a");
    let b = s.new_graft("b");
    std::fs::write(a.join("src/lib.rs"), "pub fn b() {}\n").unwrap();
    git(&a, &["commit", "-q", "-am", "work"]);
    // What an editor's "remove worktree" does.
    for p in [&a, &b] {
        s.root_git(&["worktree", "remove", p.to_str().unwrap()]);
    }
    let ls = s.ok(&["ls"]);
    assert!(ls.contains("missing"), "{ls}");
    // b had nothing of its own: its branch goes. a's commit is only on its
    // branch, so the branch stays, and prune says so.
    s.cmd(&["prune", "a"])
        .assert()
        .success()
        .stderr(predicate::str::contains("kept branch graft-live/a/live"));
    s.ok(&["prune", "b"]);
    let branches = s.root_git(&["branch", "--format=%(refname:short)"]);
    assert_eq!(branches, "graft-live/a/live\nmain\n");
    assert_eq!(s.ok(&["ls", "--json"]).matches("\"name\"").count(), 0);
}

#[test]
fn a_registered_graft_cannot_take_a_branch_another_worktree_has() {
    // The trade-off of registering (ADR 0036): git's one-branch-per-worktree
    // rule applies, as for any worktree.
    let s = Setup::new(None);
    let a = s.new_graft("a");
    let o = Command::new("git")
        .args(["checkout", "-q", "main"])
        .current_dir(&a)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(!o.status.success());
    // git 2.42 changed "is already checked out at" to "is already used by
    // worktree at".
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains("already used by worktree") || err.contains("already checked out at"),
        "{err}"
    );
}

#[test]
fn a_refused_registration_leaves_what_was_in_the_way() {
    // Regression: a failed `graft new` must undo only what it made. A
    // `graft-live/a/live` branch with work of its own (or another worktree
    // with the id `graft-a`) is someone else's and is kept.
    let s = Setup::new(None);
    git(s.repo.path(), &["branch", "graft-live/a/live"]);
    git(s.repo.path(), &["checkout", "-q", "graft-live/a/live"]);
    std::fs::write(s.repo.path().join("mine.txt"), "mine\n").unwrap();
    git(s.repo.path(), &["add", "mine.txt"]);
    git(s.repo.path(), &["commit", "-q", "-m", "mine"]);
    git(s.repo.path(), &["checkout", "-q", "main"]);
    let mine = s.root_git(&["rev-parse", "graft-live/a/live"]);
    s.cmd(&["new", "a", "--backend", "reflink"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("is on no other branch"));
    assert_eq!(s.root_git(&["rev-parse", "graft-live/a/live"]), mine);
    assert_eq!(s.ok(&["ls", "--json"]).matches("\"name\"").count(), 0);

    // git names a worktree's id after its directory.
    let elsewhere = tempfile::tempdir().unwrap();
    let other = elsewhere.path().join("graft-b");
    git(
        s.repo.path(),
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "theirs",
            other.to_str().unwrap(),
        ],
    );
    let admin = s.repo.path().join(".git/worktrees");
    let gitdir = std::fs::read_to_string(admin.join("graft-b/gitdir")).unwrap();
    s.cmd(&["new", "b", "--backend", "reflink"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "already has a worktree with id graft-b",
        ));
    assert_eq!(
        std::fs::read_to_string(admin.join("graft-b/gitdir")).unwrap(),
        gitdir
    );
    assert!(s.root_git(&["worktree", "list"]).contains("[theirs]"));
}

#[test]
fn the_setting_only_affects_new_grafts() {
    let s = Setup::new(None);
    std::fs::write(s.home.path().join("config.toml"), "").unwrap();
    let old = s.new_graft("old");
    assert!(old.join(".git").is_dir(), "private repository, as before");
    std::fs::write(
        s.home.path().join("config.toml"),
        "[git]\nworktrees = true\n",
    )
    .unwrap();
    let new = s.new_graft("new");
    assert!(new.join(".git").is_file());
    let list = s.root_git(&["worktree", "list"]);
    assert_eq!(list.lines().count(), 2, "{list}");
    // Both prune as they were made.
    s.ok(&["prune", "old"]);
    s.ok(&["prune", "new"]);
    assert_eq!(s.root_git(&["worktree", "list"]).lines().count(), 1);
}

#[test]
fn registered_graft_layers_are_ignored() {
    let toml = "[[layer]]\npath = \"deps\"\ninputs = [\"deps.lock\"]\nbuild = 'mkdir deps && cp deps.lock deps/copy'\n";
    let s = Setup::new(Some(toml));
    std::fs::write(s.repo.path().join("deps.lock"), "v1\n").unwrap();
    git(s.repo.path(), &["add", "deps.lock"]);
    git(s.repo.path(), &["commit", "-q", "-m", "lock"]);
    s.ok(&["trust"]);
    let a = s.new_graft("a");
    assert_eq!(
        std::fs::read_to_string(a.join("deps/copy")).unwrap(),
        "v1\n"
    );
    assert_eq!(
        out(&a, &["status", "--porcelain"]),
        "",
        "the layer is ignored"
    );
    // Only in the graft: Root's own excludes are unchanged.
    let exclude =
        std::fs::read_to_string(s.repo.path().join(".git/info/exclude")).unwrap_or_default();
    assert!(!exclude.contains("deps"), "{exclude}");
    s.ok(&["prune", "a"]);
}

#[test]
fn integrate_lands_a_registered_graft() {
    let s = Setup::new(None);
    s.root_git(&["branch", "land"]);
    let a = s.new_graft("a");
    std::fs::write(a.join("src/lib.rs"), "pub fn b() {}\n").unwrap();
    git(&a, &["commit", "-q", "-am", "feat: add b"]);
    let report = s.ok(&["integrate", "a", "--onto", "land"]);
    assert!(report.contains("landed"), "{report}");
    assert_eq!(
        s.root_git(&["log", "-1", "--format=%s", "land"]).trim(),
        "feat: add b"
    );
    assert_eq!(s.root_git(&["show", "land:src/lib.rs"]), "pub fn b() {}\n");
    // Integrated: prune needs no --force and removes the registration.
    s.ok(&["prune", "a"]);
    assert_eq!(s.root_git(&["worktree", "list"]).lines().count(), 1);
    s.root_git(&["fsck", "--strict", "--no-progress"]);
}

/// Whether this machine can confine commands (see `run.rs`).
fn sandbox_usable() -> bool {
    if cfg!(target_os = "linux") {
        std::fs::read_to_string("/sys/kernel/security/lsm")
            .is_ok_and(|l| l.trim().split(',').any(|m| m == "landlock"))
    } else {
        cfg!(target_os = "macos")
    }
}

#[test]
fn run_in_a_registered_graft_can_commit_but_not_touch_root() {
    if !sandbox_usable() {
        return;
    }
    let s = Setup::new(None);
    s.new_graft("a");
    let main_before = s.root_git(&["rev-parse", "main"]);
    let git_dir = s.repo.path().join(".git").canonicalize().unwrap();
    let q = |p: &Path| format!("'{}'", p.display());
    let script = format!(
        "echo y > f.txt && git add f.txt && git commit -qm 'feat: f' \
         && ! (echo x > {main} 2>/dev/null) \
         && ! (echo x >> {config} 2>/dev/null) \
         && ! (echo x > {hook} 2>/dev/null) \
         && ! (echo x > {escape} 2>/dev/null) \
         && ! git branch other 2>/dev/null",
        main = q(&git_dir.join("refs/heads/main")),
        config = q(&git_dir.join("config")),
        hook = q(&git_dir.join("hooks/pre-commit")),
        escape = q(&s.repo.path().join("escape.txt")),
    );
    s.cmd(&["run", "a", "--", "sh", "-c", &script])
        .env("GIT_AUTHOR_NAME", "Graft Test")
        .env("GIT_AUTHOR_EMAIL", "test@graft.invalid")
        .env("GIT_COMMITTER_NAME", "Graft Test")
        .env("GIT_COMMITTER_EMAIL", "test@graft.invalid")
        .assert()
        .success();
    assert_eq!(
        s.root_git(&["log", "-1", "--format=%s", "graft-live/a/live"])
            .trim(),
        "feat: f"
    );
    assert_eq!(s.root_git(&["rev-parse", "main"]), main_before);
    assert!(!git_dir.join("hooks/pre-commit").exists());
    assert!(!s.repo.path().join("escape.txt").exists());
    assert_eq!(
        s.root_git(&["branch", "--format=%(refname:short)"]),
        "graft-live/a/live\nmain\n"
    );
}

/// A run in one registered graft cannot move or delete another graft's
/// branch: each graft's branch has a ref directory of its own (ADR 0036).
#[test]
fn run_cannot_touch_another_grafts_branch() {
    if !sandbox_usable() {
        return;
    }
    let s = Setup::new(None);
    s.new_graft("a");
    s.new_graft("b");
    let b_before = s.root_git(&["rev-parse", "graft-live/b/live"]);
    let git_dir = s.repo.path().join(".git").canonicalize().unwrap();
    let b_ref = git_dir.join("refs/heads/graft-live/b/live");
    let script = format!(
        "! git update-ref -d refs/heads/graft-live/b/live 2>/dev/null \
         && ! git update-ref refs/heads/graft-live/b/live HEAD~0 2>/dev/null \
         && ! (rm -f '{b}' 2>/dev/null && test ! -e '{b}') \
         && ! git branch graft-live/c/live 2>/dev/null",
        b = b_ref.display()
    );
    s.cmd(&["run", "a", "--", "sh", "-c", &script])
        .assert()
        .success();
    assert!(b_ref.is_file(), "b's branch was removed");
    assert_eq!(s.root_git(&["rev-parse", "graft-live/b/live"]), b_before);
}

/// A run adds objects to Root's object store but cannot remove or change
/// the ones already there. Seatbelt can tell the two apart; Landlock
/// cannot (see `graft_sandbox::Policy::with_create_only`).
#[cfg(target_os = "macos")]
#[test]
fn run_cannot_remove_roots_objects() {
    use std::os::unix::fs::PermissionsExt as _;
    let s = Setup::new(None);
    s.new_graft("a");
    let git_dir = s.repo.path().join(".git").canonicalize().unwrap();
    let head = s.root_git(&["rev-parse", "main"]);
    let head = head.trim();
    let object = git_dir.join("objects").join(&head[..2]).join(&head[2..]);
    assert!(
        object.is_file(),
        "{} is not a loose object",
        object.display()
    );
    let script = format!(
        "! rm -f '{o}' 2>/dev/null; ! chmod 644 '{o}' 2>/dev/null; \
         ! mv '{o}' '{o}.x' 2>/dev/null; test -f '{o}'",
        o = object.display()
    );
    s.cmd(&["run", "a", "--", "sh", "-c", &script])
        .assert()
        .success();
    assert!(object.is_file(), "Root's commit object was removed");
    assert_eq!(
        std::fs::metadata(&object).unwrap().permissions().mode() & 0o222,
        0,
        "Root's object became writable"
    );
    s.root_git(&["cat-file", "-e", head]);
}

/// Rings of a registered reflink graft live with its registration in
/// Root's `.git` (per-worktree refs): rewind works, Root's own refs never
/// see them, and they go when the graft is pruned (ADR 0038).
#[test]
fn rings_of_a_registered_graft_stay_in_its_worktree() {
    let s = Setup::new(None);
    let a = s.new_graft("a");
    std::fs::write(a.join("src/lib.rs"), "pub fn ringed() {}\n").unwrap();
    s.ok(&["ring", "a", "r1"]);
    std::fs::write(a.join("src/lib.rs"), "pub fn later() {}\n").unwrap();
    s.ok(&["rewind", "a", "r1"]);
    assert_eq!(
        std::fs::read_to_string(a.join("src/lib.rs")).unwrap(),
        "pub fn ringed() {}\n"
    );
    assert_eq!(s.root_git(&["for-each-ref", "refs/worktree"]).trim(), "");
    let admin = s.repo.path().join(".git/worktrees/graft-a");
    assert!(admin.join("refs/worktree/graft/rings/r1").is_file());
    s.ok(&["prune", "a", "--force"]);
    assert!(!admin.exists());
}
