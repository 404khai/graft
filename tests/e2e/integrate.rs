//! `graft integrate`, `graft queue` and `graft rebase` (AGENTS.md Phase 13,
//! ADR 0017), on reflink grafts.
//!
//! The structural-merge test needs Mergiraf (`$GRAFT_MERGIRAF` or `PATH`);
//! with `GRAFT_TEST_MERGIRAF=1` its absence is a failure. Without it, the
//! same fixture is checked to stop as a conflict.

use std::path::{Path, PathBuf};
use std::process::Command;

use predicates::prelude::*;

use crate::{git, git_repo, graft};

const CONFIG: &str = "pub struct Config {\n    pub name: String,\n}\n";

struct Fixture {
    home: tempfile::TempDir,
    repo: tempfile::TempDir,
}

/// `git` in `dir`, returning stdout.
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

impl Fixture {
    fn new(toml: &str, files: &[(&str, &str)]) -> Self {
        let home = tempfile::tempdir().unwrap();
        let repo = git_repo();
        git(repo.path(), &["config", "user.name", "Graft Test"]);
        git(repo.path(), &["config", "user.email", "test@graft.invalid"]);
        std::fs::write(repo.path().join("graft.toml"), toml).unwrap();
        for (p, c) in files {
            let path = repo.path().join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, c).unwrap();
        }
        git(repo.path(), &["add", "-A"]);
        git(repo.path(), &["commit", "-qm", "fixture"]);
        git(repo.path(), &["branch", "land"]);
        let f = Self { home, repo };
        f.ok(&["init"]);
        f.ok(&["trust"]);
        f
    }

    fn cmd(&self) -> assert_cmd::Command {
        let mut c = graft(self.home.path());
        c.current_dir(self.repo.path())
            .env_remove("GRAFT_FS")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        c
    }

    fn ok(&self, args: &[&str]) -> String {
        let o = self.cmd().args(args).output().unwrap();
        assert!(
            o.status.success(),
            "graft {args:?}: {}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8(o.stdout).unwrap()
    }

    fn new_graft(&self, name: &str) -> PathBuf {
        self.ok(&["new", name, "--backend", "reflink", "--from", "land"]);
        PathBuf::from(self.ok(&["path", name]).trim())
    }

    fn rev(&self, r: &str) -> String {
        out(self.repo.path(), &["rev-parse", r]).trim().to_owned()
    }

    /// `git fsck` and a clean Root working tree.
    fn assert_root_clean(&self) {
        let fsck = Command::new("git")
            .args(["fsck", "--strict", "--no-progress"])
            .current_dir(self.repo.path())
            .output()
            .unwrap();
        assert!(
            fsck.status.success(),
            "fsck: {}",
            String::from_utf8_lossy(&fsck.stderr)
        );
        assert_eq!(
            out(self.repo.path(), &["status", "--porcelain"]),
            "",
            "Root's working tree changed"
        );
    }
}

const GATE: &str = "[integrate]\ntest = \"sh check.sh\"\ntimeout_secs = 60\n";
const CHECK: &str =
    "#!/bin/sh\n# The gate: fails if any graft left a file called broken.\ntest ! -e broken\n";

#[test]
fn non_overlapping_grafts_integrate_unattended() {
    let f = Fixture::new(
        GATE,
        &[
            ("check.sh", CHECK),
            ("src/a.txt", "a\n"),
            ("src/b.txt", "b\n"),
        ],
    );
    let a = f.new_graft("a");
    let b = f.new_graft("b");
    std::fs::write(a.join("src/a.txt"), "a changed\n").unwrap();
    std::fs::write(b.join("src/b.txt"), "b changed\n").unwrap();
    std::fs::write(b.join("src/new.txt"), "new\n").unwrap();
    git(&b, &["add", "src/new.txt"]);
    git(&b, &["commit", "-qm", "feat: add new.txt"]);
    let before = f.rev("main");
    let report = f.ok(&["integrate", "a", "b", "--onto", "land"]);
    assert!(
        report.contains("landed   a → land") && report.contains("landed   b → land"),
        "{report}"
    );
    // Two ordinary commits on land, parent to child, nothing else moved.
    let log = out(f.repo.path(), &["log", "--format=%s%n%b", "land", "-2"]);
    assert!(log.contains("feat: add new.txt"), "{log}");
    assert!(log.contains("chore: integrate graft a"), "{log}");
    assert!(
        log.contains("Graft: a ") && log.contains("Graft: b "),
        "{log}"
    );
    assert_eq!(
        out(f.repo.path(), &["show", "land:src/a.txt"]),
        "a changed\n"
    );
    assert_eq!(
        out(f.repo.path(), &["show", "land:src/b.txt"]),
        "b changed\n"
    );
    assert_eq!(out(f.repo.path(), &["show", "land:src/new.txt"]), "new\n");
    assert_eq!(f.rev("main"), before, "main moved");
    f.assert_root_clean();
    let ls = f.ok(&["ls"]);
    assert!(ls.contains("integrated"), "{ls}");
    let q = f.ok(&["queue"]);
    assert_eq!(q.matches("landed").count(), 2, "{q}");
    // An integrated graft prunes without --force.
    f.ok(&["prune", "a"]);
    let refs = out(f.repo.path(), &["for-each-ref", "refs/graft/integrating"]);
    assert_eq!(refs, "", "staging refs left");
}

