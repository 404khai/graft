//! `graft new`, `ls`, `path`, `status`, `diff` and `prune`.

use std::os::unix::fs::PermissionsExt as _;
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
    // The tree was moved aside and is deleted in the background.
    let mounts = path.parent().unwrap();
    assert!(
        empties(mounts),
        "mounts dir not empty: {:?}",
        entries(mounts)
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

fn entries(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

/// Whether `dir` becomes empty within 20 s.
fn empties(dir: &Path) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        if entries(dir).is_empty() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    false
}

/// `--wait` deletes before returning; the name is free again at once.
#[test]
fn prune_wait_deletes_before_returning_and_frees_the_name() {
    let s = Setup::new();
    s.run(&["new", "w"]).success();
    let path = s.path("w");
    std::fs::create_dir_all(path.join("target/debug")).unwrap();
    for i in 0..200 {
        std::fs::write(path.join(format!("target/debug/f{i}")), "x").unwrap();
    }
    s.run(&["prune", "w", "--force", "--wait"]).success();
    assert_eq!(entries(path.parent().unwrap()), Vec::<String>::new());
    s.run(&["new", "w"]).success();
    s.run(&["prune", "w"]).success();
    s.run(&["new", "w"]).success();
    assert!(s.path("w").join("src/lib.rs").exists());
}

/// A background delete that never ran (here: a retired tree planted by
/// hand) is finished by `graft gc`, which only touches `.trash-*` entries.
#[test]
fn gc_finishes_an_interrupted_prune() {
    let s = Setup::new();
    s.run(&["new", "keep"]).success();
    let mounts = s.path("keep").parent().unwrap().to_owned();
    let left = mounts.join(".trash-00000000000000000000000000000000");
    std::fs::create_dir_all(left.join("a/b")).unwrap();
    std::fs::write(left.join("a/b/c"), "x").unwrap();
    s.run(&["gc"])
        .success()
        .stdout(predicate::str::contains("Removed 1 pruned working tree(s)"));
    assert!(!left.exists());
    assert_eq!(entries(&mounts), ["keep"]);
}

#[test]
fn ls_no_disk_skips_measuring() {
    let s = Setup::new();
    s.run(&["new", "a"]).success();
    let ls = s.stdout(&["ls", "--no-disk"]);
    let row = ls.lines().nth(1).unwrap();
    assert!(row.trim_end().ends_with(" -"), "{ls}");
    let v: serde_json::Value =
        serde_json::from_str(&s.stdout(&["ls", "--json", "--no-disk"])).unwrap();
    assert!(v["grafts"][0]["disk_bytes"].is_null(), "{v}");
    assert_eq!(v["grafts"][0]["note"], "disk not measured", "{v}");
    let v: serde_json::Value = serde_json::from_str(&s.stdout(&["ls", "--json"])).unwrap();
    assert!(v["grafts"][0]["disk_bytes"].is_u64(), "{v}");
}

/// `--clone-ignored` brings Root's ignored build outputs into a new graft
/// as they are, with no build; they stay private and never block prune.
#[test]
fn clone_ignored_brings_roots_build_outputs() {
    let s = Setup::new();
    let r = s.repo.path();
    std::fs::write(r.join(".gitignore"), "/target/\nweb/node_modules\n").unwrap();
    git(r, &["add", ".gitignore"]);
    git(r, &["commit", "-qm", "ignore"]);
    std::fs::create_dir_all(r.join("target/debug/deps")).unwrap();
    std::fs::write(r.join("target/debug/deps/app"), "binary").unwrap();
    std::fs::create_dir_all(r.join("web/node_modules/left-pad")).unwrap();
    std::fs::write(
        r.join("web/node_modules/left-pad/index.js"),
        "module.exports = 1\n",
    )
    .unwrap();

    let out = s.stdout(&[
        "new",
        "a",
        "--clone-ignored",
        "target,web/node_modules/",
        "--json",
    ]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["cloned"][0]["path"], "target", "{v}");
    assert_eq!(v["cloned"][1]["path"], "web/node_modules", "{v}");
    let total =
        v["cloned"][0]["reflinked"].as_u64().unwrap() + v["cloned"][0]["copied"].as_u64().unwrap();
    assert_eq!(total, 1, "{v}");
    let a = s.path("a");
    assert_eq!(
        std::fs::read_to_string(a.join("target/debug/deps/app")).unwrap(),
        "binary"
    );
    assert!(a.join("web/node_modules/left-pad/index.js").is_file());

    // Private: a write in the graft does not reach Root.
    std::fs::write(a.join("target/debug/deps/app"), "rebuilt").unwrap();
    assert_eq!(
        std::fs::read_to_string(r.join("target/debug/deps/app")).unwrap(),
        "binary"
    );
    // Ignored, so not a change and no reason to refuse prune.
    let status = s.stdout(&["status", "a"]);
    assert!(status.contains("nothing changed since base"), "{status}");
    s.run(&["prune", "a"]).success();
    assert!(
        r.join("target/debug/deps/app").is_file(),
        "prune touched Root"
    );
}

#[test]
fn clone_ignored_refuses_what_it_should_not_copy() {
    let s = Setup::new();
    let r = s.repo.path();
    std::fs::write(r.join(".gitignore"), "/out\n/link\n").unwrap();
    git(r, &["add", ".gitignore"]);
    git(r, &["commit", "-qm", "ignore"]);
    std::fs::create_dir_all(r.join("out/x")).unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), r.join("link")).unwrap();
    for (arg, needle) in [
        ("src", "not ignored by git"),
        ("missing", "not in Root's working tree"),
        ("../etc", "must not contain . or .."),
        ("/etc", "must be relative"),
        (".git", "inside .git"),
        ("link", "not a directory"),
        ("out,out/x", "inside out"),
    ] {
        s.run(&["new", "x", "--clone-ignored", arg])
            .failure()
            .stderr(predicate::str::contains(needle));
    }
    // Nothing was created by any refused call.
    s.run(&["ls"])
        .success()
        .stdout(predicate::str::contains("No grafts"));
    s.run(&["new", "x", "--clone-ignored", "out", "--backend", "fuse"])
        .failure()
        .stderr(predicate::str::contains("needs a reflink graft"));
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
        diff.contains("deleted file mode 100644\n--- a/src/lib.rs\n+++ /dev/null\n"),
        "{diff}"
    );
    assert!(
        diff.contains("new file mode 100644\n--- /dev/null\n+++ b/new.txt\n"),
        "{diff}"
    );
    let ls = s.stdout(&["ls"]);
    assert!(ls.lines().nth(1).unwrap().contains(" 3 "), "{ls}");
}

