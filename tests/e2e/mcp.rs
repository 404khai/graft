//! `graft mcp` (AGENTS.md Phase 15, ADR 0019), driven by a minimal MCP
//! client over stdio: JSON-RPC lines, no SDK, so the test sees exactly
//! what any client would.

// The helpers take `json!` values by value: call sites read as requests.
#![allow(clippy::needless_pass_by_value)]

use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
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
        // The instructions say which Root and version this server is.
        let instructions = init["instructions"].as_str().unwrap();
        let real = root.canonicalize().unwrap();
        assert!(
            instructions.contains(&format!("the Root at {}", real.display()))
                || instructions.contains(&format!("the Root at {}", root.display())),
            "{instructions}"
        );
        assert!(
            instructions.contains(&format!("(graft {})", env!("CARGO_PKG_VERSION"))),
            "{instructions}"
        );
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
    let mut c = Client::start(home.path(), r);

    // init: an unregistered repository says how to register it.
    let e = error_of(&mut c, "graft_list", json!({}));
    assert_eq!(e["code"], "not_registered", "{e}");
    assert_eq!(e["hint"], "call graft_init", "{e}");
    let init: Value = serde_json::from_str(&c.ok("graft_init", json!({}))).unwrap();
    assert_eq!(init["registered"], true, "{init}");
    let again: Value = serde_json::from_str(&c.ok("graft_init", json!({}))).unwrap();
    assert_eq!(again["registered"], false, "{again}");
    assert_eq!(again["root"], init["root"]);

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
            "graft_init",
            "graft_new",
            "graft_list",
            "graft_path",
            "graft_doctor",
            "graft_fsck",
            "graft_status",
            "graft_diff",
            "graft_run",
            "graft_bud",
            "graft_ring",
            "graft_rewind",
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
        let made: Value =
            serde_json::from_str(&c.ok("graft_new", json!({"name": g, "backend": "reflink"})))
                .unwrap();
        assert_eq!(made["name"], g, "{made}");
        assert!(
            made["path"]
                .as_str()
                .unwrap()
                .ends_with(&format!("/mounts/{g}")),
            "{made}"
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
    let left: Value = serde_json::from_str(&c.ok("graft_list", json!({}))).unwrap();
    assert_eq!(left["grafts"], json!([]), "grafts left behind");
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
fn mcp_refuses_to_start_outside_a_git_repository() {
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

/// The error object of a failed call (ADR 0025).
fn error_of(c: &mut Client, tool: &str, args: Value) -> Value {
    let (text, err) = c.call(tool, args);
    assert!(err, "{tool} succeeded: {text}");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{tool}: not JSON ({e}): {text}"))
}

/// Failed calls carry a stable code and a hint that names MCP tools, not
/// CLI commands; list and status answer in JSON (ADR 0025).
#[test]
#[allow(clippy::too_many_lines)] // one check per code, read top to bottom
fn failures_have_codes_and_mcp_hints() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    let r = repo.path();
    git(r, &["config", "user.name", "Graft Test"]);
    git(r, &["config", "user.email", "test@graft.invalid"]);
    let init = Command::new(env!("CARGO_BIN_EXE_graft"))
        .arg("init")
        .current_dir(r)
        .env("GRAFT_HOME", home.path())
        .status()
        .unwrap();
    assert!(init.success());
    let mut c = Client::start(home.path(), r);

    // Empty lists are JSON too.
    assert_eq!(
        serde_json::from_str::<Value>(&c.ok("graft_list", json!({}))).unwrap()["grafts"],
        json!([])
    );
    assert_eq!(
        serde_json::from_str::<Value>(&c.ok("graft_status", json!({}))).unwrap()["grafts"],
        json!([])
    );

    let e = error_of(&mut c, "graft_status", json!({"name": "nosuch"}));
    assert_eq!(e["code"], "not_found", "{e}");
    assert!(
        e["message"]
            .as_str()
            .unwrap()
            .contains("no graft named nosuch"),
        "{e}"
    );
    assert_eq!(e["hint"], "see graft_list", "{e}");
    assert!(!e.to_string().contains("graft ls"), "{e}");

    let e = error_of(&mut c, "graft_new", json!({"name": "Bad_Name"}));
    assert_eq!(e["code"], "invalid_name", "{e}");

    let e = error_of(&mut c, "graft_integrate", json!({"names": ["a"]}));
    assert_eq!(e["code"], "confirm_required", "{e}");
    let e = error_of(&mut c, "graft_list", json!({"all": true}));
    assert_eq!(e["code"], "invalid_arguments", "{e}");

    // A command that fails inside graft_run is `failed`, with its output.
    let made: Value =
        serde_json::from_str(&c.ok("graft_new", json!({"name": "a", "backend": "reflink"})))
            .unwrap();
    assert_eq!(made["backend"], "reflink", "{made}");
    assert_eq!(made["layers"], json!([]), "{made}");
    // A command's exit code and its two streams, apart.
    let e = error_of(
        &mut c,
        "graft_run",
        json!({"name": "a", "command": ["sh", "-c", "echo out; echo err >&2; exit 3"]}),
    );
    assert_eq!(e["code"], "command_failed", "{e}");
    assert_eq!(e["exit_code"], 3, "{e}");
    assert_eq!(e["signal"], Value::Null, "{e}");
    assert_eq!(e["stdout"], "out\n", "{e}");
    assert_eq!(e["stderr"], "err", "{e}");
    assert_eq!(e["timed_out"], false, "{e}");
    assert!(e["duration_ms"].is_u64(), "{e}");
    let done: Value = serde_json::from_str(&c.ok(
        "graft_run",
        json!({"name": "a", "command": ["sh", "-c", "echo fine"]}),
    ))
    .unwrap();
    assert_eq!(done["exit_code"], 0, "{done}");
    assert_eq!(done["stdout"], "fine\n", "{done}");
    let killed = error_of(
        &mut c,
        "graft_run",
        json!({"name": "a", "command": ["sh", "-c", "kill -9 $$"]}),
    );
    assert_eq!(killed["code"], "command_failed", "{killed}");
    assert_eq!(killed["exit_code"], Value::Null, "{killed}");
    assert_eq!(killed["signal"], 9, "{killed}");

    // list and status describe the graft.
    let listing: Value = serde_json::from_str(&c.ok("graft_list", json!({}))).unwrap();
    assert!(
        listing["root"]
            .as_str()
            .is_some_and(|p| Path::new(p) == r.canonicalize().unwrap() || Path::new(p) == r),
        "{listing}"
    );
    assert_eq!(listing["version"], env!("CARGO_PKG_VERSION"), "{listing}");
    let list = &listing["grafts"];
    assert_eq!(list[0]["name"], "a", "{list}");
    assert_eq!(list[0]["backend"], "reflink", "{list}");
    assert_eq!(list[0]["changed"], 0, "{list}");
    // Disk use is opt-in: walking a large build tree takes seconds.
    assert!(list[0]["disk_bytes"].is_null(), "{list}");
    assert_eq!(list[0]["note"], "disk not measured", "{list}");
    let measured: Value = serde_json::from_str(&c.ok("graft_list", json!({"disk": true}))).unwrap();
    assert!(measured["grafts"][0]["disk_bytes"].is_u64(), "{measured}");
    assert!(measured["grafts"][0]["note"].is_null(), "{measured}");
    assert!(
        list[0]["path"].as_str().unwrap().ends_with("/mounts/a"),
        "{list}"
    );
    c.ok(
        "graft_run",
        json!({"name": "a", "command": ["sh", "-c", "echo hi > new.txt"]}),
    );
    let status: Value = serde_json::from_str(&c.ok("graft_status", json!({"name": "a"}))).unwrap();
    assert_eq!(
        status["grafts"][0]["changes"],
        json!([{"kind": "A", "path": "new.txt"}]),
        "{status}"
    );

    // Reflink grafts cannot be budded, ringed or rewound; the hint says how.
    for (tool, args) in [
        ("graft_bud", json!({"src": "a", "dst": "b"})),
        ("graft_ring", json!({"name": "a"})),
        ("graft_rewind", json!({"name": "a", "ring": "r"})),
    ] {
        let e = error_of(&mut c, tool, args);
        assert_eq!(e["code"], "backend_unsupported", "{tool}: {e}");
        assert!(
            e["hint"]
                .as_str()
                .unwrap()
                .contains("graft_new with backend"),
            "{tool}: {e}"
        );
    }
    c.ok(
        "graft_prune",
        json!({"name": "a", "force": true, "confirm": true}),
    );

    // A failing layer build: nothing created, the output and a next step.
    std::fs::write(
        r.join("graft.toml"),
        "[[layer]]\npath = \"deps\"\ninputs = [\"deps.lock\"]\nbuild = 'echo resolving; echo broken toolchain >&2; exit 3'\n",
    )
    .unwrap();
    std::fs::write(r.join("deps.lock"), "v1\n").unwrap();
    git(r, &["add", "-A"]);
    git(r, &["commit", "-qm", "layer"]);
    let trust = Command::new(env!("CARGO_BIN_EXE_graft"))
        .arg("trust")
        .current_dir(r)
        .env("GRAFT_HOME", home.path())
        .status()
        .unwrap();
    assert!(trust.success());
    let e = error_of(
        &mut c,
        "graft_new",
        json!({"name": "b", "backend": "reflink"}),
    );
    assert_eq!(e["code"], "layer_build_failed", "{e}");
    assert!(
        e["message"].as_str().unwrap().contains("broken toolchain"),
        "{e}"
    );
    assert!(
        e["hint"].as_str().unwrap().contains("call graft_new again"),
        "{e}"
    );
    // It names the environment the build ran in: the MCP client's.
    let message = e["message"].as_str().unwrap();
    assert!(message.contains("build environment: `echo` is /"), "{e}");
    assert!(message.contains("PATH="), "{e}");
    // The hint is not repeated in the message.
    assert!(!message.contains("call graft_new again"), "{e}");
    assert_eq!(
        serde_json::from_str::<Value>(&c.ok("graft_list", json!({}))).unwrap()["grafts"],
        json!([])
    );
}

/// `graft_ring` then `graft_rewind` over MCP on a virtual graft, and the undo
/// hint names the tool (ADR 0025).
#[cfg(any(
    all(feature = "fuse-tests", target_os = "linux"),
    all(feature = "nfs-tests", target_os = "macos")
))]
#[test]
fn rings_rewind_over_mcp() {
    let (var, backend) = if cfg!(target_os = "linux") {
        ("GRAFT_TEST_FUSE", "fuse")
    } else {
        ("GRAFT_TEST_NFS", "nfs")
    };
    assert_eq!(std::env::var(var).as_deref(), Ok("1"), "needs {var}=1");
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    let r = repo.path();
    let init = Command::new(env!("CARGO_BIN_EXE_graft"))
        .arg("init")
        .current_dir(r)
        .env("GRAFT_HOME", home.path())
        .status()
        .unwrap();
    assert!(init.success());
    let mut c = Client::start(home.path(), r);
    c.ok("graft_new", json!({"name": "v", "backend": backend}));
    c.ok(
        "graft_run",
        json!({"name": "v", "command": ["sh", "-c", "echo one > a.txt"]}),
    );
    c.ok("graft_ring", json!({"name": "v", "label": "one"}));
    c.ok(
        "graft_run",
        json!({"name": "v", "command": ["sh", "-c", "echo two > b.txt"]}),
    );
    let done: Value =
        serde_json::from_str(&c.ok("graft_rewind", json!({"name": "v", "ring": "one"}))).unwrap();
    assert_eq!(done["ring"], "one", "{done}");
    assert!(
        done["undo"].as_str().unwrap().starts_with("undo-"),
        "{done}"
    );
    let status: Value = serde_json::from_str(&c.ok("graft_status", json!({"name": "v"}))).unwrap();
    assert_eq!(
        status["grafts"][0]["changes"],
        json!([{"kind": "A", "path": "a.txt"}]),
        "{status}"
    );
    let e = error_of(&mut c, "graft_rewind", json!({"name": "v", "ring": "nope"}));
    assert_eq!(e["code"], "not_found", "{e}");
    assert!(e["hint"].as_str().unwrap().contains("graft_ring"), "{e}");
    c.ok(
        "graft_prune",
        json!({"name": "v", "force": true, "confirm": true}),
    );
    let stop = Command::new(env!("CARGO_BIN_EXE_graft"))
        .args(["daemon", "stop"])
        .current_dir(r)
        .env("GRAFT_HOME", home.path())
        .status()
        .unwrap();
    assert!(stop.success());
}