#[test]
fn a_failing_gate_keeps_the_graft_live_and_root_unchanged() {
    let f = Fixture::new(GATE, &[("check.sh", CHECK)]);
    let a = f.new_graft("a");
    std::fs::write(a.join("broken"), "oops\n").unwrap();
    let before = f.rev("land");
    let o = f
        .cmd()
        .args(["integrate", "a", "--onto", "land"])
        .output()
        .unwrap();
    assert!(!o.status.success());
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(stdout.contains("failed   a → land"), "{stdout}");
    assert!(
        stdout.contains("test gate `sh check.sh` failed"),
        "{stdout}"
    );
    assert_eq!(
        f.rev("land"),
        before,
        "Root received a commit that failed the gate"
    );
    let objects_before = out(f.repo.path(), &["count-objects", "-v"]);
    assert!(f.ok(&["ls"]).contains(" live "), "graft not live");
    f.cmd()
        .arg("queue")
        .assert()
        .success()
        .stdout(predicate::str::contains("failed").and(predicate::str::contains("test gate")));
    // Fixed, it lands; a dry run first changes nothing.
    std::fs::remove_file(a.join("broken")).unwrap();
    std::fs::write(a.join("fixed.txt"), "ok\n").unwrap();
    let dry = f.ok(&["integrate", "a", "--onto", "land", "--dry-run"]);
    assert!(dry.contains("checked  a → land"), "{dry}");
    assert_eq!(f.rev("land"), before);
    assert_eq!(out(f.repo.path(), &["count-objects", "-v"]), objects_before);
    f.ok(&[
        "integrate",
        "a",
        "--onto",
        "land",
        "-m",
        "fix: add fixed.txt",
    ]);
    assert_eq!(
        out(f.repo.path(), &["log", "-1", "--format=%s", "land"]),
        "fix: add fixed.txt\n"
    );
    f.assert_root_clean();
}

#[test]
fn the_checked_out_branch_needs_update_checkout() {
    let f = Fixture::new("", &[("x.txt", "x\n")]);
    let a = f.new_graft("a");
    std::fs::write(a.join("x.txt"), "y\n").unwrap();
    let o = f
        .cmd()
        .args(["integrate", "a", "--onto", "main"])
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stdout).contains("checked out in Root"));
    assert_eq!(
        std::fs::read_to_string(f.repo.path().join("x.txt")).unwrap(),
        "x\n"
    );
    f.ok(&["integrate", "a", "--onto", "main", "--update-checkout"]);
    assert_eq!(
        std::fs::read_to_string(f.repo.path().join("x.txt")).unwrap(),
        "y\n"
    );
    f.assert_root_clean();
}

/// AGENTS.md Phase 13: a conflict that a line-based merge fails is
/// resolved by the structural merge step.
#[test]
fn a_reordering_conflict_is_resolved_structurally() {
    let f = Fixture::new("", &[("src/config.rs", CONFIG)]);
    let a = f.new_graft("a");
    let b = f.new_graft("b");
    std::fs::write(
        a.join("src/config.rs"),
        CONFIG.replace("String,\n", "String,\n    pub port: u16,\n"),
    )
    .unwrap();
    std::fs::write(
        b.join("src/config.rs"),
        CONFIG.replace("String,\n", "String,\n    pub verbose: bool,\n"),
    )
    .unwrap();
    f.ok(&["integrate", "a", "--onto", "land"]);
    let mergiraf = graft_integrate::Merger::detect();
    if mergiraf.mergiraf().is_none() {
        assert_ne!(
            std::env::var("GRAFT_TEST_MERGIRAF").as_deref(),
            Ok("1"),
            "mergiraf required"
        );
        let o = f
            .cmd()
            .args(["integrate", "b", "--onto", "land"])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&o.stdout);
        assert!(
            stdout.contains("conflict b → land") && stdout.contains("Mergiraf is not installed"),
            "{stdout}"
        );
        return;
    }
    let report = f.ok(&["integrate", "b", "--onto", "land"]);
    assert!(report.contains("landed   b → land"), "{report}");
    let merged = out(f.repo.path(), &["show", "land:src/config.rs"]);
    assert!(
        merged.contains("pub port: u16,") && merged.contains("pub verbose: bool,"),
        "{merged}"
    );
    assert!(
        f.ok(&["queue", "--verbose"])
            .contains("structural merge (Mergiraf): src/config.rs")
    );
    f.assert_root_clean();
}

