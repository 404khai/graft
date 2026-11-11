//! The tools: their JSON Schemas, and how each call becomes `graft`
//! arguments (ADR 0019).

use std::time::Duration;

use rmcp::model::{JsonObject, Tool, ToolAnnotations};
use serde_json::{Value, json};

/// How to run one tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// Arguments for the `graft` binary.
    pub args: Vec<String>,
    /// Task text to write to a temporary file, whose path replaces the
    /// `{task}` argument (`graft_plan` with `task`).
    pub task_text: Option<String>,
    /// How long it may run.
    pub timeout: Duration,
    /// For `graft_radar`: only overlaps involving this graft.
    pub radar_filter: Option<String>,
}

/// The default limit for most tools.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

fn object(v: Value) -> JsonObject {
    match v {
        Value::Object(m) => m,
        _ => JsonObject::new(),
    }
}

fn tool(
    name: &'static str,
    title: &str,
    description: &'static str,
    schema: Value,
    annotations: ToolAnnotations,
) -> Tool {
    let mut annotations = annotations;
    annotations.title = Some(title.to_owned());
    Tool::new(name, description, object(schema)).with_annotations(annotations)
}

const NAME: &str = "Graft name: lowercase letters, digits and hyphens, at most 40 characters.";

/// Every tool, in a fixed order.
#[must_use]
#[allow(clippy::too_many_lines)] // one schema per tool, read top to bottom
pub fn tools() -> Vec<Tool> {
    let read_only = || ToolAnnotations::new().read_only(true);
    let writes = || ToolAnnotations::new().read_only(false).destructive(false);
    vec![
        tool(
            "graft_new",
            "Create a graft",
            "Create a graft: a private copy-on-write workspace checked out from Root. Returns where it is.",
            json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": NAME},
                    "from": {"type": "string", "description": "Revision to start from (default: Root's HEAD)."},
                    "backend": {"type": "string", "enum": ["reflink", "fuse", "nfs"], "description": "Filesystem backend (default: the configured one)."},
                    "blueprint": {"type": "string", "description": "Blueprint of graft.toml to provision: ports, services, env files."}
                },
                "required": ["name"],
                "additionalProperties": false
            }),
            writes(),
        ),
        tool(
            "graft_list",
            "List grafts",
            "List grafts with their base, state, changed files and disk use.",
            json!({"type": "object", "properties": {}, "additionalProperties": false}),
            read_only(),
        ),
        tool(
            "graft_status",
            "Show a graft's changes",
            "Show what changed in a graft since its base (or summarise every graft).",
            json!({
                "type": "object",
                "properties": {"name": {"type": "string", "description": NAME}},
                "additionalProperties": false
            }),
            read_only(),
        ),
        tool(
            "graft_diff",
            "Diff a graft",
            "Show a graft's changes as a unified diff.",
            json!({
                "type": "object",
                "properties": {"name": {"type": "string", "description": NAME}},
                "required": ["name"],
                "additionalProperties": false
            }),
            read_only(),
        ),
        tool(
            "graft_run",
            "Run a command in a graft",
            "Run a command confined to a graft: it can only write inside the graft (and its private TMPDIR), with the graft's blueprint environment. The command is an argument list, not a shell line.",
            json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": NAME},
                    "command": {"type": "array", "items": {"type": "string"}, "minItems": 1, "description": "Program and arguments, e.g. [\"cargo\", \"test\"]."},
                    "timeout_secs": {"type": "integer", "minimum": 1, "maximum": 86400, "description": "Kill it after this many seconds (default 600)."}
                },
                "required": ["name", "command"],
                "additionalProperties": false
            }),
            writes(),
        ),
        tool(
            "graft_bud",
            "Fork a graft",
            "Fork a virtual graft in constant time: a new graft with the same base and a copy of its changes.",
            json!({
                "type": "object",
                "properties": {
                    "src": {"type": "string", "description": NAME},
                    "dst": {"type": "string", "description": "The new graft's name."}
                },
                "required": ["src", "dst"],
                "additionalProperties": false
            }),
            writes(),
        ),
        tool(
            "graft_ring",
            "Checkpoint a graft",
            "Checkpoint a virtual graft's changes as a named ring (restore it later with `graft rewind`), or list its rings.",
            json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": NAME},
                    "label": {"type": "string", "description": "The ring's label (default ring-<seq>)."},
                    "list": {"type": "boolean", "description": "List the graft's rings instead."}
                },
                "required": ["name"],
                "additionalProperties": false
            }),
            writes(),
        ),
        tool(
            "graft_radar",
            "Find overlapping work",
            "Overlaps between live grafts, as JSON: same file (path), same symbol, merge conflicts, or tests that fail only when merged (behaviour). To be told when overlaps change, subscribe to the resource graft://radar/<graft> (or graft://radar for all).",
            json!({
                "type": "object",
                "properties": {
                    "graft": {"type": "string", "description": "Only overlaps involving this graft."},
                    "check": {"type": "boolean", "description": "Run the behaviour checks (speculative merges with the [radar] test) first."}
                },
                "additionalProperties": false
            }),
            read_only(),
        ),
        tool(
            "graft_integrate",
            "Land grafts in Root",
            "Merge grafts onto a branch of Root through the integrate queue: structural merge, test gate, one commit each. Changes Root's history, so it needs confirm: true unless dry_run.",
            json!({
                "type": "object",
                "properties": {
                    "names": {"type": "array", "items": {"type": "string"}, "minItems": 1, "description": "Grafts, in order."},
                    "onto": {"type": "string", "description": "Branch to land on."},
                    "message": {"type": "string", "description": "Commit message."},
                    "dry_run": {"type": "boolean", "description": "Run every step except landing."},
                    "prune": {"type": "boolean", "description": "Prune each graft once it has landed."},
                    "update_checkout": {"type": "boolean", "description": "Allow landing on the branch Root has checked out (fast-forwards Root's clean working tree)."},
                    "confirm": {"type": "boolean", "description": "Required (true) to land anything."}
                },
                "required": ["names"],
                "additionalProperties": false
            }),
            ToolAnnotations::new().read_only(false).destructive(true),
        ),
        tool(
            "graft_plan",
            "Plan a task",
            "Split a task into an ordered DAG of grafts along the code's dependency graph, as JSON. The task names files and symbols in `code spans`. With apply, creates the grafts that are ready and records their claims.",
            json!({
                "type": "object",
                "properties": {
                    "task": {"type": "string", "description": "The task, as Markdown."},
                    "task_file": {"type": "string", "description": "Or a path to it."},
                    "apply": {"type": "boolean", "description": "Create the ready grafts."},
                    "backend": {"type": "string", "enum": ["reflink", "fuse", "nfs"]}
                },
                "additionalProperties": false
            }),
            writes(),
        ),
        tool(
            "graft_prune",
            "Remove a graft",
            "Remove a graft. Refuses if work would be lost; force discards it and needs confirm: true.",
            json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": NAME},
                    "force": {"type": "boolean", "description": "Discard uncommitted or unintegrated work."},
                    "confirm": {"type": "boolean", "description": "Required (true) with force."}
                },
                "required": ["name"],
                "additionalProperties": false
            }),
            ToolAnnotations::new().read_only(false).destructive(true),
        ),
    ]
}

