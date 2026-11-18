//! `graft run`: commands confined to their graft (AGENTS.md Phase 10,
//! ADR 0014). These run on every CI platform; on a Linux kernel without
//! Landlock they check the error instead.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use predicates::prelude::*;

use crate::{git, git_repo, graft};

/// Whether this machine can confine commands. On Linux without Landlock
/// the tests check `graft run`'s error instead.
fn sandbox_usable() -> bool {
    if cfg!(target_os = "linux") {
        std::fs::read_to_string("/sys/kernel/security/lsm")
            .is_ok_and(|l| l.trim().split(',').any(|m| m == "landlock"))
    } else {
        cfg!(target_os = "macos")
    }
}

struct Fixture {
    home: tempfile::TempDir,
    repo: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let repo = git_repo();
        let f = Self { home, repo };
        f.graft().arg("init").assert().success();
        f
    }

    fn graft(&self) -> assert_cmd::Command {
        let mut cmd = graft(self.home.path());
        cmd.current_dir(self.repo.path()).env_remove("GRAFT_FS");
        cmd
    }

    fn new_graft(&self, name: &str) -> PathBuf {
        self.graft()
            .args(["new", name, "--backend", "reflink"])
            .assert()
            .success();
        let out = self.graft().args(["path", name]).output().unwrap();
        PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
    }

    fn run(&self, name: &str, script: &str) -> assert_cmd::assert::Assert {
        self.graft()
            .args(["run", name, "--", "sh", "-c", script])
            .assert()
    }
}

/// Removes `path` if a failed test let a write through.
struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn sh_quote(p: &Path) -> String {
    format!("'{}'", p.display().to_string().replace('\'', r"'\''"))
}

#[test]
fn writes_outside_the_graft_are_denied() {
    let f = Fixture::new();
    let a = f.new_graft("a");
    let b = f.new_graft("b");
    if !sandbox_usable() {
        f.run("a", "true")
            .failure()
            .stderr(predicate::str::contains("does not enforce Landlock"));
        return;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let escapes = [
        f.repo.path().join("escape.txt"),
        b.join("escape.txt"),
        home.join(format!("graft-run-escape-{}", std::process::id())),
    ];
    let _cleanup: Vec<_> = escapes.iter().cloned().map(Cleanup).collect();
    let mut script = String::from("echo in > inside.txt && echo t > \"$TMPDIR/t\"");
    for e in &escapes {
        let _ = write!(script, " && ! (echo x > {} 2>/dev/null)", sh_quote(e));
    }
    f.run("a", &script).success();
    assert_eq!(
        std::fs::read_to_string(a.join("inside.txt")).unwrap(),
        "in\n"
    );
    for e in &escapes {
        assert!(!e.exists(), "{} was written", e.display());
    }
    // Nothing reaches another graft or Root through a rename either.
    f.run(
        "a",
        &format!("! mv inside.txt {} 2>/dev/null", sh_quote(&b.join("moved"))),
    )
    .success();
    assert!(!b.join("moved").exists());
}

/// A failed run names the writes the sandbox refused (macOS logs them),
/// so a failure it caused is not mistaken for a bug; an ordinary failure
/// says nothing about the sandbox.
#[cfg(target_os = "macos")]
#[test]
fn a_failed_run_names_the_writes_the_sandbox_refused() {
    let f = Fixture::new();
    f.new_graft("a");
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let escape = home.join(format!("graft-run-denied-{}", std::process::id()));
    let _cleanup = Cleanup(escape.clone());
    let out = f
        .run("a", &format!("echo x > {}", sh_quote(&escape)))
        .failure()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8_lossy(&out);
    assert!(!escape.exists());
    if stderr.contains("the sandbox refused") {
        assert!(
            stderr.contains("the sandbox refused 1 write(s)"),
            "{stderr}"
        );
        assert!(stderr.contains(&escape.display().to_string()), "{stderr}");
        assert!(stderr.contains("--write PATH"), "{stderr}");
    } else {
        // macOS drops kernel sandbox reports from the unified log when it
        // is overloaded; then there is nothing for Graft to read. Only a
        // log that records a plain sandbox-exec denial right now makes the
        // missing list Graft's fault.
        assert!(
            !denials_are_logged(),
            "the log records denials, but graft run named none: {stderr}"
        );
        #[allow(clippy::print_stderr)] // a skipped check must say so
        {
            eprintln!("skipped: the unified log is dropping sandbox reports, so no denial list");
        }
    }
    assert!(!escape.exists());

    f.run("a", "exit 3")
        .code(3)
        .stderr(predicate::str::contains("sandbox").not());
}

/// Whether the unified log records a Seatbelt denial at the moment: a
/// tagged `sandbox-exec` denial is looked for, as `graft run` looks.
#[cfg(target_os = "macos")]
fn denials_are_logged() -> bool {
    let tag = format!("graft-test-probe-{}", std::process::id());
    let profile = format!(
        "(version 1)(allow default)(deny file-write* (with message \"{tag}\"))(allow file-write* (literal \"/dev/null\"))"
    );
    let target = std::env::temp_dir().join(&tag);
    let _ = std::process::Command::new("/usr/bin/sandbox-exec")
        .args(["-p", &profile, "/usr/bin/touch"])
        .arg(&target)
        .output();
    let _ = std::fs::remove_file(&target);
    std::thread::sleep(std::time::Duration::from_secs(1));
    std::process::Command::new("/usr/bin/log")
        .args(["show", "--style", "ndjson", "--last", "30s", "--predicate"])
        .arg(format!("eventMessage CONTAINS \"{tag}\""))
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("deny("))
}