#[test]
fn the_resolver_hook_settles_conflicts() {
    let resolver = "[integrate]\nresolver = \"sh resolve.sh\"\n";
    // The resolver keeps the graft's side: theirs is after the =======.
    let script = "for f in $GRAFT_CONFLICTS; do test -s \"$GRAFT_INTEGRATE_INFO/graft.diff\" || exit 1; sed -e '/^<<<<<<< /,/^=======$/d' -e '/^>>>>>>> /d' \"$f\" > \"$f.new\" && mv \"$f.new\" \"$f\"; done\n";
    let f = Fixture::new(resolver, &[("resolve.sh", script), ("v.txt", "1\n")]);
    let a = f.new_graft("a");
    let b = f.new_graft("b");
    std::fs::write(a.join("v.txt"), "2\n").unwrap();
    std::fs::write(b.join("v.txt"), "3\n").unwrap();
    f.ok(&["integrate", "a", "--onto", "land"]);
    let report = f.ok(&["integrate", "b", "--onto", "land"]);
    assert!(report.contains("landed   b → land"), "{report}");
    assert_eq!(out(f.repo.path(), &["show", "land:v.txt"]), "3\n");
}

#[test]
fn unapproved_gates_stop_the_entry() {
    let f = Fixture::new("", &[("check.sh", CHECK)]);
    // A gate added after `graft trust` is not approved.
    std::fs::write(f.repo.path().join("graft.toml"), GATE).unwrap();
    git(f.repo.path(), &["commit", "-qam", "add a gate"]);
    git(f.repo.path(), &["branch", "-f", "land", "main"]);
    let a = f.new_graft("a");
    std::fs::write(a.join("z.txt"), "z\n").unwrap();
    let o = f
        .cmd()
        .args(["integrate", "a", "--onto", "land"])
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stdout).contains("not approved"));
}

#[test]
fn rebase_moves_a_live_graft_onto_the_new_head() {
    let f = Fixture::new("", &[("one.txt", "1\n"), ("two.txt", "2\n")]);
    let a = f.new_graft("a");
    let b = f.new_graft("b");
    std::fs::write(a.join("one.txt"), "one\n").unwrap();
    std::os::unix::fs::symlink("one.txt", a.join("one-link")).unwrap();
    std::fs::write(b.join("two.txt"), "two\n").unwrap();
    f.ok(&["integrate", "a", "--onto", "land"]);
    let rebased = f.ok(&["rebase", "b", "--onto", "land"]);
    assert!(rebased.contains("Rebased graft b onto land"), "{rebased}");
    assert_eq!(std::fs::read_to_string(b.join("one.txt")).unwrap(), "one\n");
    assert_eq!(std::fs::read_to_string(b.join("two.txt")).unwrap(), "two\n");
    assert_eq!(
        std::fs::read_link(b.join("one-link")).unwrap(),
        PathBuf::from("one.txt")
    );
    // b's own change is all that differs from its new base.
    assert_eq!(out(&b, &["status", "--porcelain"]), " M two.txt\n");
    let status = f.ok(&["status", "b"]);
    assert!(
        status.contains("two.txt") && !status.contains("one.txt"),
        "{status}"
    );
    assert!(
        f.ok(&["rebase", "b", "--onto", "land"])
            .contains("already based")
    );
}

#[test]
fn symlinks_land_as_symlinks() {
    let f = Fixture::new("", &[("target.txt", "t\n")]);
    let a = f.new_graft("a");
    std::os::unix::fs::symlink("target.txt", a.join("link")).unwrap();
    f.ok(&["integrate", "a", "--onto", "land"]);
    let tree = out(f.repo.path(), &["ls-tree", "land", "link"]);
    assert!(tree.starts_with("120000 "), "not a symlink in git: {tree}");
    assert_eq!(
        out(f.repo.path(), &["cat-file", "-p", "land:link"]),
        "target.txt"
    );
}
