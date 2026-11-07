//! Virtual grafts (FUSE on Linux, NFS on macOS) and graftd, driven through
//! the `graft` binary.
//!
//! Tests that mount need Linux with `--features fuse-tests` and
//! `GRAFT_TEST_FUSE=1`, or macOS with `--features nfs-tests` and
//! `GRAFT_TEST_NFS=1`. The rest run everywhere.

use std::path::{Path, PathBuf};

use predicates::prelude::*;

use crate::{git_repo, graft};

/// `graftd`, built next to `graft` by `cargo build --workspace`.
fn graftd() -> PathBuf {
    let path = Path::new(env!("CARGO_BIN_EXE_graft"))
        .parent()
        .unwrap()
        .join("graftd");
    assert!(
        path.is_file(),
        "{} is missing; run `cargo build --workspace` first",
        path.display()
    );
    path
}

/// A `graft` command that starts this build's `graftd`.
fn graft_d(home: &Path) -> assert_cmd::Command {
    let mut cmd = graft(home);
    cmd.env("GRAFTD", graftd()).env_remove("GRAFT_FS");
    cmd
}

/// Stops the test's graftd however the test ends.
struct Daemon<'a>(&'a Path);

impl Drop for Daemon<'_> {
    fn drop(&mut self) {
        let _ = graft_d(self.0).args(["daemon", "stop"]).output();
    }
}

#[test]
fn rejects_an_unknown_backend() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    graft(home.path())
        .current_dir(repo.path())
        .arg("init")
        .assert()
        .success();
    graft(home.path())
        .current_dir(repo.path())
        .args(["new", "x"])
        .env("GRAFT_FS", "zfs")
        .assert()
        .failure()
        .stderr(predicate::str::contains("GRAFT_FS"))
        .stderr(predicate::str::contains("unknown filesystem backend"));
    graft(home.path())
        .current_dir(repo.path())
        .args(["new", "x", "--backend", "zfs"])
        .assert()
        .failure();
}

#[test]
fn daemon_status_when_not_running() {
    let home = tempfile::tempdir().unwrap();
    graft_d(home.path())
        .args(["daemon", "status"])
        .assert()
        .success()
        .stdout("graftd is not running\n");
}

#[cfg(not(target_os = "linux"))]
#[test]
fn fuse_grafts_are_refused_off_linux_and_leave_nothing() {
    let home = tempfile::tempdir().unwrap();
    let _daemon = Daemon(home.path());
    let repo = git_repo();
    graft(home.path())
        .current_dir(repo.path())
        .arg("init")
        .assert()
        .success();
    graft_d(home.path())
        .current_dir(repo.path())
        .args(["new", "v", "--backend", "fuse"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("fuse needs Linux"));
    graft_d(home.path())
        .current_dir(repo.path())
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("No grafts"));
    // The daemon was started on demand and is reachable.
    graft_d(home.path())
        .args(["daemon", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("running"));
    let refs = std::process::Command::new("git")
        .args(["for-each-ref", "refs/graft"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(refs.stdout.is_empty(), "a failed graft new left a pin ref");
}

/// Bud, ring and rewind refuse reflink grafts until the owner settles
/// how they should work there (ADR 0013), and leave nothing behind.
#[test]
fn bud_ring_and_rewind_refuse_reflink_grafts() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    graft(home.path())
        .current_dir(repo.path())
        .arg("init")
        .assert()
        .success();
    graft(home.path())
        .current_dir(repo.path())
        .args(["new", "r", "--backend", "reflink"])
        .assert()
        .success();
    for args in [&["bud", "r", "s"][..], &["ring", "r"], &["ring", "r", "x"]] {
        graft(home.path())
            .current_dir(repo.path())
            .args(args)
            .assert()
            .failure()
            .stderr(predicate::str::contains("reflink backend"));
    }
    graft(home.path())
        .current_dir(repo.path())
        .args(["rewind", "r", "x"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("reflink backend"));
    graft(home.path())
        .current_dir(repo.path())
        .args(["ring", "r", "--list"])
        .assert()
        .success()
        .stdout("graft r: no rings\n");
    graft(home.path())
        .current_dir(repo.path())
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains(" s ").not());
}

#[cfg(any(
    all(feature = "fuse-tests", target_os = "linux"),
    all(feature = "nfs-tests", target_os = "macos")
))]
#[allow(clippy::print_stderr)] // Timings and skipped comparisons belong in CI logs.
mod fuse {
    use std::path::Path;
    use std::process::Command;
    use std::time::Instant;

