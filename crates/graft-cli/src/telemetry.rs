//! Opt-in usage telemetry (ADR 0034).
//!
//! Off unless the user runs `graft telemetry on`, and even then nothing
//! leaves the machine until `config.toml` names an `[telemetry] endpoint`;
//! Graft has no default. `$DO_NOT_TRACK` or `GRAFT_TELEMETRY=0` turns it
//! off for one process.
//!
//! An event is one finished command: its name, whether it succeeded, the
//! error *code* (never the message), a coarse duration bucket, the day,
//! the version, OS and architecture, and whether it came through MCP. No
//! arguments, graft names, paths, repository data, environment, user or
//! machine identifier. Events queue in `$GRAFT_HOME/telemetry/queue.jsonl`
//! (at most [`MAX_QUEUED`]); `graft telemetry show` prints exactly what
//! would be sent. A queue is sent at most once a day by a detached
//! `graft __telemetry-send`, so no command ever waits on the network.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use anyhow::{Context, bail};
use graft_core::{GraftHome, Timestamp};
use serde_json::{Value, json};

use crate::{Environment, Output};

/// Variable that turns telemetry off for a process (`0`, `off`, `false`).
pub(crate) const TELEMETRY_ENV: &str = "GRAFT_TELEMETRY";
/// The cross-tool convention (<https://consoledonottrack.com>).
const DO_NOT_TRACK_ENV: &str = "DO_NOT_TRACK";
/// Events kept; the oldest are dropped beyond this.
pub(crate) const MAX_QUEUED: usize = 1000;
/// Format of an event and of a batch.
const SCHEMA: u32 = 1;
/// How often a queue is sent automatically.
const SEND_EVERY: Duration = Duration::from_hours(24);
/// Upload timeout.
const TIMEOUT: Duration = Duration::from_secs(10);
/// A sender lock older than this is from a sender that died.
const STALE_LOCK: Duration = Duration::from_secs(600);

/// Arguments for `graft telemetry`.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    #[command(subcommand)]
    action: Action,
    /// For status, on and off: print the resulting state as JSON.
    #[arg(long, global = true)]
    json: bool,
}

/// The state `status`, `on` and `off` report as JSON.
fn state_json(home: &GraftHome, url: Option<&str>) -> Value {
    json!({
        "consent": consent_file(home).is_file(),
        "recording": recording(home),
        "overridden_by": overridden(),
        "endpoint": url,
        "queued": queued(home).len(),
        "collects": WHAT,
    })
}

#[derive(Debug, clap::Subcommand)]
enum Action {
    /// Say whether telemetry is on, where it would go, and what is queued.
    Status,
    /// Opt in: record anonymous usage events (see `graft telemetry show`).
    On,
    /// Opt out, and delete every queued event.
    Off,
    /// Print the queued events exactly as they would be sent.
    Show,
    /// Send the queued events now.
    Send,
}

fn dir(home: &GraftHome) -> PathBuf {
    home.path().join("telemetry")
}
fn consent_file(home: &GraftHome) -> PathBuf {
    dir(home).join("consent")
}
fn queue_file(home: &GraftHome) -> PathBuf {
    dir(home).join("queue.jsonl")
}
fn sent_file(home: &GraftHome) -> PathBuf {
    dir(home).join("last-sent")
}

/// Why telemetry is off for this process despite consent, if it is.
fn overridden() -> Option<&'static str> {
    let off = |v: &str| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "off" | "false" | "no"
        )
    };
    if std::env::var(TELEMETRY_ENV).is_ok_and(|v| off(&v)) {
        return Some("GRAFT_TELEMETRY is off");
    }
    if std::env::var(DO_NOT_TRACK_ENV).is_ok_and(|v| !v.trim().is_empty() && v.trim() != "0") {
        return Some("DO_NOT_TRACK is set");
    }
    None
}

/// Whether events are recorded: the user opted in and nothing overrides it.
fn recording(home: &GraftHome) -> bool {
    consent_file(home).is_file() && overridden().is_none()
}

fn endpoint(env: &Environment) -> Option<String> {
    env.config
        .as_ref()
        .and_then(|c| c.as_ref().ok())
        .and_then(|c| c.telemetry.endpoint.clone())
}

/// Duration as a coarse bucket, so timings cannot fingerprint a repo.
fn bucket(took: Duration) -> &'static str {
    match took.as_millis() {
        0..100 => "<100ms",
        100..1000 => "<1s",
        1000..10_000 => "<10s",
        10_000..60_000 => "<1m",
        _ => ">=1m",
    }
}

/// The event for one finished command.
fn event(command: &str, ok: bool, code: Option<&str>, took: Duration) -> Value {
    let day = Timestamp::now().to_string();
    json!({
        "v": SCHEMA,
        "day": day.get(..10).unwrap_or(&day),
        "command": command,
        "ok": ok,
        "code": code,
        "duration": bucket(took),
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "via": if crate::agent::is_mcp() { "mcp" } else { "cli" },
    })
}

