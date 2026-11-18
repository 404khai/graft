//! Opt-in telemetry (ADR 0034): off by default, inspectable, and sent only
//! to a configured endpoint.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use predicates::prelude::*;
use serde_json::Value;

fn graft(home: &Path) -> assert_cmd::Command {
    let mut cmd = crate::graft(home);
    cmd.env_remove("DO_NOT_TRACK").env_remove("GRAFT_TELEMETRY");
    cmd
}

fn queue(home: &Path) -> Vec<Value> {
    std::fs::read_to_string(home.join("telemetry/queue.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// A one-shot HTTP server on loopback answering `status`; sends each
/// request body it receives.
fn server(status: u16) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://127.0.0.1:{}/v1/events",
        listener.local_addr().unwrap().port()
    );
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; len];
            reader.read_exact(&mut body).unwrap();
            let _ = tx.send(String::from_utf8(body).unwrap());
            let mut s = stream;
            let _ = write!(
                s,
                "HTTP/1.1 {status} X\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
            );
        }
    });
    (url, rx)
}

fn set_endpoint(home: &Path, url: &str) {
    std::fs::write(
        home.join("config.toml"),
        format!("[telemetry]\nendpoint = \"{url}\"\n"),
    )
    .unwrap();
}

#[test]
fn off_by_default_records_nothing() {
    let home = tempfile::tempdir().unwrap();
    graft(home.path()).arg("doctor").assert().success();
    assert!(!home.path().join("telemetry").exists());
    graft(home.path())
        .args(["telemetry", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("off (opt in"));
}

#[test]
fn opted_in_events_hold_no_paths_and_stay_local_without_an_endpoint() {
    let home = tempfile::tempdir().unwrap();
    graft(home.path())
        .args(["telemetry", "on"])
        .assert()
        .success()
        .stdout(predicate::str::contains("events stay on this machine"));
    let repo = crate::fake_repo();
    graft(home.path())
        .arg("doctor")
        .arg(repo.path())
        .assert()
        .success();
    graft(home.path())
        .args(["path", "nope"])
        .current_dir(repo.path())
        .assert()
        .failure();

    let events = queue(home.path());
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(events[0]["command"], "doctor");
    assert_eq!(events[0]["ok"], true);
    assert_eq!(events[1]["command"], "path");
    assert_eq!(events[1]["ok"], false);
    assert!(events[1]["code"].is_string());
    let text = std::fs::read_to_string(home.path().join("telemetry/queue.jsonl")).unwrap();
    for private in [
        home.path().to_str().unwrap(),
        repo.path().to_str().unwrap(),
        "nope",
    ] {
        assert!(
            !text.contains(private),
            "{private} reached telemetry: {text}"
        );
    }
    // Telemetry's own commands are not recorded; show prints the queue.
    graft(home.path())
        .args(["telemetry", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"command\":\"doctor\""));
    assert_eq!(queue(home.path()).len(), 2);
    graft(home.path())
        .args(["telemetry", "send"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no [telemetry] endpoint"));
}

#[test]
fn do_not_track_overrides_consent() {
    let home = tempfile::tempdir().unwrap();
    graft(home.path())
        .args(["telemetry", "on"])
        .assert()
        .success();
    for (var, value) in [("DO_NOT_TRACK", "1"), ("GRAFT_TELEMETRY", "off")] {
        graft(home.path())
            .arg("doctor")
            .env(var, value)
            .assert()
            .success();
        graft(home.path())
            .args(["telemetry", "status"])
            .env(var, value)
            .assert()
            .stdout(predicate::str::contains("off for this process"));
    }
    assert_eq!(queue(home.path()), Vec::<Value>::new());
}

#[test]
fn sends_to_the_endpoint_and_keeps_events_when_it_fails() {
    let home = tempfile::tempdir().unwrap();
    graft(home.path())
        .args(["telemetry", "on"])
        .assert()
        .success();
    graft(home.path()).arg("doctor").assert().success();

    let (bad, _) = server(500);
    set_endpoint(home.path(), &bad);
    graft(home.path())
        .args(["telemetry", "send"])
        .assert()
        .failure();
    assert_eq!(
        queue(home.path()).len(),
        1,
        "a failed send keeps the events"
    );

    let (good, bodies) = server(204);
    set_endpoint(home.path(), &good);
    graft(home.path())
        .args(["telemetry", "send"])
        .assert()
        .success()
        .stdout(predicate::str::contains("sent 1 event(s)"));
    let body: Value =
        serde_json::from_str(&bodies.recv_timeout(Duration::from_secs(5)).unwrap()).unwrap();
    assert_eq!(body["schema"], 1);
    assert_eq!(body["events"][0]["command"], "doctor");
    assert_eq!(queue(home.path()), Vec::<Value>::new());

    // Opting out deletes the queue.
    graft(home.path()).arg("doctor").assert().success();
    graft(home.path())
        .args(["telemetry", "off"])
        .assert()
        .success();
    assert_eq!(queue(home.path()), Vec::<Value>::new());
    graft(home.path()).arg("doctor").assert().success();
    assert_eq!(queue(home.path()), Vec::<Value>::new());
}

#[test]
fn a_command_starts_a_background_send_when_one_is_due() {
    let home = tempfile::tempdir().unwrap();
    let (url, bodies) = server(204);
    graft(home.path())
        .args(["telemetry", "on"])
        .assert()
        .success();
    set_endpoint(home.path(), &url);
    graft(home.path()).arg("doctor").assert().success();
    let body: Value =
        serde_json::from_str(&bodies.recv_timeout(Duration::from_secs(15)).unwrap()).unwrap();
    assert_eq!(body["events"][0]["command"], "doctor");
    // Not again within the day.
    graft(home.path()).arg("doctor").assert().success();
    assert!(bodies.recv_timeout(Duration::from_secs(2)).is_err());
}

#[test]
fn refuses_a_plain_http_endpoint() {
    let home = tempfile::tempdir().unwrap();
    set_endpoint(home.path(), "http://telemetry.example.com/v1");
    graft(home.path())
        .args(["telemetry", "on"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("must use https"));
}
