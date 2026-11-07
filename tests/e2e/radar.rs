//! `graft radar`: overlapping work across live grafts (AGENTS.md Phase 12,
//! ADR 0016), on reflink grafts (the default backend).

// Timings belong in CI logs.
#![allow(clippy::print_stderr)]

use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::{git, git_repo, graft};

const LIB: &str = "pub fn one() -> u32 {\n    1\n}\n\npub fn two() -> u32 {\n    2\n}\n\npub fn three() -> u32 {\n    3\n}\n";

struct Fixture {
    home: tempfile::TempDir,
    repo: tempfile::TempDir,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        let home = tempfile::tempdir().unwrap();
        let repo = git_repo();
        for (p, c) in files {
            std::fs::write(repo.path().join(p), c).unwrap();
        }
        git(repo.path(), &["add", "-A"]);
        git(repo.path(), &["commit", "-qm", "fixture"]);
        let f = Self { home, repo };
        f.ok(&["init"]);
        f
    }

    fn cmd(&self) -> assert_cmd::Command {
        let mut c = graft(self.home.path());
        c.current_dir(self.repo.path()).env_remove("GRAFT_FS");
        c
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.cmd().args(args).output().unwrap();
        assert!(
            out.status.success(),
            "graft {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn new_graft(&self, name: &str) -> PathBuf {
        self.ok(&["new", name, "--backend", "reflink"]);
        PathBuf::from(self.ok(&["path", name]).trim())
    }

    fn json(&self, args: &[&str]) -> serde_json::Value {
        let mut a = vec!["radar", "--json"];
        a.extend_from_slice(args);
        serde_json::from_str(&self.ok(&a)).unwrap()
    }
}

fn edit(dir: &Path, file: &str, from: &str, to: &str) {
    let p = dir.join(file);
    let s = std::fs::read_to_string(&p).unwrap();
    assert!(s.contains(from), "{from:?} not in {}", p.display());
    std::fs::write(&p, s.replacen(from, to, 1)).unwrap();
}

fn levels(v: &serde_json::Value) -> Vec<(String, String, String)> {
    v["overlaps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                o["level"].as_str().unwrap().to_owned(),
                format!(
                    "{}+{}",
                    o["grafts"][0].as_str().unwrap(),
                    o["grafts"][1].as_str().unwrap()
                ),
                o["symbols"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| s.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join(","),
            )
        })
        .collect()
}

#[test]
fn same_function_is_symbol_level_and_different_functions_path_level() {
    let f = Fixture::new(&[("lib.rs", LIB), ("README.md", "hello\n")]);
    let a = f.new_graft("a");
    let b = f.new_graft("b");
    let c = f.new_graft("c");
    assert_eq!(f.ok(&["radar"]), "No overlaps between live grafts.\n");
    edit(&a, "lib.rs", "    1\n", "    10\n");
    edit(&b, "lib.rs", "    1\n", "    11\n");
    edit(&c, "lib.rs", "    3\n", "    30\n");
    std::fs::write(c.join("README.md"), "changed\n").unwrap();
    let found = levels(&f.json(&[]));
    assert_eq!(
        found,
        [
            ("symbol".to_owned(), "a+b".to_owned(), "one".to_owned()),
            ("path".to_owned(), "a+c".to_owned(), String::new()),
            ("path".to_owned(), "b+c".to_owned(), String::new()),
        ]
    );
    let table = f.ok(&["radar"]);
    assert!(table.starts_with("LEVEL"), "{table}");
    assert!(
        table.contains("note: behaviour checks are off: graft.toml has no [radar] test"),
        "{table}"
    );
}

/// AGENTS.md Phase 12 target: a same-function overlap is reported at
/// symbol level within 5 s of the second write, by `graft radar --watch`.
#[test]
fn watch_reports_a_symbol_overlap_within_five_seconds() {
    let f = Fixture::new(&[("lib.rs", LIB)]);
    let a = f.new_graft("a");
    let b = f.new_graft("b");
    edit(&a, "lib.rs", "    2\n", "    20\n");
    let mut watch = Command::new(env!("CARGO_BIN_EXE_graft"))
        .current_dir(f.repo.path())
        .env("GRAFT_HOME", f.home.path())
        .env_remove("GRAFT_FS")
        .args(["radar", "--watch", "--json", "--interval", "1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = watch.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send((Instant::now(), line)).is_err() {
                break;
            }
        }
    });
    // Let the watcher take its first scan, then make the second write.
    std::thread::sleep(Duration::from_millis(1500));
    let written = Instant::now();
    edit(&b, "lib.rs", "    2\n", "    21\n");
    let seen = loop {
        let (at, line) = rx
            .recv_timeout(Duration::from_secs(30))
            .expect("no overlap reported in 30 s");
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        if v["event"] == "added" && v["overlap"]["level"] == "symbol" {
            assert_eq!(v["overlap"]["symbols"], serde_json::json!(["two"]));
            break at;
        }
    };
    let _ = watch.kill();
    let _ = watch.wait();
    let latency = seen.duration_since(written);
    eprintln!("radar --watch: symbol overlap reported {latency:?} after the second write");
    assert!(latency < Duration::from_secs(5), "took {latency:?}");
}

