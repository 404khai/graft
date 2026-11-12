//! `graft mcp` (AGENTS.md Phase 15, ADR 0019), driven by a minimal MCP
//! client over stdio: JSON-RPC lines, no SDK, so the test sees exactly
//! what any client would.

// The helpers take `json!` values by value: call sites read as requests.
#![allow(clippy::needless_pass_by_value)]

use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{git, git_repo};

struct Client {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Value>,
    next: u64,
    /// Notifications seen so far.
    notes: Vec<Value>,
}

impl Client {
    fn start(home: &std::path::Path, root: &std::path::Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_graft"))
            .arg("mcp")
            .current_dir(root)
            .env("GRAFT_HOME", home)
            .env_remove("GRAFT_FS")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Ok(v) = serde_json::from_str::<Value>(&line)
                    && tx.send(v).is_err()
                {
                    break;
                }
            }
        });
        let mut c = Self {
            child,
            stdin,
            lines: rx,
            next: 1,
            notes: Vec::new(),
        };
        let init = c.request(
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "graft-e2e", "version": "1"}
            }),
        );
        assert_eq!(init["serverInfo"]["name"], "graft", "{init}");
        assert_eq!(
            init["capabilities"]["resources"]["subscribe"], true,
            "{init}"
        );
        c.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        c
    }

    fn send(&mut self, v: &Value) {
        writeln!(self.stdin, "{v}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Sends a request and waits for its response, keeping notifications.
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let v = self
                .lines
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|_| panic!("no response to {method}"));
            if v.get("id") == Some(&json!(id)) {
                assert!(v.get("error").is_none(), "{method}: {v}");
                return v["result"].clone();
            }
            if v.get("method").is_some() {
                self.notes.push(v);
            }
        }
    }

    /// Calls a tool: its text and whether it is an error.
    fn call(&mut self, tool: &str, args: Value) -> (String, bool) {
        let r = self.request("tools/call", json!({"name": tool, "arguments": args}));
        let text = r["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        (text, r["isError"] == true)
    }

    fn ok(&mut self, tool: &str, args: Value) -> String {
        let (text, err) = self.call(tool, args.clone());
        assert!(!err, "{tool} {args}: {text}");
        text
    }

    /// Waits for a notification matching `pred`.
    fn wait_note(&mut self, pred: impl Fn(&Value) -> bool) -> Value {
        if let Some(n) = self.notes.iter().find(|n| pred(n)) {
            return n.clone();
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let v = self
                .lines
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|_| panic!("no matching notification in 30 s"));
            if pred(&v) {
                return v;
            }
            if v.get("method").is_some() {
                self.notes.push(v);
            }
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

const LIB: &str = "def f():\n    return 1\n\n\ndef g():\n    return 2\n";

/// AGENTS.md Phase 15: an MCP client can run the full cycle: plan → new →
/// run → radar → integrate → prune, and destructive tools refuse without
/// confirmation.
#[test]
#[allow(clippy::too_many_lines)] // one scenario, step by step
fn an_agent_runs_the_whole_cycle_over_mcp() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    let r = repo.path();
    git(r, &["config", "user.name", "Graft Test"]);
    git(r, &["config", "user.email", "test@graft.invalid"]);
    std::fs::write(r.join("lib.py"), LIB).unwrap();
    git(r, &["add", "lib.py"]);
    git(r, &["commit", "-qm", "lib"]);
    git(r, &["branch", "land"]);
    let init = Command::new(env!("CARGO_BIN_EXE_graft"))
        .arg("init")
        .current_dir(r)
        .env("GRAFT_HOME", home.path())
        .status()
        .unwrap();
    assert!(init.success());
    let mut c = Client::start(home.path(), r);

    let tools = c.request("tools/list", json!({}));
    let names: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "graft_new",
            "graft_list",
            "graft_status",
            "graft_diff",
            "graft_run",
            "graft_bud",
            "graft_ring",
            "graft_radar",
            "graft_integrate",
            "graft_plan",
            "graft_prune"
        ]
    );

    // plan
    let plan: Value =
        serde_json::from_str(&c.ok("graft_plan", json!({"task": "Change `lib.py`."}))).unwrap();
    assert_eq!(plan["nodes"][0]["claims"][0]["path"], "lib.py", "{plan}");

    // new, run
    for g in ["a", "b"] {
        assert!(
            c.ok("graft_new", json!({"name": g, "backend": "reflink"}))
                .contains(&format!("Created graft {g}"))
        );
    }
    for (g, v) in [("a", "10"), ("b", "11")] {
        c.ok(
            "graft_run",
            json!({"name": g, "command": ["sh", "-c", format!("sed -i.bak 's/return 1$/return {v}/' lib.py && rm lib.py.bak")]}),
        );
    }
    assert!(
        c.ok("graft_diff", json!({"name": "a"}))
            .contains("+    return 10")
    );
    // graft_run cannot write outside the graft.
    let escape = r.join("escaped.txt");
    let (_, err) = c.call(
        "graft_run",
        json!({"name": "a", "command": ["sh", "-c", format!("echo x > '{}'", escape.display())]}),
    );
    assert!(err && !escape.exists(), "graft_run wrote outside its graft");

    // radar: as a tool, as a resource, and as a subscription.
    let radar: Value = serde_json::from_str(&c.ok("graft_radar", json!({"graft": "a"}))).unwrap();
    assert_eq!(radar["overlaps"][0]["level"], "symbol", "{radar}");
    assert_eq!(radar["overlaps"][0]["symbols"], json!(["f"]));
    let read = c.request("resources/read", json!({"uri": "graft://radar/a"}));
    assert!(
        read["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("\"symbol\""),
        "{read}"
    );
    c.request("resources/subscribe", json!({"uri": "graft://radar/a"}));
    let note = c.wait_note(|n| n["method"] == "notifications/resources/updated");
    assert_eq!(note["params"]["uri"], "graft://radar/a", "{note}");

    // integrate: refused without confirmation, then lands.
    let (text, err) = c.call("graft_integrate", json!({"names": ["a"], "onto": "land"}));
    assert!(err && text.contains("confirm: true"), "{text}");
    let before = Command::new("git")
        .args(["rev-parse", "land"])
        .current_dir(r)
        .output()
        .unwrap()
        .stdout;
    assert!(
        c.ok(
            "graft_integrate",
            json!({"names": ["a"], "onto": "land", "dry_run": true})
        )
        .contains("checked")
    );
    let after = Command::new("git")
        .args(["rev-parse", "land"])
        .current_dir(r)
        .output()
        .unwrap()
        .stdout;
    assert_eq!(before, after, "a dry run moved land");
    assert!(
        c.ok(
            "graft_integrate",
            json!({"names": ["a"], "onto": "land", "confirm": true})
        )
        .contains("landed")
    );
    let landed = Command::new("git")
        .args(["show", "land:lib.py"])
        .current_dir(r)
        .output()
        .unwrap()
        .stdout;
    assert!(String::from_utf8_lossy(&landed).contains("return 10"));

    // prune: b has unintegrated work, so force needs confirmation.
    let (text, err) = c.call("graft_prune", json!({"name": "b"}));
    assert!(
        err && text.contains("work that pruning would lose"),
        "{text}"
    );
    let (text, err) = c.call("graft_prune", json!({"name": "b", "force": true}));
    assert!(err && text.contains("confirm: true"), "{text}");
    c.ok(
        "graft_prune",
        json!({"name": "b", "force": true, "confirm": true}),
    );
    c.ok("graft_prune", json!({"name": "a"}));
    assert!(c.ok("graft_list", json!({})).contains("No grafts"));
}