/// Records a finished `command` if the user opted in, then starts a
/// background send when one is due. Never fails the command: problems
/// are logged at debug.
pub(crate) fn after_command(
    env: &Environment,
    command: &str,
    result: &anyhow::Result<ExitCode>,
    took: Duration,
) {
    let Ok(home) = &env.home else { return };
    if !recording(home) {
        return;
    }
    let (ok, code) = match result {
        Ok(c) if *c == ExitCode::SUCCESS => (true, None),
        Ok(_) => (false, Some("failed")),
        Err(e) => (false, Some(crate::agent::code_of(e))),
    };
    if let Err(e) = append(home, &event(command, ok, code, took)) {
        tracing::debug!(error = %e, "telemetry: cannot queue an event");
        return;
    }
    if endpoint(env).is_some() && due(home) {
        spawn_sender(home);
    }
}

fn append(home: &GraftHome, event: &Value) -> std::io::Result<()> {
    let queue = queue_file(home);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&queue)?;
    writeln!(f, "{event}")?;
    drop(f);
    // Trim from the front when over the cap; rare, so rewrite in place.
    let text = std::fs::read_to_string(&queue)?;
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() > MAX_QUEUED {
        let keep = lines[lines.len() - MAX_QUEUED..].join("\n") + "\n";
        std::fs::write(&queue, keep)?;
    }
    Ok(())
}

fn queued(home: &GraftHome) -> Vec<Value> {
    std::fs::read_to_string(queue_file(home))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Whether the last send was long enough ago (or never happened).
fn due(home: &GraftHome) -> bool {
    std::fs::metadata(sent_file(home))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_none_or(|age| age >= SEND_EVERY)
}

fn spawn_sender(home: &GraftHome) {
    use std::process::{Command, Stdio};
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut cmd = Command::new(exe);
    cmd.arg("__telemetry-send")
        .env(graft_core::GRAFT_HOME_ENV, home.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    if let Err(e) = cmd.spawn() {
        tracing::debug!(error = %e, "telemetry: cannot start the sender");
    }
}

/// Checks that `url` may receive events: `https`, or `http` to loopback.
fn check_endpoint(url: &str) -> anyhow::Result<()> {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return Ok(());
    }
    if let Some(rest) = lower.strip_prefix("http://") {
        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
        let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
        let host = if host.starts_with('[') {
            host.split(']').next().map_or(host, |h| &h[1..])
        } else {
            host.split(':').next().unwrap_or(host)
        };
        if matches!(host, "localhost" | "127.0.0.1" | "::1") {
            return Ok(());
        }
    }
    bail!("telemetry endpoint {url:?} must use https (http is allowed only to localhost)")
}

/// Holds the sender lock while it lives.
struct SendLock(PathBuf);

impl SendLock {
    fn take(home: &GraftHome) -> Option<Self> {
        let path = dir(home).join("send.lock");
        let stale = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age > STALE_LOCK);
        if stale {
            let _ = std::fs::remove_file(&path);
        }
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .ok()
            .map(|_| Self(path))
    }
}

impl Drop for SendLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Sends the queue to `endpoint`; returns how many events went. Events
/// queued while it runs are kept for the next send.
fn send(home: &GraftHome, endpoint: &str) -> anyhow::Result<usize> {
    check_endpoint(endpoint)?;
    let Some(_lock) = SendLock::take(home) else {
        bail!("another graft process is sending telemetry");
    };
    let queue = queue_file(home);
    let sending = dir(home).join("sending.jsonl");
    // Take the queue aside, so appends during the upload go to a new file.
    match std::fs::rename(&queue, &sending) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !sending.exists() => {
            touch(&sent_file(home));
            return Ok(0);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {} // retry a failed batch
        Err(e) => return Err(e).context("taking the telemetry queue"),
    }
    let events: Vec<Value> = std::fs::read_to_string(&sending)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let body = json!({"schema": SCHEMA, "events": events});
    let sent = agent()
        .post(endpoint)
        .header("content-type", "application/json")
        .header("user-agent", concat!("graft/", env!("CARGO_PKG_VERSION")))
        .send(body.to_string());
    match sent {
        Ok(_) => {
            let _ = std::fs::remove_file(&sending);
            touch(&sent_file(home));
            Ok(events.len())
        }
        Err(e) => {
            // Put the batch back in front of anything queued meanwhile.
            let newer = std::fs::read_to_string(&queue).unwrap_or_default();
            let older = std::fs::read_to_string(&sending).unwrap_or_default();
            std::fs::write(&queue, older + &newer).context("restoring the telemetry queue")?;
            let _ = std::fs::remove_file(&sending);
            // Back off until the next day either way.
            touch(&sent_file(home));
            Err(anyhow::anyhow!("sending telemetry to {endpoint}: {e}"))
        }
    }
}

/// The HTTP client: rustls, verifying with the OS trust store (so
/// enterprise CAs work), with a timeout; non-2xx answers are errors.
fn agent() -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig};
    ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(true)
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .new_agent()
}