/// A string argument that cannot be mistaken for a flag.
fn word(args: &JsonObject, key: &str) -> Result<Option<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.is_empty() => Err(format!("{key} must not be empty")),
        Some(Value::String(s)) if s.starts_with('-') => {
            Err(format!("{key} must not start with '-'"))
        }
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("{key} must be a string")),
    }
}

fn required(args: &JsonObject, key: &str) -> Result<String, String> {
    word(args, key)?.ok_or_else(|| format!("{key} is required"))
}

fn flag(args: &JsonObject, key: &str) -> Result<bool, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{key} must be true or false")),
    }
}

fn strings(args: &JsonObject, key: &str) -> Result<Vec<String>, String> {
    match args.get(key) {
        Some(Value::Array(a)) if !a.is_empty() => a
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("{key} must be strings"))
            })
            .collect(),
        _ => Err(format!("{key} must be a non-empty array of strings")),
    }
}

fn option(out: &mut Vec<String>, flag: &str, value: Option<String>) {
    if let Some(v) = value {
        out.push(flag.to_owned());
        out.push(v);
    }
}

/// `graft_integrate`'s arguments.
fn integrate(args: &JsonObject, a: &mut Vec<String>) -> Result<(), String> {
    let dry_run = flag(args, "dry_run")?;
    if !dry_run && !flag(args, "confirm")? {
        return Err(
            "graft_integrate lands commits in Root: pass confirm: true to do it, or dry_run: true to check first"
                .to_owned(),
        );
    }
    a.push("integrate".to_owned());
    for n in strings(args, "names")? {
        if n.starts_with('-') {
            return Err("names must not start with '-'".to_owned());
        }
        a.push(n);
    }
    option(a, "--onto", word(args, "onto")?);
    if let Some(m) = args.get("message") {
        let m = m.as_str().ok_or("message must be a string")?;
        a.extend(["--message".to_owned(), m.to_owned()]);
    }
    for (key, f) in [
        ("dry_run", "--dry-run"),
        ("prune", "--prune"),
        ("update_checkout", "--update-checkout"),
    ] {
        if flag(args, key)? {
            a.push(f.to_owned());
        }
    }
    Ok(())
}

