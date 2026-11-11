//! `graft plan`, `graft claim` and `graft claims` (AGENTS.md Phase 14,
//! ADR 0018), on the three-module fixture in `tests/fixtures/tasks/`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{git, graft};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/tasks")
}

struct Fixture {
    home: tempfile::TempDir,
    repo: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let ok = Command::new("sh")
            .arg(fixtures().join("three-modules.sh"))
            .arg(repo.path())
            .status()
            .unwrap();
        assert!(ok.success());
        git(repo.path(), &["init", "-q", "-b", "main"]);
        git(repo.path(), &["config", "user.name", "Graft Test"]);
        git(repo.path(), &["config", "user.email", "test@graft.invalid"]);
        git(repo.path(), &["add", "-A"]);
        git(repo.path(), &["commit", "-qm", "three modules"]);
        let f = Self { home, repo };
        f.ok(&["init"]);
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

    fn fails(&self, args: &[&str]) -> String {
        let o = self.cmd().args(args).output().unwrap();
        assert!(!o.status.success(), "graft {args:?} succeeded");
        String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
    }

    fn path(&self, name: &str) -> PathBuf {
        PathBuf::from(self.ok(&["path", name]).trim())
    }
}

/// The fixture task's path.
fn task() -> String {
    fixtures().join("three-modules.md").display().to_string()
}

fn edit(file: &Path, from: &str, to: &str) {
    let s = std::fs::read_to_string(file).unwrap();
    assert!(s.contains(from), "{from:?} not in {}", file.display());
    std::fs::write(file, s.replacen(from, to, 1)).unwrap();
}

#[test]
fn plans_the_fixture_with_the_contract_first() {
    let f = Fixture::new();
    let task = task();
    let text = f.ok(&["plan", &task]);
    let expected = "4 node(s), width 3:\n\
1. contract (contract)\n     app/types.py\n\
2. billing after contract\n     billing/invoice.py\n     billing/tax.py\n\
3. search after contract\n     search/index.py\n     search/query.py\n\
4. users after contract\n     users/auth.py\n     users/models.py\n";
    assert!(text.ends_with(expected), "{text}");
    // Deterministic, byte for byte.
    let json = f.ok(&["plan", &task, "--json"]);
    for _ in 0..3 {
        assert_eq!(f.ok(&["plan", &task, "--json"]), json);
    }
}

#[test]
fn apply_creates_ready_nodes_and_claims_enforce_them() {
    let f = Fixture::new();
    let task = task();
    // Only the contract is ready.
    let applied = f.ok(&["plan", &task, "--apply"]);
    assert!(
        applied.contains("created  contract (1 claim(s))"),
        "{applied}"
    );
    assert!(
        applied.contains("waiting  billing (after contract)"),
        "{applied}"
    );
    assert!(
        f.ok(&["claims"])
            .contains("contract             app/types.py")
    );
    let contract = f.path("contract");
    edit(
        &contract.join("app/types.py"),
        "self.cents = cents",
        "self.cents = cents\n        self.currency = \"EUR\"",
    );
    f.ok(&[
        "integrate",
        "contract",
        "--onto",
        "main",
        "--update-checkout",
    ]);

    // The contract landed: the three module nodes start, at most width 3.
    let applied = f.ok(&["plan", &task, "--apply"]);
    for n in ["billing", "search", "users"] {
        assert!(
            applied.contains(&format!("created  {n} (2 claim(s))")),
            "{applied}"
        );
    }
    assert!(
        f.ok(&["plan", &task, "--apply"])
            .lines()
            .all(|l| !l.starts_with("created"))
    );

    // A graft outside the plan cannot take billing's symbols...
    f.ok(&["new", "rogue", "--backend", "reflink"]);
    let refused = f.fails(&["claim", "rogue", "billing/invoice.py::total"]);
    assert!(
        refused.contains("rejected: billing/invoice.py is claimed by graft billing"),
        "{refused}"
    );
    let rogue = f.path("rogue");
    edit(
        &rogue.join("billing/invoice.py"),
        "cents * rate()",
        "cents * rate() * 2",
    );
    let o = f
        .cmd()
        .args(["integrate", "rogue", "--onto", "main", "--update-checkout"])
        .output()
        .unwrap();
    assert!(!o.status.success());
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(
        stdout.contains(
            "billing/invoice.py: rejected: billing/invoice.py is claimed by graft billing"
        ),
        "{stdout}"
    );
    // ...but may claim and change what nobody holds.
    f.ok(&["claim", "rogue", "README.md"]);
    let claims = f.ok(&["claims"]);
    assert!(
        claims.contains("Refused writes:") && claims.contains("rogue"),
        "{claims}"
    );
}

#[test]
fn a_later_node_is_queued_behind_an_earlier_one() {
    let f = Fixture::new();
    let task = task();
    f.ok(&["plan", &task, "--apply"]);
    let contract = f.path("contract");
    edit(
        &contract.join("app/types.py"),
        "self.text = text",
        "self.text = text.strip()",
    );
    f.ok(&[
        "integrate",
        "contract",
        "--onto",
        "main",
        "--update-checkout",
    ]);
    f.ok(&["plan", &task, "--apply"]);
    // users (node 4) wants a symbol billing (node 2) holds: queued.
    let queued = f.fails(&["claim", "users", "billing/tax.py::rate"]);
    assert!(
        queued.contains("queued: billing/tax.py is claimed by graft billing, which lands first"),
        "{queued}"
    );
    // Once billing lands, its claims no longer count.
    let billing = f.path("billing");
    edit(&billing.join("billing/tax.py"), "return 0.2", "return 0.25");
    f.ok(&[
        "integrate",
        "billing",
        "--onto",
        "main",
        "--update-checkout",
    ]);
    f.ok(&["claim", "users", "billing/tax.py::rate"]);
}