/// `--write` and `--dev-caches` widen one run only; `$HOME` stays read-only.
#[test]
fn write_and_dev_caches_widen_one_run() {
    let f = Fixture::new();
    f.new_graft("a");
    if !sandbox_usable() {
        return;
    }
    let extra = tempfile::tempdir().unwrap();
    let target = extra.path().join("ok");
    f.graft()
        .args(["run", "a", "--write"])
        .arg(extra.path())
        .args(["--", "sh", "-c", &format!("echo y > {}", sh_quote(&target))])
        .assert()
        .success();
    assert!(target.exists());
    f.run(
        "a",
        &format!(
            "! (echo z > {} 2>/dev/null)",
            sh_quote(&extra.path().join("no"))
        ),
    )
    .success();
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let escape = home.join(format!("graft-run-devcache-{}", std::process::id()));
    let _cleanup = Cleanup(escape.clone());
    f.graft()
        .args(["run", "a", "--dev-caches", "--", "sh", "-c"])
        .arg(format!("! (echo x > {} 2>/dev/null)", sh_quote(&escape)))
        .assert()
        .success();
    assert!(!escape.exists(), "--dev-caches made $HOME writable");
}

#[test]
fn the_command_sees_its_graft_and_keeps_its_exit_code() {
    let f = Fixture::new();
    let a = f.new_graft("a");
    if !sandbox_usable() {
        return;
    }
    let out = f
        .graft()
        .args([
            "run",
            "a",
            "--",
            "sh",
            "-c",
            "pwd -P; echo $GRAFT_NAME; echo ${#GRAFT_ID}; echo ${#GRAFT_RUN}; echo $TMPDIR; exit 7",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(7));
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines[0], a.canonicalize().unwrap().to_str().unwrap());
    assert_eq!(&lines[1..4], ["a", "32", "32"]);
    let tmp = PathBuf::from(lines[4]);
    assert!(tmp.is_dir(), "{}", tmp.display());
    assert_eq!(tmp.parent().unwrap().file_name().unwrap(), "tmp");

    // Started inside a subdirectory of the graft, it runs there.
    std::fs::create_dir(a.join("sub")).unwrap();
    graft(f.home.path())
        .current_dir(a.join("sub"))
        .args(["run", "a", "--", "pwd", "-P"])
        .assert()
        .success()
        .stdout(format!(
            "{}\n",
            a.join("sub").canonicalize().unwrap().display()
        ));

    // Prune removes the private temp dir.
    f.graft().args(["prune", "a", "--force"]).assert().success();
    assert!(!tmp.exists(), "prune left {}", tmp.display());
}

#[test]
fn git_cargo_and_the_network_work_inside_the_sandbox() {
    let f = Fixture::new();
    let a = f.new_graft("a");
    if !sandbox_usable() {
        return;
    }
    std::fs::create_dir_all(a.join("t/src")).unwrap();
    std::fs::write(
        a.join("t/Cargo.toml"),
        "[package]\nname = \"t\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(
        a.join("t/src/main.rs"),
        "fn main() { println!(\"built\"); }\n",
    )
    .unwrap();
    f.run(
        "a",
        "git add -A && git -c user.email=t@t -c user.name=t commit -qm sandboxed \
         && git status --porcelain && cd t && cargo run -q --offline",
    )
    .success()
    .stdout(predicate::str::ends_with("built\n"));
    // The network is not restricted: a loopback HTTP request succeeds.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        use std::io::{Read as _, Write as _};
        let (mut s, _) = listener.accept().unwrap();
        let mut buf = [0; 1024];
        let _ = s.read(&mut buf).unwrap();
        s.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 5\r\n\r\npong\n")
            .unwrap();
    });
    f.run("a", &format!("curl -sS http://127.0.0.1:{port}/"))
        .success()
        .stdout("pong\n");
    server.join().unwrap();
    let log = std::process::Command::new("git")
        .args(["log", "--format=%s", "-1"])
        .current_dir(&a)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&log.stdout), "sandboxed\n");
}