    use predicates::prelude::*;

    use super::{Daemon, graft_d};
    use crate::git;

    /// This platform's virtual backend.
    const BACKEND: &str = if cfg!(target_os = "linux") {
        "fuse"
    } else {
        "nfs"
    };

    fn require_fuse() {
        let var = if cfg!(target_os = "linux") {
            "GRAFT_TEST_FUSE"
        } else {
            "GRAFT_TEST_NFS"
        };
        assert_eq!(
            std::env::var(var).as_deref(),
            Ok("1"),
            "virtual graft tests need {var}=1 and a mountable {BACKEND}"
        );
    }

    fn init(home: &Path, repo: &Path) {
        graft_d(home)
            .current_dir(repo)
            .arg("init")
            .assert()
            .success();
    }

    fn new_fuse(home: &Path, repo: &Path, name: &str) -> std::path::PathBuf {
        graft_d(home)
            .current_dir(repo)
            .args(["new", name, "--backend", BACKEND])
            .assert()
            .success()
            .stdout(predicate::str::contains("virtual"));
        let out = graft_d(home)
            .current_dir(repo)
            .args(["path", name])
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().into()
    }

    #[test]
    fn full_lifecycle_leaves_nothing_behind() {
        require_fuse();
        let home = tempfile::tempdir().unwrap();
        let _daemon = Daemon(home.path());
        let repo = crate::git_repo();
        init(home.path(), repo.path());
        let path = new_fuse(home.path(), repo.path(), "v");
        assert_eq!(
            std::fs::read_to_string(path.join("README.md")).unwrap(),
            "hello\n"
        );
        graft_d(home.path())
            .current_dir(repo.path())
            .args(["status", "v"])
            .assert()
            .success()
            .stdout(predicate::str::contains("nothing changed"));

        // Writes land in the graft's delta, never in Root.
        std::fs::write(path.join("README.md"), "changed\n").unwrap();
        std::fs::write(path.join("new.txt"), "new\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.path().join("README.md")).unwrap(),
            "hello\n"
        );
        graft_d(home.path())
            .current_dir(repo.path())
            .arg("ls")
            .assert()
            .success()
            .stdout(predicate::str::is_match(r"v\s+[0-9a-f]{7}\s+live\s+2\s").unwrap());
        graft_d(home.path())
            .current_dir(repo.path())
            .args(["status", "v"])
            .assert()
            .success()
            .stdout(predicate::str::contains("  M README.md\n  A new.txt\n"));
        graft_d(home.path())
            .current_dir(repo.path())
            .args(["diff", "v"])
            .assert()
            .success()
            .stdout(predicate::str::contains("-hello\n+changed\n"));
        graft_d(home.path())
            .current_dir(repo.path())
            .args(["log", "v"])
            .assert()
            .success()
            .stdout(predicate::str::contains("write").and(predicate::str::contains("create")));

        // Prune refuses to lose the changes, unless forced.
        graft_d(home.path())
            .current_dir(repo.path())
            .args(["prune", "v"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("changes since base"));
        graft_d(home.path())
            .current_dir(repo.path())
            .args(["prune", "v", "--force"])
            .assert()
            .success()
            .stdout("Pruned graft v\n");
        assert!(!path.exists());
        let mounts = path.parent().unwrap();
        let left: Vec<_> = std::fs::read_dir(mounts).unwrap().flatten().collect();
        assert!(left.is_empty(), "prune left {left:?}");
        let root_dir = mounts.parent().unwrap();
        let files: Vec<_> = ["blobs", "grafts"]
            .iter()
            .flat_map(|d| walk_files(&root_dir.join(d)))
            .collect();
        assert_eq!(
            files,
            Vec::<std::path::PathBuf>::new(),
            "prune left blobs or graft state"
        );
        let refs = Command::new("git")
            .args(["for-each-ref", "refs/graft"])
            .current_dir(repo.path())
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&refs.stdout),
            "",
            "prune left a pin ref"
        );
        graft_d(home.path())
            .current_dir(repo.path())
            .arg("ls")
            .assert()
            .stdout(predicate::str::contains("No grafts"));
    }

    fn run(home: &Path, repo: &Path, args: &[&str]) -> String {
        let out = graft_d(home).current_dir(repo).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "graft {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    /// AGENTS.md Phase 9: bud forks a graft's delta, ring checkpoints it,
    /// rewind restores it and can be undone, and pruning a graft keeps the
    /// blobs its bud uses.
    #[test]
    fn bud_ring_and_rewind_round_trip() {
        require_fuse();
        let home = tempfile::tempdir().unwrap();
        let _daemon = Daemon(home.path());
        let repo = crate::git_repo();
        let (h, r) = (home.path(), repo.path());
        init(h, r);
        let v = new_fuse(h, r, "v");
        std::fs::write(v.join("a.txt"), "a\n").unwrap();
        assert!(run(h, r, &["ring", "v", "before"]).contains("as before (op "));
        assert!(run(h, r, &["ring", "v", "--list"]).starts_with("before "));
        std::fs::write(v.join("b.txt"), "b\n").unwrap();
        std::fs::write(v.join("README.md"), "changed\n").unwrap();
        let after = run(h, r, &["status", "v"]);

        // The bud sees everything the source wrote, and diverges from it.
        assert!(run(h, r, &["bud", "v", "w"]).contains("Budded graft w from v"));
        let w = std::path::PathBuf::from(run(h, r, &["path", "w"]).trim());
        for (f, c) in [
            ("a.txt", "a\n"),
            ("b.txt", "b\n"),
            ("README.md", "changed\n"),
        ] {
            assert_eq!(std::fs::read_to_string(w.join(f)).unwrap(), c, "{f}");
        }
        std::fs::write(w.join("only-w.txt"), "w\n").unwrap();
        assert!(!v.join("only-w.txt").exists());

        // Rewind restores the ring; Root and the bud are untouched.
        let out = run(h, r, &["rewind", "v", "before"]);
        let undo = out.rsplit(' ').next().unwrap().trim().to_owned();
        assert!(undo.starts_with("undo-"), "{out}");
        assert_eq!(std::fs::read_to_string(v.join("a.txt")).unwrap(), "a\n");
        assert!(!v.join("b.txt").exists());
        assert_eq!(
            std::fs::read_to_string(v.join("README.md")).unwrap(),
            "hello\n"
        );
        assert!(w.join("b.txt").exists());
        assert!(!r.join("a.txt").exists());
        assert!(run(h, r, &["log", "v"]).contains("rewind"));

        // Re-applying returns exactly to the state before the rewind.
        run(h, r, &["rewind", "v", &undo]);
        assert_eq!(run(h, r, &["status", "v"]), after);
        assert_eq!(std::fs::read_to_string(v.join("b.txt")).unwrap(), "b\n");

        // Pruning the source keeps the blobs the bud uses.
        run(h, r, &["prune", "v", "--force"]);
        assert_eq!(std::fs::read_to_string(w.join("a.txt")).unwrap(), "a\n");
        assert_eq!(std::fs::read_to_string(w.join("b.txt")).unwrap(), "b\n");
        run(h, r, &["prune", "w", "--force"]);
        let blobs = walk_files(&w.parent().unwrap().parent().unwrap().join("blobs"));
        assert_eq!(blobs, Vec::<std::path::PathBuf>::new(), "blobs left");
    }

    /// AGENTS.md Phase 9 target: bud of a graft with 1,000 changed files
    /// in < 200 ms. Reported, not asserted (a target).
    #[test]
    fn bud_time_with_a_thousand_changed_files() {
        require_fuse();
        let home = tempfile::tempdir().unwrap();
        let _daemon = Daemon(home.path());
        let repo = crate::git_repo();
        let (h, r) = (home.path(), repo.path());
        init(h, r);
        let v = new_fuse(h, r, "src");
        std::fs::create_dir(v.join("many")).unwrap();
        for i in 0..1000 {
            std::fs::write(v.join(format!("many/f{i}.txt")), format!("{i}\n")).unwrap();
        }
        let mut times = Vec::new();
        for i in 0..3 {
            let name = format!("bud-{i}");
            let start = Instant::now();
            run(h, r, &["bud", "src", &name]);
            times.push(start.elapsed());
            let b = std::path::PathBuf::from(run(h, r, &["path", &name]).trim());
            assert_eq!(
                std::fs::read_to_string(b.join("many/f999.txt")).unwrap(),
                "999\n"
            );
        }
        times.sort();
        eprintln!(
            "graft bud, 1,000 changed files ({BACKEND}): {times:?}, median {:?}",
            times[1]
        );
    }

    /// AGENTS.md Phase 10: `graft run` in a virtual graft confines the
    /// command to the mount and tags its op log rows with the run session.
    #[test]
    fn run_tags_the_op_log_and_stays_in_the_mount() {
        require_fuse();
        let home = tempfile::tempdir().unwrap();
        let _daemon = Daemon(home.path());
        let repo = crate::git_repo();
        let (h, r) = (home.path(), repo.path());
        init(h, r);
        let v = new_fuse(h, r, "v");
        let escape = r.join("escape.txt");
        let script = format!(
            "echo $GRAFT_RUN; echo run > ran.txt && ! (echo x > '{}' 2>/dev/null)",
            escape.display()
        );
        let session = run(h, r, &["run", "v", "--", "sh", "-c", &script]);
        let session = session.trim();
        assert_eq!(session.len(), 32, "{session}");
        assert_eq!(std::fs::read_to_string(v.join("ran.txt")).unwrap(), "run\n");
        assert!(!escape.exists());
        let log = run(h, r, &["log", "v"]);
        let line = log.lines().find(|l| l.contains("ran.txt")).unwrap();
        assert!(line.ends_with(&format!("[run:{session}]")), "{log}");
        // Writes after the run are not attributed to it.
        std::fs::write(v.join("later.txt"), "later\n").unwrap();
        let log = run(h, r, &["log", "v"]);
        let line = log.lines().find(|l| l.contains("later.txt")).unwrap();
        assert!(!line.contains("[run:"), "{log}");
    }

    /// AGENTS.md Phase 12: Radar reads virtual grafts' changes from their
    /// delta and finds a same-function overlap with a reflink graft.
    #[test]
    fn radar_sees_virtual_and_reflink_grafts() {
        require_fuse();
        let home = tempfile::tempdir().unwrap();
        let _daemon = Daemon(home.path());
        let repo = crate::git_repo();
        let (h, r) = (home.path(), repo.path());
        std::fs::write(
            r.join("lib.py"),
            "def f():\n    return 1\n\n\ndef g():\n    return 2\n",
        )
        .unwrap();
        crate::git(r, &["add", "lib.py"]);
        crate::git(r, &["commit", "-qm", "lib"]);
        init(h, r);
        let v = new_fuse(h, r, "v");
        run(h, r, &["new", "w", "--backend", "reflink"]);
        let w = std::path::PathBuf::from(run(h, r, &["path", "w"]).trim());
        std::fs::write(
            v.join("lib.py"),
            "def f():\n    return 10\n\n\ndef g():\n    return 2\n",
        )
        .unwrap();
        std::fs::write(
            w.join("lib.py"),
            "def f():\n    return 11\n\n\ndef g():\n    return 2\n",
        )
        .unwrap();
        let out = run(h, r, &["radar", "--json"]);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["overlaps"][0]["level"], "symbol", "{out}");
        assert_eq!(v["overlaps"][0]["grafts"], serde_json::json!(["v", "w"]));
        assert_eq!(v["overlaps"][0]["symbols"], serde_json::json!(["f"]));
    }

    /// AGENTS.md Phase 13: a virtual graft integrates from its delta into
    /// a branch of Root, as ordinary git history.
    #[test]
    fn a_virtual_graft_integrates() {
        require_fuse();
        let home = tempfile::tempdir().unwrap();
        let _daemon = Daemon(home.path());
        let repo = crate::git_repo();
        let (h, r) = (home.path(), repo.path());
        crate::git(r, &["config", "user.name", "Graft Test"]);
        crate::git(r, &["config", "user.email", "test@graft.invalid"]);
        crate::git(r, &["branch", "land"]);
        init(h, r);
        let v = new_fuse(h, r, "v");
        std::fs::write(v.join("README.md"), "from a virtual graft\n").unwrap();
        std::fs::write(v.join("added.txt"), "added\n").unwrap();
        let report = run(h, r, &["integrate", "v", "--onto", "land"]);
        assert!(report.contains("landed   v → land"), "{report}");
        let show = |spec: &str| {
            let o = Command::new("git")
                .args(["show", spec])
                .current_dir(r)
                .output()
                .unwrap();
            String::from_utf8(o.stdout).unwrap()
        };
        assert_eq!(show("land:README.md"), "from a virtual graft\n");
        assert_eq!(show("land:added.txt"), "added\n");
        let fsck = Command::new("git")
            .args(["fsck", "--strict"])
            .current_dir(r)
            .status()
            .unwrap();
        assert!(fsck.success());
        run(h, r, &["prune", "v"]);
    }

    /// AGENTS.md Phase 13: a live virtual graft sees Root's new head after
    /// `graft rebase`, keeping its own changes; replay stays exact.
    #[test]
    fn a_virtual_graft_rebases_onto_a_new_head() {
        require_fuse();
        let home = tempfile::tempdir().unwrap();
        let _daemon = Daemon(home.path());
        let repo = crate::git_repo();
        let (h, r) = (home.path(), repo.path());
        crate::git(r, &["config", "user.name", "Graft Test"]);
        crate::git(r, &["config", "user.email", "test@graft.invalid"]);
        std::fs::write(r.join("a.txt"), "a\n").unwrap();
        std::fs::write(r.join("b.txt"), "b\n").unwrap();
        crate::git(r, &["add", "-A"]);
        crate::git(r, &["commit", "-qm", "files"]);
        crate::git(r, &["branch", "land"]);
        init(h, r);
        let v = new_fuse(h, r, "v");
        std::fs::write(v.join("a.txt"), "a from v\n").unwrap();
        std::fs::create_dir(v.join("new")).unwrap();
        std::fs::write(v.join("new/v.txt"), "v\n").unwrap();
        run(h, r, &["ring", "v", "before"]);
        let git_status = |dir: &Path| {
            let o = Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(dir)
                .output()
                .unwrap();
            String::from_utf8(o.stdout).unwrap()
        };
        assert!(git_status(&v).contains("a.txt"));
        // Another graft lands a change to b.txt.
        run(
            h,
            r,
            &["new", "w", "--backend", "reflink", "--from", "land"],
        );
        let other = std::path::PathBuf::from(run(h, r, &["path", "w"]).trim());
        std::fs::write(other.join("b.txt"), "b from w\n").unwrap();
        run(h, r, &["integrate", "w", "--onto", "land"]);

        let out = run(h, r, &["rebase", "v", "--onto", "land"]);
        assert!(out.contains("Rebased graft v onto land"), "{out}");
        assert_eq!(
            std::fs::read_to_string(v.join("a.txt")).unwrap(),
            "a from v\n"
        );
        assert_eq!(std::fs::read_to_string(v.join("new/v.txt")).unwrap(), "v\n");
        assert_eq!(
            std::fs::read_to_string(v.join("b.txt")).unwrap(),
            "b from w\n"
        );
        let status = run(h, r, &["status", "v"]);
        assert!(
            status.contains("a.txt") && !status.contains("b.txt"),
            "{status}"
        );
        let gs = git_status(&v);
        assert!(gs.contains("a.txt") && !gs.contains("b.txt"), "{gs}");
        assert!(run(h, r, &["log", "v"]).contains("rebase"));
        let o = graft_d(h)
            .current_dir(r)
            .args(["rewind", "v", "before"])
            .output()
            .unwrap();
        assert!(!o.status.success());
        assert!(String::from_utf8_lossy(&o.stderr).contains("predates its last rebase"));
        // And it integrates cleanly on top.
        run(h, r, &["integrate", "v", "--onto", "land"]);
    }

    fn walk_files(dir: &Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk_files(&p));
            } else {
                out.push(p);
            }
        }
        out
    }

    fn daemon_pid(home: &Path) -> String {
        let status = graft_d(home).args(["daemon", "status"]).output().unwrap();
        let status = String::from_utf8(status.stdout).unwrap();
        status
            .split("(pid ")
            .nth(1)
            .and_then(|s| s.split(')').next())
            .unwrap()
            .to_owned()
    }

    fn kill9(pid: &str) {
        assert!(
            Command::new("kill")
                .args(["-9", pid])
                .status()
                .unwrap()
                .success()
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    /// AGENTS.md Phase 6: `kill -9` of graftd mid-write loses at most the
    /// in-flight operation, and the delta is consistent on restart.
    #[test]
    fn a_crash_mid_write_loses_at_most_the_operation_in_flight() {
        use std::io::Write as _;
        require_fuse();
        let home = tempfile::tempdir().unwrap();
        let _daemon = Daemon(home.path());
        let repo = crate::git_repo();
        init(home.path(), repo.path());
        let path = new_fuse(home.path(), repo.path(), "crash");
        // Completed operations.
        std::fs::write(path.join("done.txt"), "done\n").unwrap();
        std::fs::create_dir(path.join("dir")).unwrap();
        // A file still open for writing when the daemon dies.
        let mut open = std::fs::File::create(path.join("open.txt")).unwrap();
        open.write_all(b"written before the crash\n").unwrap();
        // The macOS NFS client may keep writes in its own cache until close
        // or fsync; fsync sends them to graftd, which has not journaled them
        // yet when it is killed. (On FUSE writes reach graftd at once, and an
        // fsync would journal the file, skipping the recovery path.)
        #[cfg(target_os = "macos")]
        open.sync_data().unwrap();
        kill9(&daemon_pid(home.path()));
        drop(open);
        graft_d(home.path())
            .current_dir(repo.path())
            .args(["status", "crash"])
            .assert()
            .success()
            .stdout(
                predicate::str::contains("A done.txt").and(predicate::str::contains("A open.txt")),
            );
        assert_eq!(
            std::fs::read_to_string(path.join("done.txt")).unwrap(),
            "done\n"
        );
        assert!(path.join("dir").is_dir());
        assert_eq!(
            std::fs::read_to_string(path.join("open.txt")).unwrap(),
            "written before the crash\n"
        );
        graft_d(home.path())
            .current_dir(repo.path())
            .args(["prune", "crash", "--force"])
            .assert()
            .success();
    }

    #[test]
    fn a_restarted_daemon_remounts_grafts() {
        require_fuse();
        let home = tempfile::tempdir().unwrap();
        let _daemon = Daemon(home.path());
        let repo = crate::git_repo();
        init(home.path(), repo.path());
        let path = new_fuse(home.path(), repo.path(), "survivor");
        std::fs::write(path.join("kept.txt"), "kept\n").unwrap();
        kill9(&daemon_pid(home.path()));
        // The mount is stale until a command restarts graftd.
        assert!(std::fs::read_to_string(path.join("README.md")).is_err());
        graft_d(home.path())
            .current_dir(repo.path())
            .arg("ls")
            .assert()
            .success()
            .stdout(predicate::str::contains("survivor"));
        assert_eq!(
            std::fs::read_to_string(path.join("README.md")).unwrap(),
            "hello\n"
        );
        assert_eq!(
            std::fs::read_to_string(path.join("kept.txt")).unwrap(),
            "kept\n"
        );
        graft_d(home.path())
            .current_dir(repo.path())
            .args(["prune", "survivor", "--force"])
            .assert()
            .success();
    }

    #[test]
    fn prune_unmounts_lazily_when_busy() {
        require_fuse();
        let home = tempfile::tempdir().unwrap();
        let _daemon = Daemon(home.path());
        let repo = crate::git_repo();
        init(home.path(), repo.path());
        let path = new_fuse(home.path(), repo.path(), "busy");
        let open = std::fs::File::open(path.join("README.md")).unwrap();
        graft_d(home.path())
            .current_dir(repo.path())
            .args(["prune", "busy"])
            .assert()
            .success()
            .stderr(predicate::str::contains("was in use, so it was unmounted"));
        assert!(!path.exists());
        drop(open);
    }

    #[test]
    fn creation_does_not_scale_with_repo_size() {
        require_fuse();
        let home = tempfile::tempdir().unwrap();
        let _daemon = Daemon(home.path());
        let repo = crate::git_repo();
        for d in 0..100 {
            let dir = repo.path().join(format!("gen/d{d}"));
            std::fs::create_dir_all(&dir).unwrap();
            for i in 0..200 {
                std::fs::write(dir.join(format!("f{i}")), format!("{d}.{i}")).unwrap();
            }
        }
        git(repo.path(), &["add", "-A"]);
        git(repo.path(), &["commit", "-q", "-m", "20k files"]);
        init(home.path(), repo.path());
        // Warm graftd so its start-up is not counted.
        new_fuse(home.path(), repo.path(), "warm");
        let started = Instant::now();
        let path = new_fuse(home.path(), repo.path(), "big");
        let took = started.elapsed();
        eprintln!("graft new --backend fuse on 20k files: {took:?}");
        assert!(took.as_secs_f64() < 1.0, "{took:?}");
        assert_eq!(
            std::fs::read_to_string(path.join("gen/d99/f199")).unwrap(),
            "99.199"
        );
    }

    #[test]
    fn layers_are_built_once_and_served() {
        require_fuse();
        let home = tempfile::tempdir().unwrap();
        let _daemon = Daemon(home.path());
        let repo = crate::git_repo();
        let p = repo.path();
        std::fs::write(
            p.join("graft.toml"),
            "[[layer]]\npath = \"deps\"\ninputs = [\"deps.lock\"]\nbuild = 'mkdir deps && cat deps.lock > deps/lock && echo run >> \"$GRAFT_HOME/builds\"'\n",
        )
        .unwrap();
        std::fs::write(p.join("deps.lock"), "v1\n").unwrap();
        std::fs::write(p.join(".gitignore"), "deps/\n").unwrap();
        git(p, &["add", "-A"]);
        git(p, &["commit", "-q", "-m", "layer"]);
        init(home.path(), p);
        graft_d(home.path())
            .current_dir(p)
            .arg("trust")
            .assert()
            .success();
        let a = new_fuse(home.path(), p, "a");
        let b = new_fuse(home.path(), p, "b");
        for g in [&a, &b] {
            assert_eq!(
                std::fs::read_to_string(g.join("deps/lock")).unwrap(),
                "v1\n"
            );
        }
        let builds = std::fs::read_to_string(home.path().join("builds")).unwrap();
        assert_eq!(builds.lines().count(), 1, "the layer is built once");
        let scratch = a.parent().unwrap().join(".layer-build");
        let left: Vec<_> = std::fs::read_dir(&scratch)
            .map(|d| d.flatten().collect())
            .unwrap_or_default();
        assert!(left.is_empty(), "build scratch left {left:?}");
        graft_d(home.path())
            .current_dir(p)
            .args(["gc"])
            .assert()
            .success();
        assert_eq!(
            std::fs::read_to_string(b.join("deps/lock")).unwrap(),
            "v1\n"
        );
    }

    /// `cargo check` and `tsc --noEmit` give the same result in a graft as
    /// in Root.
    #[test]
    fn type_checkers_agree_with_root() {
        require_fuse();
        let home = tempfile::tempdir().unwrap();
        let _daemon = Daemon(home.path());
        let repo = crate::git_repo();
        let p = repo.path();
        std::fs::create_dir_all(p.join("src")).unwrap();
        std::fs::write(
            p.join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(
            p.join("src/lib.rs"),
            "pub fn add(a: u32, b: u32) -> u32 { a + b }\npub fn unused() { let x = 1; }\n",
        )
        .unwrap();
        std::fs::write(
            p.join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            p.join("tsconfig.json"),
            "{\"compilerOptions\":{\"strict\":true,\"noEmit\":true},\"include\":[\"web\"]}\n",
        )
        .unwrap();
        std::fs::create_dir_all(p.join("web")).unwrap();
        std::fs::write(
            p.join("web/a.ts"),
            "export const n: number = 1;\nexport const s: string = n;\n",
        )
        .unwrap();
        git(p, &["add", "-A"]);
        git(p, &["commit", "-q", "-m", "toolchains"]);
        init(home.path(), p);
        let g = new_fuse(home.path(), p, "check");

        let run = |dir: &Path, cmd: &str, args: &[&str], target: &Path| {
            let out = Command::new(cmd)
                .args(args)
                .current_dir(dir)
                .env("CARGO_TARGET_DIR", target)
                .env("CARGO_TERM_COLOR", "never")
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&out.stderr).to_string()
                + &String::from_utf8_lossy(&out.stdout);
            // Paths differ between the two trees; the diagnostics must not.
            let text = text.replace(&dir.display().to_string(), "<dir>");
            let lines: Vec<_> = text
                .lines()
                .filter(|l| {
                    !l.contains("Finished") && !l.contains("Checking") && !l.contains("Compiling")
                })
                .map(str::to_owned)
                .collect();
            (out.status.success(), lines)
        };
        let t = tempfile::tempdir().unwrap();
        let ours = run(
            &g,
            "cargo",
            &["check", "--offline", "--locked"],
            &t.path().join("g"),
        );
        let theirs = run(
            p,
            "cargo",
            &["check", "--offline", "--locked"],
            &t.path().join("r"),
        );
        assert!(ours.0, "cargo check failed in the graft: {:?}", ours.1);
        assert_eq!(ours, theirs, "cargo check");
        assert!(
            ours.1.iter().any(|l| l.contains("unused variable")),
            "{:?}",
            ours.1
        );

        if Command::new("tsc").arg("--version").output().is_ok() {
            let ours = run(&g, "tsc", &["--noEmit", "-p", "."], t.path());
            let theirs = run(p, "tsc", &["--noEmit", "-p", "."], t.path());
            assert!(!ours.0, "the fixture has a type error");
            assert_eq!(ours, theirs, "tsc --noEmit");
        } else {
            assert!(
                std::env::var_os("GRAFT_TEST_REQUIRE_TSC").is_none(),
                "tsc is required"
            );
            eprintln!("tsc not installed; skipped the tsc comparison");
        }
    }
}