/// `graft_plan`'s arguments; a task given as text goes to `task_text`.
fn plan(
    args: &JsonObject,
    task_text: &mut Option<String>,
    a: &mut Vec<String>,
) -> Result<(), String> {
    a.push("plan".to_owned());
    match (
        args.get("task").and_then(Value::as_str),
        word(args, "task_file")?,
    ) {
        (Some(text), None) => {
            *task_text = Some(text.to_owned());
            a.push("{task}".to_owned());
        }
        (None, Some(file)) => a.push(file),
        _ => return Err("pass exactly one of task and task_file".to_owned()),
    }
    a.push("--json".to_owned());
    if flag(args, "apply")? {
        a.push("--apply".to_owned());
    }
    option(a, "--backend", word(args, "backend")?);
    Ok(())
}

/// The `graft` invocation for tool `name` with `args`, or why the call is
/// refused (bad arguments, or a destructive call without `confirm`).
///
/// # Errors
///
/// A message for the caller.
pub fn invocation(name: &str, args: &JsonObject) -> Result<Invocation, String> {
    let known: Vec<String> = tools()
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| format!("no tool {name}"))?
        .input_schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default();
    if let Some(extra) = args.keys().find(|k| !known.contains(k)) {
        return Err(format!("{name} takes no argument {extra}"));
    }
    let mut inv = Invocation {
        args: Vec::new(),
        task_text: None,
        timeout: DEFAULT_TIMEOUT,
        radar_filter: None,
    };
    let a = &mut inv.args;
    match name {
        "graft_new" => {
            a.extend(["new".to_owned(), required(args, "name")?]);
            option(a, "--from", word(args, "from")?);
            option(a, "--backend", word(args, "backend")?);
            option(a, "--blueprint", word(args, "blueprint")?);
        }
        "graft_list" => a.push("ls".to_owned()),
        "graft_status" => {
            a.push("status".to_owned());
            a.extend(word(args, "name")?);
        }
        "graft_diff" => a.extend(["diff".to_owned(), required(args, "name")?]),
        "graft_run" => {
            a.extend(["run".to_owned(), required(args, "name")?, "--".to_owned()]);
            a.extend(strings(args, "command")?);
            if let Some(t) = args.get("timeout_secs") {
                let secs = t
                    .as_u64()
                    .filter(|s| (1..=86_400).contains(s))
                    .ok_or("timeout_secs must be an integer from 1 to 86400")?;
                inv.timeout = Duration::from_secs(secs);
            }
        }
        "graft_bud" => a.extend([
            "bud".to_owned(),
            required(args, "src")?,
            required(args, "dst")?,
        ]),
        "graft_ring" => {
            a.extend(["ring".to_owned(), required(args, "name")?]);
            if flag(args, "list")? {
                a.push("--list".to_owned());
            } else {
                a.extend(word(args, "label")?);
            }
        }
        "graft_radar" => {
            a.extend(["radar".to_owned(), "--json".to_owned()]);
            if flag(args, "check")? {
                a.push("--check".to_owned());
            }
            inv.radar_filter = word(args, "graft")?;
        }
        "graft_integrate" => integrate(args, a)?,
        "graft_plan" => plan(args, &mut inv.task_text, a)?,
        "graft_prune" => {
            let force = flag(args, "force")?;
            if force && !flag(args, "confirm")? {
                return Err(
                    "graft_prune with force discards the graft's unintegrated work: pass confirm: true to do it"
                        .to_owned(),
                );
            }
            a.extend(["prune".to_owned(), required(args, "name")?]);
            if force {
                a.push("--force".to_owned());
            }
        }
        _ => return Err(format!("no tool {name}")),
    }
    Ok(inv)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn call(name: &str, args: Value) -> Result<Vec<String>, String> {
        invocation(name, &object(args)).map(|i| i.args)
    }

    #[test]
    fn tool_schemas_are_stable() {
        let all: Vec<Value> = tools()
            .iter()
            .map(|t| serde_json::to_value(t).unwrap())
            .collect();
        insta::assert_snapshot!(serde_json::to_string_pretty(&all).unwrap());
    }

    #[test]
    fn calls_become_graft_arguments() {
        assert_eq!(
            call("graft_new", json!({"name": "a", "backend": "nfs"})).unwrap(),
            ["new", "a", "--backend", "nfs"]
        );
        assert_eq!(
            call(
                "graft_run",
                json!({"name": "a", "command": ["sh", "-c", "echo $HOME"]})
            )
            .unwrap(),
            ["run", "a", "--", "sh", "-c", "echo $HOME"]
        );
        assert_eq!(
            call("graft_radar", json!({"check": true})).unwrap(),
            ["radar", "--json", "--check"]
        );
        assert_eq!(
            call(
                "graft_integrate",
                json!({"names": ["a", "b"], "onto": "main", "confirm": true})
            )
            .unwrap(),
            ["integrate", "a", "b", "--onto", "main"]
        );
        assert_eq!(
            call("graft_plan", json!({"task_file": "t.md", "apply": true})).unwrap(),
            ["plan", "t.md", "--json", "--apply"]
        );
        let plan = invocation("graft_plan", &object(json!({"task": "Change `x`"}))).unwrap();
        assert_eq!(plan.args, ["plan", "{task}", "--json"]);
        assert_eq!(plan.task_text.as_deref(), Some("Change `x`"));
    }

    #[test]
    fn destructive_calls_need_confirmation() {
        let e = call("graft_integrate", json!({"names": ["a"]})).unwrap_err();
        assert!(e.contains("confirm: true"), "{e}");
        assert_eq!(
            call("graft_integrate", json!({"names": ["a"], "dry_run": true})).unwrap(),
            ["integrate", "a", "--dry-run"]
        );
        let e = call("graft_prune", json!({"name": "a", "force": true})).unwrap_err();
        assert!(e.contains("confirm: true"), "{e}");
        assert_eq!(
            call("graft_prune", json!({"name": "a"})).unwrap(),
            ["prune", "a"]
        );
        assert_eq!(
            call(
                "graft_prune",
                json!({"name": "a", "force": true, "confirm": true})
            )
            .unwrap(),
            ["prune", "a", "--force"]
        );
    }

    #[test]
    fn arguments_are_checked() {
        for (name, args, needle) in [
            ("graft_new", json!({}), "name is required"),
            (
                "graft_new",
                json!({"name": "--help"}),
                "must not start with '-'",
            ),
            ("graft_diff", json!({"name": 3}), "must be a string"),
            (
                "graft_run",
                json!({"name": "a", "command": []}),
                "non-empty array",
            ),
            (
                "graft_run",
                json!({"name": "a", "command": ["x"], "timeout_secs": 0}),
                "timeout_secs",
            ),
            ("graft_list", json!({"all": true}), "takes no argument all"),
            ("graft_plan", json!({}), "exactly one of task and task_file"),
            ("graft_shell", json!({}), "no tool graft_shell"),
        ] {
            let e = call(name, args).unwrap_err();
            assert!(e.contains(needle), "{name}: {e}");
        }
    }
}