#[test]
fn graft_toml_sandbox_paths_need_approval() {
    let f = Fixture::new();
    let cache = tempfile::tempdir().unwrap();
    let cache_path = cache.path().canonicalize().unwrap();
    std::fs::write(
        f.repo.path().join("graft.toml"),
        format!("[sandbox]\nwrite = [\"{}\"]\n", cache_path.display()),
    )
    .unwrap();
    git(f.repo.path(), &["add", "graft.toml"]);
    git(f.repo.path(), &["commit", "-qm", "sandbox paths"]);
    f.new_graft("a");
    if !sandbox_usable() {
        return;
    }
    let target = cache_path.join("written");
    let script = format!("echo c > {}", sh_quote(&target));
    f.run("a", &script)
        .failure()
        .stderr(predicate::str::contains("not been approved"))
        .stderr(predicate::str::contains(cache_path.to_str().unwrap()));
    f.graft()
        .arg("trust")
        .assert()
        .success()
        .stdout(predicate::str::contains("Approved sandbox write"));
    f.run("a", &script).success();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "c\n");

    // --write grants a path for one run only.
    let once = tempfile::tempdir().unwrap();
    let file = once.path().join("f");
    f.run(
        "a",
        &format!("! (echo x > {} 2>/dev/null)", sh_quote(&file)),
    )
    .success();
    f.graft()
        .args(["run", "a", "--write"])
        .arg(once.path())
        .args(["--", "sh", "-c", &format!("echo x > {}", sh_quote(&file))])
        .assert()
        .success();
    assert!(file.exists());
}

#[test]
fn run_refuses_unknown_and_missing_commands() {
    let f = Fixture::new();
    f.new_graft("a");
    f.graft()
        .args(["run", "nope", "--", "true"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no graft named nope"));
    if sandbox_usable() {
        // No command starts $SHELL; one that does not exist fails.
        f.graft()
            .args(["run", "a"])
            .env("SHELL", "/no/such/shell")
            .assert()
            .failure();
        f.graft()
            .args(["run", "a", "--", "/no/such/program"])
            .assert()
            .failure();
    }
}

/// `graft new <name> -- <command>` creates the graft, prints the summary,
/// then runs the command in it, confined; its exit code is graft's.
#[test]
fn new_starts_a_command_in_the_new_graft() {
    let f = Fixture::new();
    if !sandbox_usable() {
        return;
    }
    let out = f
        .graft()
        .args(["new", "b", "--backend", "reflink", "--", "sh", "-c"])
        .arg("echo \"in $GRAFT_NAME\" && touch made-here")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    let created = stdout.find("Created graft b").expect("summary printed");
    let ran = stdout.find("in b").expect("command ran");
    assert!(created < ran, "summary must come first: {stdout}");
    let path = PathBuf::from(
        String::from_utf8(f.graft().args(["path", "b"]).output().unwrap().stdout)
            .unwrap()
            .trim(),
    );
    assert!(path.join("made-here").is_file());

    // Confined, and the exit code passes through; the graft stays.
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let escape = home.join(format!("graft-new-exec-{}", std::process::id()));
    let _cleanup = Cleanup(escape.clone());
    f.graft()
        .args(["new", "c", "--backend", "reflink", "--", "sh", "-c"])
        .arg(format!(
            "echo x > {} 2>/dev/null; exit 7",
            sh_quote(&escape)
        ))
        .assert()
        .code(7);
    assert!(!escape.exists(), "the command ran unconfined");
    f.graft()
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("c "));
}

/// `graft run <name>` with no command starts `$SHELL` in the graft.
#[test]
fn run_without_a_command_starts_a_shell_in_the_graft() {
    let f = Fixture::new();
    let a = f.new_graft("a");
    if !sandbox_usable() {
        return;
    }
    f.graft()
        .args(["run", "a"])
        .env("SHELL", "/bin/sh")
        .write_stdin("echo \"$GRAFT_NAME\" > who\n")
        .assert()
        .success();
    assert_eq!(std::fs::read_to_string(a.join("who")).unwrap(), "a\n");
}