/// Integrate's and prune's refusals have their own codes and MCP wording;
/// a dry run predicts the checked-out-branch refusal; plan lists what it
/// could not place (ADR 0025).
#[test]
#[allow(clippy::too_many_lines)] // one scenario, step by step
fn refusals_have_codes_and_dry_runs_predict_them() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    let r = repo.path();
    git(r, &["config", "user.name", "Graft Test"]);
    git(r, &["config", "user.email", "test@graft.invalid"]);
    std::fs::create_dir_all(r.join("pkg")).unwrap();
    std::fs::write(r.join("lib.py"), LIB).unwrap();
    std::fs::write(r.join("pkg/one.py"), "X = 1\n").unwrap();
    std::fs::write(r.join("pkg/two.py"), "Y = 2\n").unwrap();
    git(r, &["add", "-A"]);
    git(r, &["commit", "-qm", "lib"]);
    let init = Command::new(env!("CARGO_BIN_EXE_graft"))
        .arg("init")
        .current_dir(r)
        .env("GRAFT_HOME", home.path())
        .status()
        .unwrap();
    assert!(init.success());
    let mut c = Client::start(home.path(), r);
    let head = |r: &Path| {
        String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(r)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
    };
    let before = head(r);

    // plan: a directory expands, a new file is reported, not dropped.
    let plan: Value = serde_json::from_str(&c.ok(
        "graft_plan",
        json!({"task": "Edit `pkg` and add `pkg/three.py`."}),
    ))
    .unwrap();
    let claimed: Vec<&str> = plan["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|n| n["claims"].as_array().unwrap())
        .map(|c| c["path"].as_str().unwrap())
        .collect();
    assert!(
        claimed.contains(&"pkg/one.py") && claimed.contains(&"pkg/two.py"),
        "{plan}"
    );
    assert_eq!(plan["unresolved"], json!(["pkg/three.py"]), "{plan}");

    c.ok("graft_new", json!({"name": "a", "backend": "reflink"}));
    c.ok(
        "graft_run",
        json!({"name": "a", "command": ["sh", "-c", "echo 'Z = 3' >> pkg/one.py"]}),
    );
    // main is checked out in Root: the dry run already refuses, in MCP terms.
    let e = error_of(
        &mut c,
        "graft_integrate",
        json!({"names": ["a"], "onto": "main", "dry_run": true}),
    );
    assert_eq!(e["code"], "checkout_would_fall_behind", "{e}");
    let results: Value = serde_json::from_str(e["output"].as_str().unwrap()).unwrap();
    assert_eq!(
        results["results"][0]["code"], "checkout_would_fall_behind",
        "{results}"
    );
    let why = results["results"][0]["output"].as_str().unwrap();
    assert!(
        why.contains("update_checkout: true") && !why.contains("--update-checkout"),
        "{why}"
    );
    // With update_checkout the dry run passes, and so does the real one.
    let dry: Value = serde_json::from_str(&c.ok(
        "graft_integrate",
        json!({"names": ["a"], "onto": "main", "dry_run": true, "update_checkout": true}),
    ))
    .unwrap();
    assert_eq!(dry["results"][0]["state"], "checked", "{dry}");
    assert_eq!(head(r), before, "a dry run moved main");
    let landed: Value = serde_json::from_str(&c.ok(
        "graft_integrate",
        json!({"names": ["a"], "onto": "main", "update_checkout": true, "confirm": true}),
    ))
    .unwrap();
    assert_eq!(landed["results"][0]["state"], "landed", "{landed}");
    assert_eq!(landed["results"][0]["code"], "landed", "{landed}");
    assert_eq!(landed["gated"], false, "{landed}");
    let commit = landed["results"][0]["commit"].as_str().unwrap();
    assert_eq!(head(r).trim(), commit, "{landed}");

    // prune: would-lose-work has its own code and an MCP hint.
    c.ok("graft_new", json!({"name": "b", "backend": "reflink"}));
    c.ok(
        "graft_run",
        json!({"name": "b", "command": ["sh", "-c", "echo w > work.txt"]}),
    );
    let e = error_of(&mut c, "graft_prune", json!({"name": "b"}));
    assert_eq!(e["code"], "work_would_be_lost", "{e}");
    let hint = e["hint"].as_str().unwrap();
    assert!(
        hint.contains("force: true and confirm: true") && !hint.contains("--force"),
        "{e}"
    );
    let pruned: Value = serde_json::from_str(&c.ok(
        "graft_prune",
        json!({"name": "b", "force": true, "confirm": true}),
    ))
    .unwrap();
    assert_eq!(pruned, json!({"name": "b", "pruned": true}));
    c.ok("graft_prune", json!({"name": "a"}));
}