fn touch(path: &Path) {
    let _ = std::fs::write(path, Timestamp::now().to_string());
}

/// `graft __telemetry-send`: the detached background sender.
pub(crate) fn background_send(env: &Environment) -> ExitCode {
    let (Ok(home), Some(url)) = (&env.home, endpoint(env)) else {
        return ExitCode::SUCCESS;
    };
    if !recording(home) {
        return ExitCode::SUCCESS;
    }
    match send(home, &url) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::debug!(error = %e, "telemetry: send failed");
            ExitCode::FAILURE
        }
    }
}

const WHAT: &str = "Each finished command records: its name, success or failure, the error code \
(not the message), a duration bucket, the day, graft's version, OS and architecture, and \
whether it came through MCP. Never arguments, graft names, paths, file contents, environment \
or any user or machine identifier.";

/// Runs `graft telemetry`.
///
/// # Errors
///
/// An unusable `$GRAFT_HOME`, or a failed `send`.
pub(crate) fn run(args: &Args, env: &Environment) -> anyhow::Result<Output> {
    let home = env.require_home()?;
    let url = endpoint(env);
    let mut out = String::new();
    match args.action {
        Action::On => {
            std::fs::create_dir_all(dir(home)).context("creating the telemetry directory")?;
            touch(&consent_file(home));
            let _ = writeln!(out, "Telemetry is on.\n{WHAT}");
            let _ = writeln!(
                out,
                "See what is queued with `graft telemetry show`; turn it off with `graft telemetry off`."
            );
            match &url {
                Some(u) => {
                    check_endpoint(u)?;
                    let _ = writeln!(out, "Events go to {u}, at most once a day.");
                }
                None => {
                    let _ = writeln!(
                        out,
                        "No [telemetry] endpoint is set in config.toml, so events stay on this machine."
                    );
                }
            }
        }
        Action::Off => {
            for f in [
                consent_file(home),
                queue_file(home),
                dir(home).join("sending.jsonl"),
            ] {
                match std::fs::remove_file(&f) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e).with_context(|| format!("removing {}", f.display())),
                }
            }
            out.push_str("Telemetry is off; queued events were deleted.\n");
        }
        Action::Status => {
            let consent = consent_file(home).is_file();
            let state = match (consent, overridden()) {
                (false, _) => "off (opt in with `graft telemetry on`)".to_owned(),
                (true, Some(why)) => format!("on, but off for this process ({why})"),
                (true, None) => "on".to_owned(),
            };
            let _ = writeln!(out, "telemetry  {state}");
            let _ = writeln!(
                out,
                "endpoint   {}",
                url.as_deref().unwrap_or("none (nothing is sent)")
            );
            let _ = writeln!(out, "queued     {} event(s)", queued(home).len());
        }
        Action::Show => {
            let events = queued(home);
            if events.is_empty() {
                out.push_str("no events queued\n");
            }
            for e in events {
                let _ = writeln!(out, "{e}");
            }
        }
        Action::Send => {
            let Some(u) = url.clone() else {
                bail!(
                    "no [telemetry] endpoint is set in config.toml, so there is nowhere to send to"
                );
            };
            if !consent_file(home).is_file() {
                bail!("telemetry is off; opt in with `graft telemetry on` first");
            }
            let n = send(home, &u)?;
            let _ = writeln!(out, "sent {n} event(s) to {u}");
        }
    }
    let json = matches!(args.action, Action::Status | Action::On | Action::Off) && args.json;
    Ok(Output::stdout(out).json_if(json, || state_json(home, url.as_deref())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_must_be_https_or_loopback() {
        for ok in [
            "https://t.example.com/v1",
            "http://localhost:8080/x",
            "http://127.0.0.1:9/x",
            "http://[::1]:9/",
        ] {
            assert!(check_endpoint(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://t.example.com/",
            "http://localhost.evil.com/",
            "http://127.0.0.1.evil.com/",
            "ftp://x",
            "localhost",
        ] {
            assert!(check_endpoint(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn durations_are_coarse() {
        assert_eq!(bucket(Duration::from_millis(5)), "<100ms");
        assert_eq!(bucket(Duration::from_millis(1500)), "<10s");
        assert_eq!(bucket(Duration::from_secs(600)), ">=1m");
    }

    #[test]
    fn an_event_holds_nothing_identifying() {
        let e = event("new", false, Some("not_found"), Duration::from_millis(20));
        let keys: Vec<&str> = e.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "arch", "code", "command", "day", "duration", "ok", "os", "v", "version", "via"
            ]
        );
        assert_eq!(e["day"].as_str().unwrap().len(), 10);
    }

    /// TLS works with the configured crypto provider and the OS trust
    /// store. Needs the network, so run it by hand:
    /// `cargo test -p graft-cli --bin graft tls -- --ignored`.
    #[test]
    #[ignore = "needs the network"]
    fn tls_handshake_works() {
        agent().get("https://example.com/").call().unwrap();
    }
}