const CALC: &str = "def rate():\n    return 2\n\n\ndef total(x):\n    return x * rate()\n\n\ndef tax(x):\n    return x // 10\n";
const RADAR_TOML: &str =
    "[radar]\ntest = \"for t in test_*.py; do python3 $t || exit 1; done\"\ntimeout_secs = 120\n";

/// AGENTS.md Phase 12: a seeded behavioural conflict (both pass alone,
/// fail together) is detected by a speculative merge.
#[test]
fn a_seeded_behavioural_conflict_is_detected() {
    let f = Fixture::new(&[
        ("calc.py", CALC),
        ("test_calc.py", "import calc\nassert calc.total(5) == 10\n"),
        ("graft.toml", RADAR_TOML),
    ]);
    let a = f.new_graft("a");
    let b = f.new_graft("b");
    // a renames rate() and updates its caller; b adds a function calling
    // rate(). Each passes alone; calc.py merges cleanly; merged, b's test
    // fails.
    edit(&a, "calc.py", "def rate():", "def unit_rate():");
    edit(&a, "calc.py", "x * rate()", "x * unit_rate()");
    std::fs::write(
        b.join("calc.py"),
        format!("{CALC}\n\ndef discount(x):\n    return total(x) - rate()\n"),
    )
    .unwrap();
    std::fs::write(
        b.join("test_discount.py"),
        "import calc\nassert calc.discount(5) == 8\n",
    )
    .unwrap();

    // The command needs approval before it runs.
    let v = f.json(&["--check"]);
    assert_eq!(levels(&v)[0].0, "path");
    assert!(
        v["notes"][0].as_str().unwrap().contains("not approved"),
        "{v}"
    );
    f.ok(&["trust"]);

    let started = Instant::now();
    let v = f.json(&["--check"]);
    eprintln!(
        "radar --check with three speculative runs took {:?}",
        started.elapsed()
    );
    let top = &v["overlaps"][0];
    assert_eq!(top["level"], "behaviour", "{v}");
    assert_eq!(top["grafts"], serde_json::json!(["a", "b"]));
    assert!(top["detail"].as_str().unwrap().contains("NameError"), "{v}");
    // Cached: plain radar shows it without re-running.
    assert_eq!(f.json(&[])["overlaps"][0]["level"], "behaviour");
    // Fixing b (calling the new name) clears it.
    edit(&b, "calc.py", "total(x) - rate()", "total(x) - 2");
    let v = f.json(&["--check"]);
    assert!(levels(&v).iter().all(|l| l.0 == "path"), "{v}");
    // Scratch checkouts are gone.
    let radar_dir = std::fs::read_dir(f.home.path().join("roots"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("radar");
    let left: Vec<_> = std::fs::read_dir(&radar_dir)
        .into_iter()
        .flatten()
        .flatten()
        .collect();
    assert!(left.is_empty(), "radar left {left:?}");
}

#[test]
fn textual_conflicts_are_reported_without_running_tests() {
    let f = Fixture::new(&[("calc.py", CALC), ("graft.toml", RADAR_TOML)]);
    let a = f.new_graft("a");
    let b = f.new_graft("b");
    edit(&a, "calc.py", "x // 10", "x // 5");
    edit(&b, "calc.py", "x // 10", "x // 20");
    f.ok(&["trust"]);
    let v = f.json(&["--check"]);
    assert_eq!(
        levels(&v)[..2],
        [
            ("conflict".to_owned(), "a+b".to_owned(), String::new()),
            ("symbol".to_owned(), "a+b".to_owned(), "tax".to_owned()),
        ]
    );
}

/// AGENTS.md Phase 12 target: Radar adds < 5% to write latency. Radar never
/// runs on the write path; this measures writes in a graft with and without
/// `graft radar --watch` scanning every 100 ms next to them. Reported, not
/// asserted (a target, and the machine's load dominates).
#[test]
fn write_latency_with_radar_watching() {
    let f = Fixture::new(&[("lib.rs", LIB)]);
    let a = f.new_graft("a");
    let b = f.new_graft("b");
    edit(&b, "lib.rs", "    1\n", "    2\n");
    let write_many = |round: usize| {
        let start = Instant::now();
        for i in 0..2000 {
            std::fs::write(a.join(format!("f{round}-{i}.txt")), b"x\n").unwrap();
        }
        start.elapsed()
    };
    for (k, interval) in ["1", "0.1"].into_iter().enumerate() {
        let mut without = Vec::new();
        let mut with = Vec::new();
        for round in 0..3 {
            without.push(write_many(k * 10 + round * 2));
            let mut watch = Command::new(env!("CARGO_BIN_EXE_graft"))
                .current_dir(f.repo.path())
                .env("GRAFT_HOME", f.home.path())
                .env_remove("GRAFT_FS")
                .args(["radar", "--watch", "--interval", interval])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            std::thread::sleep(Duration::from_millis(300));
            with.push(write_many(k * 10 + round * 2 + 1));
            let _ = watch.kill();
            let _ = watch.wait();
        }
        without.sort();
        with.sort();
        let ratio = with[1].as_secs_f64() / without[1].as_secs_f64();
        eprintln!(
            "2,000 small writes in a reflink graft: median {:?} alone, {:?} with radar --watch --interval {interval} (x{ratio:.3})",
            without[1], with[1]
        );
    }
}
