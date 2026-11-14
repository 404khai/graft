//! Output for agents driving `graft` through `graft mcp` (ADR 0025).
//!
//! The MCP server runs this binary with `GRAFT_MCP=1`. Then hints name the
//! MCP tools instead of CLI commands, and a failure ends with one JSON line
//! on stderr, `{"error":{"code":…,"message":…,"hint":…}}`, so an agent can
//! tell why a call failed without matching on its wording.

use std::fmt;

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
    }
    "failed"
}

/// The hint of the first [`Coded`] in `e`'s chain.
fn hint_of(e: &anyhow::Error) -> Option<&str> {
    e.chain()
        .find_map(|c| c.downcast_ref::<Coded>())
        .and_then(|c| c.hint.as_deref())
}

/// The JSON line that reports `e` to `graft mcp`.
pub(crate) fn error_json(e: &anyhow::Error) -> String {
    serde_json::json!({
        "error": {
            "code": code_of(e),
            "message": format!("{e:#}"),
            "hint": hint_of(e),
        }
    })
    .to_string()
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
                .starts_with("integrating a: no graft named a")
        );
        let bad = "Bad_Name".parse::<graft_core::GraftName>().unwrap_err();
        assert_eq!(code_of(&anyhow::Error::new(bad)), "invalid_name");
        assert_eq!(code_of(&anyhow::anyhow!("boom")), "failed");
    }
}
