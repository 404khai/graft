//! MCP server for Graft (ADR 0019).
//!
//! `graft mcp` serves the [`tools`] over stdio with the official Rust MCP
//! SDK. Each call runs the `graft` binary itself with arguments built from
//! the validated parameters ([`tools::invocation`]), so the tools behave
//! exactly as the CLI does, safety checks included. Destructive calls need
//! `confirm: true`. Radar's overlaps are also resources, `graft://radar`
//! and `graft://radar/<graft>`: a client that subscribes is sent
//! `notifications/resources/updated` whenever they change.

#![forbid(unsafe_code)]

pub mod tools;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    InitializeResult, ListResourceTemplatesResult, ListResourcesResult, ListToolsResult,
    PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult,
    Resource, ResourceContents, ResourceTemplate, ResourceUpdatedNotificationParam,
    ServerCapabilities, ServerConfig, SubscribeRequestParams, UnsubscribeRequestParams,
};
use rmcp::service::{Peer, RequestContext};
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt as _};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;

pub use tools::{Invocation, invocation, tools};

/// The Radar resource for all grafts.
pub const RADAR_URI: &str = "graft://radar";

/// What a Radar resource covers.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Scope {
    /// Every overlap.
    All,
    /// Overlaps involving one graft.
    Graft(String),
}

impl Scope {
    fn graft(&self) -> Option<String> {
        match self {
            Self::All => None,
            Self::Graft(g) => Some(g.clone()),
        }
    }
}

/// The scope of a Radar resource URI, or `None` if it is not one.
fn radar_filter(uri: &str) -> Option<Scope> {
    if uri == RADAR_URI {
        return Some(Scope::All);
    }
    uri.strip_prefix("graft://radar/")
        .filter(|g| !g.is_empty() && !g.starts_with('-') && !g.contains('/'))
        .map(|g| Scope::Graft(g.to_owned()))
}

/// The MCP server: runs `graft` for each tool call.
pub struct GraftServer {
    bin: PathBuf,
    cwd: PathBuf,
    watcher: Mutex<Option<tokio::task::JoinHandle<()>>>,
    subscriptions: Arc<Mutex<BTreeSet<String>>>,
    next_task: AtomicU64,
}

impl std::fmt::Debug for GraftServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraftServer")
            .field("bin", &self.bin)
            .field("cwd", &self.cwd)
            .finish_non_exhaustive()
    }
}

/// What running one command produced.
struct Ran {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl GraftServer {
    /// A server running the `graft` binary at `bin` in `cwd` (inside a
    /// registered Root).
    #[must_use]
    pub fn new(bin: PathBuf, cwd: PathBuf) -> Self {
        Self {
            bin,
            cwd,
            watcher: Mutex::new(None),
            subscriptions: Arc::new(Mutex::new(BTreeSet::new())),
            next_task: AtomicU64::new(1),
        }
    }

    async fn run(&self, inv: &Invocation) -> Result<Ran, String> {
        let task_file = match &inv.task_text {
            Some(text) => {
                let n = self.next_task.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir()
                    .join(format!("graft-mcp-task-{}-{n}.md", std::process::id()));
                std::fs::write(&path, text).map_err(|e| format!("writing the task: {e}"))?;
                Some(path)
            }
            None => None,
        };
        let args: Vec<String> = inv
            .args
            .iter()
            .map(|a| match (&task_file, a.as_str()) {
                (Some(p), "{task}") => p.display().to_string(),
                _ => a.clone(),
            })
            .collect();
        let child = Command::new(&self.bin)
            .args(&args)
            .current_dir(&self.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("starting {}: {e}", self.bin.display()));
        let result = match child {
            Ok(child) => match tokio::time::timeout(inv.timeout, child.wait_with_output()).await {
                Ok(Ok(out)) => Ok(Ran {
                    ok: out.status.success(),
                    stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                }),
                Ok(Err(e)) => Err(format!("running graft: {e}")),
                Err(_) => Err(format!(
                    "graft {} timed out after {}s",
                    args[0],
                    inv.timeout.as_secs()
                )),
            },
            Err(e) => Err(e),
        };
        if let Some(p) = task_file {
            let _ = std::fs::remove_file(p);
        }
        result
    }