/// An agent can diagnose the machine and hand the user a bug-report bundle.
#[test]
fn doctor_over_mcp_reports_and_writes_a_bundle() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    let mut c = Client::start(home.path(), repo.path());
    let v: Value = serde_json::from_str(&c.ok("graft_doctor", json!({}))).unwrap();
    assert_eq!(v["healthy"], true);
    assert!(v["reflink"].is_string(), "{v}");
    assert!(v.get("report").is_none());

    let v: Value = serde_json::from_str(&c.ok("graft_doctor", json!({"report": true}))).unwrap();
    let path = std::path::PathBuf::from(v["report"].as_str().unwrap());
    assert!(path.starts_with(home.path().join("reports")), "{path:?}");
    let b: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(b["doctor"]["healthy"], true);
}

/// A failed call names its request ID, to match against graftd's log.
#[test]
fn failures_carry_a_request_id() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    let mut c = Client::start(home.path(), repo.path());
    c.ok("graft_init", json!({}));
    let (text, err) = c.call("graft_path", json!({"name": "missing"}));
    assert!(err, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    let id = v["request_id"].as_str().unwrap_or_default();
    assert!(id.starts_with("mcp-"), "{v}");
    assert!(id.parse::<graft_core::RequestId>().is_ok(), "{id}");
}

/// An agent can check Root's stored state.
#[test]
fn fsck_over_mcp() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    let mut c = Client::start(home.path(), repo.path());
    c.ok("graft_init", json!({}));
    let v: Value = serde_json::from_str(&c.ok("graft_fsck", json!({}))).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["findings"], json!([]));
}
