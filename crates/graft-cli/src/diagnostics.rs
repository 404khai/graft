//! `graft doctor --report`: one JSON file to attach to a bug report.
//!
//! The bundle holds doctor's findings, the build, `GRAFT_*` variables,
//! `config.toml`, each Root's schema version, integrity and graft counts,
//! whether graftd answers, the end of its log, and the newest crash files.
//! Values of secret-looking names (AGENTS.md §11) are redacted wherever
//! they appear as `NAME=value` or `NAME = value`. File contents of grafts
//! are never read. Paths are kept: they are what a bug report needs, and
//! doctor tells the user to read the file before sharing it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use graft_core::{GraftHome, Timestamp};
use serde_json::{Map, Value, json};

use crate::Environment;
use crate::blueprints::is_secret;

/// Bumped when the bundle's shape changes.
const BUNDLE_VERSION: u32 = 1;
/// graftd log lines kept.
const LOG_LINES: usize = 200;
/// Crash files kept, newest first.
const CRASHES: usize = 5;

/// Writes the bundle and returns its path. No `dest` means
/// `$GRAFT_HOME/reports/report-<ms>.json`; a directory gets that file name.
///
/// # Errors
///
/// If there is nowhere to write it, or writing fails.
pub(crate) fn write_bundle(
    dest: Option<&Path>,
    env: &Environment,
    doctor: &Map<String, Value>,
) -> anyhow::Result<PathBuf> {
    let now = Timestamp::now();
    let name = format!("report-{}.json", now.as_millis());
    let path = if let Some(dest) = dest {
        if dest.is_dir() {
            dest.join(name)
        } else {
            dest.to_owned()
        }
    } else {
        let home = env
            .home
            .as_ref()
            .map_err(|e| anyhow::anyhow!("{e}; pass --report-to FILE to write it elsewhere"))?;
        let dir = home.path().join("reports");
        create_private_dir(&dir)?;
        dir.join(name)
    };
    let bundle = bundle(env, doctor, now);
    let text = serde_json::to_string_pretty(&bundle).context("encoding the report")? + "\n";
    write_private(&path, &text).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

fn create_private_dir(dir: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .with_context(|| format!("creating {}", dir.display()))
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(text.as_bytes())
}

/// The bundle's contents.
fn bundle(env: &Environment, doctor: &Map<String, Value>, now: Timestamp) -> Value {
    let mut out = json!({
        "bundle_version": BUNDLE_VERSION,
        "created_at": now.to_string(),
        "build": {
            "version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "debug": cfg!(debug_assertions),
        },
        "doctor": doctor,
        "environment": graft_variables(std::env::vars()),
    });
    if let Ok(home) = &env.home {
        out["config"] = config_text(home);
        out["roots"] = roots(home);
        out["daemon"] = daemon(home);
        out["crashes"] = crashes(home);
    }
    out
}

/// `GRAFT_*` variables, secrets redacted.
fn graft_variables(vars: impl Iterator<Item = (String, String)>) -> BTreeMap<String, String> {
    vars.filter(|(k, _)| k.starts_with("GRAFT_"))
        .map(|(k, v)| {
            let v = if is_secret(&k) {
                "<redacted>".to_owned()
            } else {
                v
            };
            (k, v)
        })
        .collect()
}

fn config_text(home: &GraftHome) -> Value {
    match std::fs::read_to_string(home.config_file()) {
        Ok(text) => redact(&text).into(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Null,
        Err(e) => format!("unreadable: {e}").into(),
    }
}

fn roots(home: &GraftHome) -> Value {
    let roots = match graft_store::registry::list_roots(home) {
        Ok(r) => r,
        Err(e) => return json!({"error": e.to_string()}),
    };
    roots
        .iter()
        .map(|(record, store)| {
            let mut states: BTreeMap<String, u64> = BTreeMap::new();
            let mut backends: BTreeMap<String, u64> = BTreeMap::new();
            let grafts = store.grafts();
            if let Ok(grafts) = &grafts {
                for g in grafts {
                    *states.entry(g.state.to_string()).or_default() += 1;
                    *backends.entry(g.backend.to_string()).or_default() += 1;
                }
            }
            let integrity = match store.integrity_check() {
                Ok(problems) if problems.is_empty() => json!("ok"),
                Ok(problems) => json!(problems),
                Err(e) => json!(format!("error: {e}")),
            };
            json!({
                "id": record.id.to_string(),
                "path": record.path.display().to_string(),
                "schema_version": store.schema_version().ok(),
                "supported_schema_version": graft_store::SCHEMA_VERSION,
                "integrity": integrity,
                "grafts": grafts.as_ref().map_or_else(
                    |e| json!({"error": e.to_string()}),
                    |_| json!({"by_state": states, "by_backend": backends}),
                ),
            })
        })
        .collect()
}

fn daemon(home: &GraftHome) -> Value {
    // Connect only: a report never starts graftd.
    let status = match graft_daemon::Client::connect(home).and_then(|mut c| c.ping()) {
        Ok(p) => {
            let stats = graft_daemon::Client::connect(home)
                .and_then(|mut c| c.stats())
                .ok();
            json!({"running": true, "version": p.version, "pid": p.pid, "stats": stats})
        }
        Err(e) => json!({"running": false, "detail": e.to_string()}),
    };
    let log = graft_daemon::paths::log_file(home);
    json!({
        "status": status,
        "log_file": log.display().to_string(),
        "log_tail": std::fs::read_to_string(&log).ok().map(|t| redact(&tail(&t, LOG_LINES))),
    })
}

fn crashes(home: &GraftHome) -> Value {
    graft_core::crash::crash_files(&graft_core::crash::crash_dir(home))
        .into_iter()
        .take(CRASHES)
        .map(|p| {
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            json!({"file": p.display().to_string(), "text": redact(&text)})
        })
        .collect()
}

/// The last `n` lines of `text`.
fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Replaces the value of every `NAME=value` or `NAME = value` whose name
/// looks secret.
fn redact(text: &str) -> String {
    text.lines().map(redact_line).collect::<Vec<_>>().join("\n")
}

fn redact_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(eq) = rest.find('=') {
        let (before, after) = rest.split_at(eq);
        let name_end = before.trim_end();
        let name_start = name_end
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'))
            .map_or(0, |i| i + 1);
        let name = &name_end[name_start..];
        out.push_str(before);
        out.push('=');
        let after = &after[1..];
        if !name.is_empty() && is_secret(name) {
            let value_start = after.len() - after.trim_start().len();
            let value_len = value_end(&after[value_start..]);
            out.push_str(&after[..value_start]);
            out.push_str("<redacted>");
            rest = &after[value_start + value_len..];
        } else {
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Length of a value: a quoted string, or up to whitespace or a comma.
fn value_end(v: &str) -> usize {
    match v.chars().next() {
        Some(q @ ('"' | '\'')) => v[1..].find(q).map_or(v.len(), |i| i + 2),
        _ => v
            .find(|c: char| c.is_whitespace() || c == ',')
            .unwrap_or(v.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secret_looking_assignments_only() {
        assert_eq!(
            redact_line("API_KEY=abc123 user=me"),
            "API_KEY=<redacted> user=me"
        );
        assert_eq!(
            redact_line(r#"token = "s e c r e t" # note"#),
            "token = <redacted> # note"
        );
        assert_eq!(
            redact_line("db_password='x', port=5432"),
            "db_password=<redacted>, port=5432"
        );
        assert_eq!(redact_line("filter = \"info\""), "filter = \"info\"");
        assert_eq!(redact_line("no assignments here"), "no assignments here");
    }

    #[test]
    fn only_graft_variables_are_kept_and_secrets_redacted() {
        let vars = [
            ("GRAFT_HOME", "/h"),
            ("GRAFT_API_TOKEN", "t"),
            ("HOME", "/Users/x"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned()));
        let kept = graft_variables(vars.into_iter());
        assert_eq!(kept.len(), 2);
        assert_eq!(kept["GRAFT_HOME"], "/h");
        assert_eq!(kept["GRAFT_API_TOKEN"], "<redacted>");
    }

    #[test]
    fn tail_keeps_the_last_lines() {
        assert_eq!(tail("a\nb\nc", 2), "b\nc");
        assert_eq!(tail("a", 5), "a");
    }
}