    /// Starts watching Radar for subscribers (once per session).
    async fn watch(&self, peer: Peer<RoleServer>) {
        let mut slot = self.watcher.lock().await;
        if slot.as_ref().is_some_and(|h| !h.is_finished()) {
            return;
        }
        let (bin, cwd) = (self.bin.clone(), self.cwd.clone());
        let subs = Arc::clone(&self.subscriptions);
        *slot = Some(tokio::spawn(async move {
            if let Err(e) = forward_radar(&bin, &cwd, &peer, &subs).await {
                tracing::warn!(error = %e, "radar watch stopped");
            }
        }));
    }
}

/// Runs `graft radar --watch --json` and, for each overlap that appears or
/// goes, tells the client that every subscribed Radar resource it concerns
/// has changed.
async fn forward_radar(
    bin: &Path,
    cwd: &Path,
    peer: &Peer<RoleServer>,
    subs: &Mutex<BTreeSet<String>>,
) -> Result<(), String> {
    let mut child = Command::new(bin)
        .args(["radar", "--watch", "--json"])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines.next_line().await.map_err(|e| e.to_string())? {
        let Ok(event) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let grafts: Vec<String> = event["overlap"]["grafts"]
            .as_array()
            .map(|gs| {
                gs.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let targets: Vec<String> = subs
            .lock()
            .await
            .iter()
            .filter(|uri| match radar_filter(uri) {
                Some(Scope::All) => true,
                Some(Scope::Graft(g)) => grafts.contains(&g),
                None => false,
            })
            .cloned()
            .collect();
        for uri in targets {
            if peer
                .notify_resource_updated(ResourceUpdatedNotificationParam::new(uri))
                .await
                .is_err()
            {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Keeps only the overlaps involving `graft` in `graft radar --json` output.
fn filter_radar(stdout: &str, graft: &str) -> String {
    let Ok(mut v) = serde_json::from_str::<Value>(stdout) else {
        return stdout.to_owned();
    };
    if let Some(list) = v["overlaps"].as_array_mut() {
        list.retain(|o| {
            o["grafts"]
                .as_array()
                .is_some_and(|gs| gs.iter().any(|x| x.as_str() == Some(graft)))
        });
    }
    v.to_string() + "\n"
}

fn text_result(text: String, error: bool) -> CallToolResponse {
    let content = vec![ContentBlock::text(text)];
    CallToolResponse::Complete(if error {
        CallToolResult::error(content)
    } else {
        CallToolResult::success(content)
    })
}

fn not_found(uri: &str) -> McpError {
    McpError::resource_not_found(format!("no resource {uri}"), None)
}

// The trait's methods are async; some of ours have nothing to await.
#[allow(clippy::unused_async_trait_impl)]
impl ServerHandler for GraftServer {
    fn get_info(&self) -> ServerConfig {
        InitializeResult::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_resources_subscribe()
                .build(),
        )
        .with_server_info(Implementation::new("graft", env!("CARGO_PKG_VERSION")))
        .with_instructions(
            "Graft gives each agent its own copy-on-write workspace (a graft) of this repository. \
             Create one with graft_new, work in it with graft_run (confined to the graft), check \
             overlaps with other agents with graft_radar or by subscribing to graft://radar/<graft>, \
             and land it with graft_integrate.",
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(tools()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let args = request.arguments.unwrap_or_default();
        let inv = match invocation(&request.name, &args) {
            Ok(i) => i,
            Err(why) => return Ok(text_result(why, true)),
        };
        Ok(match self.run(&inv).await {
            Ok(ran) if ran.ok => {
                let stdout = match &inv.radar_filter {
                    Some(g) => filter_radar(&ran.stdout, g),
                    None => ran.stdout,
                };
                let text = if ran.stderr.trim().is_empty() {
                    stdout
                } else {
                    format!("{stdout}{}", ran.stderr)
                };
                text_result(text, false)
            }
            Ok(ran) => text_result(format!("{}{}", ran.stdout, ran.stderr), true),
            Err(why) => text_result(why, true),
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let mut radar = Resource::new(RADAR_URI, "radar")
            .with_title("Overlaps between live grafts")
            .with_description("Radar's report as JSON. Subscribe to be told when it changes.");
        radar.mime_type = Some("application/json".to_owned());
        Ok(ListResourcesResult::with_all_items(vec![radar]))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        let mut t = ResourceTemplate::new("graft://radar/{graft}", "radar-graft");
        t.description = Some(
            "Radar's overlaps involving one graft, as JSON. Subscribe to be told when they change."
                .to_owned(),
        );
        t.mime_type = Some("application/json".to_owned());
        Ok(ListResourceTemplatesResult::with_all_items(vec![t]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let filter = radar_filter(&request.uri).ok_or_else(|| not_found(&request.uri))?;
        let inv = Invocation {
            args: vec!["radar".to_owned(), "--json".to_owned()],
            task_text: None,
            timeout: std::time::Duration::from_secs(120),
            radar_filter: filter.graft(),
        };
        let ran = self
            .run(&inv)
            .await
            .map_err(|e| McpError::internal_error(e, None))?;
        if !ran.ok {
            return Err(McpError::internal_error(ran.stderr, None));
        }
        let text = match filter.graft() {
            Some(g) => filter_radar(&ran.stdout, &g),
            None => ran.stdout,
        };
        Ok(ReadResourceResponse::Complete(ReadResourceResult::new(
            vec![ResourceContents::text(text, request.uri)],
        )))
    }

    async fn subscribe(
        &self,
        request: SubscribeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        if radar_filter(&request.uri).is_none() {
            return Err(not_found(&request.uri));
        }
        self.subscriptions.lock().await.insert(request.uri);
        self.watch(context.peer.clone()).await;
        Ok(())
    }

    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        self.subscriptions.lock().await.remove(&request.uri);
        Ok(())
    }
}

/// Serves the tools over stdio until the client disconnects.
///
/// # Errors
///
/// If the runtime cannot start or the transport fails.
pub fn serve_stdio(
    bin: PathBuf,
    cwd: PathBuf,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let service = GraftServer::new(bin, cwd)
            .serve(rmcp::transport::stdio())
            .await?;
        service.waiting().await?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radar_uris() {
        assert_eq!(radar_filter("graft://radar"), Some(Scope::All));
        assert_eq!(
            radar_filter("graft://radar/auth"),
            Some(Scope::Graft("auth".to_owned()))
        );
        assert_eq!(radar_filter("graft://radar/"), None);
        assert_eq!(radar_filter("graft://radar/a/b"), None);
        assert_eq!(radar_filter("file:///etc/passwd"), None);
    }

    #[test]
    fn radar_output_is_filtered_by_graft() {
        let out = r#"{"overlaps":[{"grafts":["a","b"]},{"grafts":["c","d"]}],"notes":[]}"#;
        assert_eq!(
            filter_radar(out, "a"),
            "{\"notes\":[],\"overlaps\":[{\"grafts\":[\"a\",\"b\"]}]}\n"
        );
    }
}