#[test]
fn published_ancestry_advances_display_base_without_hiding_local_work() {
    let s = Setup::new();
    s.run(&["new", "a"]).success();
    let a = s.path("a");
    let base = git_out(&a, &["rev-parse", "HEAD"]).trim().to_owned();
    std::fs::write(s.repo.path().join("upstream.txt"), "published\n").unwrap();
    git(s.repo.path(), &["add", "-A"]);
    git(s.repo.path(), &["commit", "-qm", "upstream"]);
    let published = git_out(s.repo.path(), &["rev-parse", "HEAD"])
        .trim()
        .to_owned();
    git(&a, &["checkout", "-B", "main"]);
    git(&a, &["pull", "--ff-only", "root", "main"]);
    let listing: serde_json::Value =
        serde_json::from_str(&s.stdout(&["ls", "--json", "--no-disk"])).unwrap();
    assert_eq!(listing["grafts"][0]["base"], published);
    assert_eq!(listing["grafts"][0]["recorded_base"], base);
    assert_eq!(listing["grafts"][0]["changed"], 0);
    let text = s.stdout(&["status", "a"]);
    assert!(
        text.contains(&format!(
            "base {} (recorded {}",
            &published[..7],
            &base[..7]
        )),
        "{text}"
    );
    assert!(
        text.contains("committed since base: 0; uncommitted: 0"),
        "{text}"
    );
    let ls = s.stdout(&["ls", "--no-disk"]);
    assert!(ls.lines().nth(1).unwrap().contains(&published[..7]), "{ls}");

    std::fs::write(a.join("local.txt"), "private\n").unwrap();
    git(&a, &["add", "-A"]);
    git(&a, &["commit", "-qm", "local"]);
    let head = git_out(&a, &["rev-parse", "HEAD"]).trim().to_owned();
    let status: serde_json::Value =
        serde_json::from_str(&s.stdout(&["status", "a", "--json"])).unwrap();
    let view = &status["grafts"][0];
    assert_eq!(view["base"], published);
    assert_eq!(view["recorded_base"], base);
    assert_eq!(view["committed"], 1);
    assert_eq!(view["uncommitted"], 0);
    assert_eq!(
        view["changes"],
        serde_json::json!([{"kind": "A", "path": "local.txt"}])
    );
    assert!(
        s.stdout(&["status", "a"])
            .contains("committed since base: 1; uncommitted: 0")
    );
    let listing: serde_json::Value =
        serde_json::from_str(&s.stdout(&["ls", "--json", "--no-disk"])).unwrap();
    assert_eq!(listing["grafts"][0]["changed"], 1);
    let ring = s.stdout(&["ring", "a", "clean"]);
    assert!(
        ring.contains(&format!(
            "snapshot of HEAD {} + 0 uncommitted paths vs HEAD",
            &head[..7]
        )),
        "{ring}"
    );
    let rings: serde_json::Value =
        serde_json::from_str(&s.stdout(&["ring", "a", "--list", "--json"])).unwrap();
    assert_eq!(rings[0]["head"], head);
    assert_eq!(rings[0]["uncommitted_paths"], 0);
    assert_eq!(rings[0]["entries"], 0);
    assert_ne!(rings[0]["commit"], head);
    std::fs::write(a.join("loose.txt"), "untracked\n").unwrap();
    let status: serde_json::Value =
        serde_json::from_str(&s.stdout(&["status", "a", "--json"])).unwrap();
    assert_eq!(status["grafts"][0]["uncommitted"], 1);
    let ring: serde_json::Value =
        serde_json::from_str(&s.stdout(&["ring", "a", "dirty", "--json"])).unwrap();
    assert_eq!(ring["uncommitted_paths"], 1);
    assert_eq!(ring["head"], head);
    std::fs::write(
        s.repo.path().join("later-upstream.txt"),
        "published later\n",
    )
    .unwrap();
    git(s.repo.path(), &["add", "-A"]);
    git(s.repo.path(), &["commit", "-qm", "upstream again"]);
    let merged_base = git_out(s.repo.path(), &["rev-parse", "HEAD"])
        .trim()
        .to_owned();
    git(&a, &["fetch", "root"]);
    git(&a, &["merge", "--no-edit", "root/main"]);
    let status: serde_json::Value =
        serde_json::from_str(&s.stdout(&["status", "a", "--json"])).unwrap();
    assert_eq!(status["grafts"][0]["base"], merged_base);
    assert_eq!(status["grafts"][0]["committed"], 1);
    assert_eq!(status["grafts"][0]["uncommitted"], 1);
    assert_eq!(s.store().grafts().unwrap()[0].base_commit.as_str(), base);
}