#[test]
fn bad_calls_are_tool_errors() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    let init = Command::new(env!("CARGO_BIN_EXE_graft"))
        .arg("init")
        .current_dir(repo.path())
        .env("GRAFT_HOME", home.path())
        .status()
        .unwrap();
    assert!(init.success());
    let mut c = Client::start(home.path(), repo.path());
    for (tool, args, needle) in [
        (
            "graft_new",
            json!({"name": "--force"}),
            "must not start with '-'",
        ),
        (
            "graft_run",
            json!({"name": "x", "command": "rm -rf /"}),
            "array of strings",
        ),
        ("graft_shell", json!({}), "no tool graft_shell"),
        (
            "graft_diff",
            json!({"name": "missing"}),
            "no graft named missing",
        ),
    ] {
        let (text, err) = c.call(tool, args);
        assert!(err && text.contains(needle), "{tool}: {text}");
    }
}

#[test]
fn root_can_be_given_explicitly() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    let init = Command::new(env!("CARGO_BIN_EXE_graft"))
        .arg("init")
        .current_dir(repo.path())
        .env("GRAFT_HOME", home.path())
        .status()
        .unwrap();
    assert!(init.success());
    let elsewhere = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_graft"))
        .args(["mcp", "--root"])
        .arg(repo.path())
        .current_dir(elsewhere.path())
        .env("GRAFT_HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}})
    )
    .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let _ = child.kill();
    let _ = child.wait();
    assert!(line.contains("\"serverInfo\""), "{line}");
}

#[test]
fn mcp_refuses_to_start_outside_a_root() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_graft"))
        .arg("mcp")
        .current_dir(dir.path())
        .env("GRAFT_HOME", home.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("graft"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
