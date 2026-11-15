//! Output for agents driving `graft` through `graft mcp` (ADR 0025).
//!
//! The MCP server runs this binary with `GRAFT_MCP=1`. Then hints name the
//! MCP tools instead of CLI commands, and a failure ends with one JSON line
//! on stderr, `{"error":{"code":…,"message":…,"hint":…}}`, so an agent can
//! tell why a call failed without matching on its wording.

use std::fmt;
use std::io::Write as _;

use graft_core::InvalidGraftName;

/// Set by `graft mcp` for the commands it runs.
pub(crate) const ENV: &str = "GRAFT_MCP";

/// Whether this process runs on behalf of `graft mcp`.
pub(crate) fn is_mcp() -> bool {
    std::env::var_os(ENV).is_some_and(|v| v == "1")
}

/// The way to suggest doing something: the MCP tool under `graft mcp`,
/// else the CLI command.
pub(crate) fn suggest(cli: &'static str, tool: &'static str) -> &'static str {
    if is_mcp() { tool } else { cli }
}

/// A failure with a stable code for agents, and an optional next step.
#[derive(Debug)]
pub(crate) struct Coded {
    pub code: &'static str,
    pub message: String,
    pub hint: Option<String>,
}

impl fmt::Display for Coded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if let Some(h) = &self.hint {
            write!(f, "; {h}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Coded {}

/// An error with `code`, `message` and an optional `hint`.
pub(crate) fn coded(
    code: &'static str,
    message: impl Into<String>,
    hint: Option<String>,
) -> anyhow::Error {
    anyhow::Error::new(Coded {
        code,
        message: message.into(),
        hint,
    })
}

/// The code of `e`: the first [`Coded`] in its chain, `invalid_name` for
/// a bad graft name, else `failed`.
pub(crate) fn code_of(e: &anyhow::Error) -> &'static str {
    for cause in e.chain() {
        if let Some(c) = cause.downcast_ref::<Coded>() {
            return c.code;
        }
        if cause.downcast_ref::<InvalidGraftName>().is_some() {
            return "invalid_name";
        }
        if not_registered(cause) {
            return "not_registered";
        }
    }
    "failed"
}

fn not_registered(cause: &(dyn std::error::Error + 'static)) -> bool {
    matches!(
        cause.downcast_ref::<graft_store::StoreError>(),
        Some(graft_store::StoreError::NotRegistered { .. })
    )
}

/// The hint of the first [`Coded`] in `e`'s chain.
fn hint_of(e: &anyhow::Error) -> Option<&str> {
    if e.chain().any(not_registered) {
        return Some(suggest("run `graft init`", "call graft_init"));
    }
    e.chain()
        .find_map(|c| c.downcast_ref::<Coded>())
        .and_then(|c| c.hint.as_deref())
}

/// `e`'s message chain without the hint, which is reported on its own.
fn message_of(e: &anyhow::Error) -> String {
    e.chain()
        .map(|c| match c.downcast_ref::<Coded>() {
            Some(coded) => coded.message.clone(),
            None => c.to_string(),
        })
        .collect::<Vec<_>>()
        .join(": ")
}

/// The JSON line that reports `e` to `graft mcp`.
pub(crate) fn error_json(e: &anyhow::Error) -> String {
    line(code_of(e), &message_of(e), hint_of(e))
}

fn line(code: &str, message: &str, hint: Option<&str>) -> String {
    serde_json::json!({
        "error": {"code": code, "message": message, "hint": hint}
    })
    .to_string()
}

/// For a command that fails without returning an error (it still prints
/// its result): under `graft mcp`, writes the JSON error line to stderr.
pub(crate) fn report_failure(code: &str, message: &str, hint: Option<&str>) {
    if is_mcp() {
        let _ = writeln!(std::io::stderr(), "{}", line(code, message, hint));
    }
}

/// Under `graft mcp`, writes how a `graft run` command ended to stderr as
/// `{"run":{"exit_code","signal","duration_ms"}}`, for `graft_run`'s result.
pub(crate) fn report_run(status: std::process::ExitStatus, took: std::time::Duration) {
    if !is_mcp() {
        return;
    }
    #[cfg(unix)]
    let signal = std::os::unix::process::ExitStatusExt::signal(&status);
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    let v = serde_json::json!({
        "run": {
            "exit_code": status.code(),
            "signal": signal,
            "duration_ms": u64::try_from(took.as_millis()).unwrap_or(u64::MAX),
        }
    });
    let _ = writeln!(std::io::stderr(), "{v}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_come_from_the_chain() {
        let e = coded(
            "not_found",
            "no graft named a",
            Some("see graft_list".into()),
        )
        .context("integrating a");
        assert_eq!(code_of(&e), "not_found");
        let v: serde_json::Value = serde_json::from_str(&error_json(&e)).unwrap();
        assert_eq!(v["error"]["code"], "not_found");
        assert_eq!(v["error"]["hint"], "see graft_list");
        assert!(
            v["error"]["message"]
                .as_str()
                .unwrap()
                .ends_with("no graft named a")
        );
        assert!(
            v["error"]["message"]
                .as_str()
                .unwrap()
                .starts_with("integrating a: no graft named a")
        );
        let bad = "Bad_Name".parse::<graft_core::GraftName>().unwrap_err();
        assert_eq!(code_of(&anyhow::Error::new(bad)), "invalid_name");
        assert_eq!(code_of(&anyhow::anyhow!("boom")), "failed");
    }
}