#[test]
fn display_base_uses_remote_refs_and_keeps_recorded_base_after_reset() {
    let s = Setup::new();
    s.run(&["new", "a"]).success();
    let a = s.path("a");
    let base = git_out(&a, &["rev-parse", "HEAD"]).trim().to_owned();
    std::fs::write(a.join("remote.txt"), "published elsewhere\n").unwrap();
    git(&a, &["add", "-A"]);
    git(&a, &["commit", "-qm", "remote"]);
    let published = git_out(&a, &["rev-parse", "HEAD"]).trim().to_owned();
    git(&a, &["update-ref", "refs/remotes/origin/main", &published]);
    let status: serde_json::Value =
        serde_json::from_str(&s.stdout(&["status", "a", "--json"])).unwrap();
    assert_eq!(status["grafts"][0]["base"], published);
    assert_eq!(status["grafts"][0]["changes"], serde_json::json!([]));
    git(&a, &["checkout", "--orphan", "unrelated"]);
    git(&a, &["commit", "-qm", "unrelated"]);
    let status: serde_json::Value =
        serde_json::from_str(&s.stdout(&["status", "a", "--json"])).unwrap();
    assert_eq!(status["grafts"][0]["base"], base);
}

#[test]
fn diff_applies_to_the_base_checkout() {
    let s = Setup::new();
    for (name, contents) in [
        ("empty-deleted", ""),
        ("deleted.sh", "#!/bin/sh\necho old\n"),
    ] {
        std::fs::write(s.repo.path().join(name), contents).unwrap();
    }
    std::fs::set_permissions(
        s.repo.path().join("deleted.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::os::unix::fs::symlink("README.md", s.repo.path().join("old-link")).unwrap();
    git(s.repo.path(), &["add", "-A"]);
    git(s.repo.path(), &["commit", "-qm", "diff base"]);
    s.run(&["new", "patch"]).success();
    let path = s.path("patch");
    assert_eq!(s.stdout(&["diff", "patch"]), "");
    for name in ["src/lib.rs", "empty-deleted", "deleted.sh", "old-link"] {
        std::fs::remove_file(path.join(name)).unwrap();
    }
    for (name, contents) in [
        ("README.md", "hello\nworld\n"),
        ("AUTH.md", "new\n"),
        ("empty-added", ""),
        ("no-newline", "new"),
        ("added.sh", "#!/bin/sh\necho new\n"),
    ] {
        std::fs::write(path.join(name), contents).unwrap();
    }
    std::fs::set_permissions(
        path.join("added.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::os::unix::fs::symlink("AUTH.md", path.join("new-link")).unwrap();
    let diff = s.stdout(&["diff", "patch"]);
    for header in [
        "new file mode 100644\n",
        "new file mode 100755\n",
        "new file mode 120000\n",
        "deleted file mode 100755\n",
        "deleted file mode 120000\n",
        "@@ -0,0 +1,1 @@\n+new\n",
        "@@ -1,1 +0,0 @@\n-pub fn a() {}\n",
    ] {
        assert!(diff.contains(header), "missing {header:?}: {diff}");
    }
    let output_file = s.home.path().join("diff.patch");
    std::fs::write(&output_file, diff).unwrap();
    git_out(
        s.repo.path(),
        &["apply", "--check", output_file.to_str().unwrap()],
    );
    git_out(s.repo.path(), &["apply", output_file.to_str().unwrap()]);
    for name in [
        "README.md",
        "AUTH.md",
        "empty-added",
        "no-newline",
        "added.sh",
    ] {
        assert_eq!(
            std::fs::read(s.repo.path().join(name)).unwrap(),
            std::fs::read(path.join(name)).unwrap(),
            "{name}"
        );
    }
    for name in ["src/lib.rs", "empty-deleted", "deleted.sh", "old-link"] {
        assert!(std::fs::symlink_metadata(s.repo.path().join(name)).is_err());
    }
    assert_eq!(
        std::fs::metadata(s.repo.path().join("added.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert_eq!(
        std::fs::read_link(s.repo.path().join("new-link")).unwrap(),
        Path::new("AUTH.md")
    );
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

/// Rings and rewind on a reflink graft (ADR 0038): a ring snapshots the
/// working tree (tracked and untracked files, not ignored ones) and HEAD;
/// rewind restores them, saving the current state as an undo ring first.
#[test]
fn reflink_rings_snapshot_and_rewind_the_working_tree() {
    let s = Setup::new();
    let r = s.repo.path();
    std::fs::write(r.join(".gitignore"), "/target/\n").unwrap();
    git(r, &["add", ".gitignore"]);
    git(r, &["commit", "-qm", "ignore"]);
    s.run(&["new", "a"]).success();
    let a = s.path("a");
    let gita = |args: &[&str]| git_out(&a, args).trim().to_owned();
    let base = gita(&["rev-parse", "HEAD"]);

    std::fs::write(a.join("src/lib.rs"), "pub fn v1() {}\n").unwrap();
    std::fs::write(a.join("notes.txt"), "untracked\n").unwrap();
    s.run(&["ring", "a", "first"])
        .success()
        .stdout(predicate::str::contains("Ringed graft a as first"));

    // Later work: an edit, a new file, a commit, and build output.
    std::fs::write(a.join("src/lib.rs"), "pub fn v2() {}\n").unwrap();
    std::fs::remove_file(a.join("notes.txt")).unwrap();
    std::fs::write(a.join("later.txt"), "later\n").unwrap();
    git(&a, &["add", "-A"]);
    git(&a, &["commit", "-qm", "later work"]);
    let later = gita(&["rev-parse", "HEAD"]);
    std::fs::create_dir_all(a.join("target")).unwrap();
    std::fs::write(a.join("target/build.o"), "obj").unwrap();

    let out = s.stdout(&["rewind", "a", "first"]);
    assert!(out.contains("To undo: graft rewind a undo-2"), "{out}");
    assert_eq!(
        std::fs::read_to_string(a.join("src/lib.rs")).unwrap(),
        "pub fn v1() {}\n"
    );
    assert_eq!(
        std::fs::read_to_string(a.join("notes.txt")).unwrap(),
        "untracked\n"
    );
    assert!(
        !a.join("later.txt").exists(),
        "a file the ring did not have stayed"
    );
    assert_eq!(gita(&["rev-parse", "HEAD"]), base, "HEAD did not go back");
    assert!(
        a.join("target/build.o").is_file(),
        "an ignored file was touched"
    );
    // Back where it was: the rewind undoes cleanly.
    s.run(&["rewind", "a", "undo-2"]).success();
    assert_eq!(
        std::fs::read_to_string(a.join("src/lib.rs")).unwrap(),
        "pub fn v2() {}\n"
    );
    assert!(a.join("later.txt").is_file() && !a.join("notes.txt").exists());
    assert_eq!(gita(&["rev-parse", "HEAD"]), later);

    let v: serde_json::Value =
        serde_json::from_str(&s.stdout(&["ring", "a", "--list", "--json"])).unwrap();
    let labels: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["label"].as_str().unwrap())
        .collect();
    assert!(
        labels.contains(&"first") && labels.contains(&"undo-2"),
        "{v}"
    );
    s.run(&["ring", "a", "first"])
        .failure()
        .stderr(predicate::str::contains("exists already"));
    s.run(&["rewind", "a", "nope"])
        .failure()
        .stderr(predicate::str::contains("no ring named nope"));
    // Rings belong to the graft and go with it.
    s.run(&["prune", "a", "--force"]).success();
    assert_eq!(git_out(r, &["for-each-ref", "refs/worktree"]).trim(), "");
}
