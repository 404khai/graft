//! Virtual (FUSE) grafts and graftd, driven through the `graft` binary.
//!
//! Tests that mount need Linux, `--features fuse-tests` and
//! `GRAFT_TEST_FUSE=1`. The rest run everywhere.

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
        .env("GRAFT_FS", "nfs")
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
        .stderr(predicate::str::contains("need Linux"));
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

#[cfg(all(feature = "fuse-tests", target_os = "linux"))]
#[allow(clippy::print_stderr)] // Timings and skipped comparisons belong in CI logs.
mod fuse {
    use std::path::Path;
    use std::process::Command;
    use std::time::Instant;

    use predicates::prelude::*;

    use super::{Daemon, graft_d};
    use crate::git;

    fn require_fuse() {
        assert_eq!(
            std::env::var("GRAFT_TEST_FUSE").as_deref(),
            Ok("1"),
            "fuse-tests needs GRAFT_TEST_FUSE=1 and a usable /dev/fuse"
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
            .args(["new", name, "--backend", "fuse"])
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
        let err = std::fs::write(path.join("README.md"), "x").unwrap_err();
        assert_eq!(err.raw_os_error(), Some(30), "EROFS");

        graft_d(home.path())
            .current_dir(repo.path())
            .arg("ls")
            .assert()
            .success()
            .stdout(predicate::str::is_match(r"v\s+[0-9a-f]{7}\s+live\s+0\s+0 B").unwrap());
        graft_d(home.path())
            .current_dir(repo.path())
            .args(["status", "v"])
            .assert()
            .success()
            .stdout(predicate::str::contains("nothing changed"));
        graft_d(home.path())
            .current_dir(repo.path())
            .args(["diff", "v"])
            .assert()
            .success()
            .stdout("");
        graft_d(home.path())
            .current_dir(repo.path())
            .args(["prune", "v"])
            .assert()
            .success()
            .stdout("Pruned graft v\n");
        assert!(!path.exists());
        let mounts = path.parent().unwrap();
        let left: Vec<_> = std::fs::read_dir(mounts).unwrap().flatten().collect();
        assert!(left.is_empty(), "prune left {left:?}");
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

    #[test]
    fn a_restarted_daemon_remounts_grafts() {
        require_fuse();
        let home = tempfile::tempdir().unwrap();
        let _daemon = Daemon(home.path());
        let repo = crate::git_repo();
        init(home.path(), repo.path());
        let path = new_fuse(home.path(), repo.path(), "survivor");
        let status = graft_d(home.path())
            .args(["daemon", "status"])
            .output()
            .unwrap();
        let status = String::from_utf8(status.stdout).unwrap();
        let pid = status
            .split("(pid ")
            .nth(1)
            .and_then(|s| s.split(')').next())
            .unwrap()
            .to_owned();
        assert!(
            Command::new("kill")
                .args(["-9", &pid])
                .status()
                .unwrap()
                .success()
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
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
        graft_d(home.path())
            .current_dir(repo.path())
            .args(["prune", "survivor"])
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
            .stderr(predicate::str::contains("unmounted lazily"));
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
