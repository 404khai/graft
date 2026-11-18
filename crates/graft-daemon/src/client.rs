//! A small blocking client for `graftd`, used by the CLI (ADR 0007).

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use graft_core::{GraftHome, GraftName, RootId};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::paths;
use crate::protocol::{
    ActorParams, GraftRef, JSONRPC, MountInfo, Mounted, Ping, Request, Response, RootRef, RpcError,
    Stats, Swept, Unmounted,
};

/// Environment variable naming the `graftd` binary to start.
pub const GRAFTD_ENV: &str = "GRAFTD";

/// How long to wait for a freshly started daemon to answer.
const START_TIMEOUT: Duration = Duration::from_secs(5);

/// How long one call may take.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// Errors talking to `graftd`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ClientError {
    /// The socket could not be reached or the connection broke.
    #[error("talking to graftd at {}: {source}", socket.display())]
    Io {
        /// The socket path.
        socket: PathBuf,
        /// The underlying error.
        #[source]
        source: io::Error,
    },

    /// `graftd` could not be started.
    #[error("starting graftd ({}): {reason}", graftd.display())]
    Start {
        /// The binary that was run.
        graftd: PathBuf,
        /// Why it failed, with the end of its log when there is one.
        reason: String,
    },

    /// The daemon answered with an error.
    #[error("graftd: {}", .0.message)]
    Rpc(RpcError),

    /// The daemon's answer could not be understood.
    #[error("graftd sent an unexpected response: {0}")]
    Protocol(String),
}

/// A connection to `graftd`.
#[derive(Debug)]
pub struct Client {
    socket: PathBuf,
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
}

fn connect_at(socket: &Path) -> io::Result<Client> {
    let stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(CALL_TIMEOUT))?;
    let writer = stream.try_clone()?;
    Ok(Client {
        socket: socket.to_owned(),
        reader: BufReader::new(stream),
        writer,
        next_id: 1,
    })
}

/// The `graftd` binary to start: `$GRAFTD`, else next to the running
/// executable.
#[must_use]
pub fn graftd_binary() -> PathBuf {
    if let Some(p) = std::env::var_os(GRAFTD_ENV).filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("graftd")))
        .unwrap_or_else(|| PathBuf::from("graftd"))
}

/// Size at which `graftd.log` is moved to `graftd.log.1` when graftd
/// starts.
pub const MAX_LOG_BYTES: u64 = 8 * 1024 * 1024;

/// Moves `path` to `<path>.1` (replacing it) if it is at least `max` bytes.
/// Called only before starting graftd, when no daemon holds the file open.
/// Best effort: a failure leaves the log growing, which is logged.
pub fn rotate_log(path: &Path, max: u64) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() < max {
        return;
    }
    let mut old = path.as_os_str().to_owned();
    old.push(".1");
    if let Err(e) = std::fs::rename(path, &old) {
        tracing::warn!(path = %path.display(), error = %e, "cannot rotate graftd's log");
    }
}

fn log_tail(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<_> = text.lines().rev().take(5).collect();
    lines.into_iter().rev().collect::<Vec<_>>().join("\n")
}

/// The request ID sent with every call from this process.
static REQUEST_ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Sets the request ID every [`Client`] in this process sends. The first
/// call wins; later ones are ignored.
pub fn set_request_id(id: &graft_core::RequestId) {
    let _ = REQUEST_ID.set(id.to_string());
}

/// The request ID set by [`set_request_id`], to pass on to child `graft`
/// processes as `$GRAFT_REQUEST_ID`.
#[must_use]
pub fn request_id() -> Option<&'static str> {
    REQUEST_ID.get().map(String::as_str)
}

impl Client {
    /// Connects to the running daemon.
    ///
    /// # Errors
    ///
    /// If no daemon is listening.
    pub fn connect(home: &GraftHome) -> Result<Self, ClientError> {
        let socket = paths::socket(home);
        connect_at(&socket).map_err(|source| ClientError::Io { socket, source })
    }

    /// Connects to the daemon, starting `graftd` first if none is running.
    ///
    /// # Errors
    ///
    /// If the daemon cannot be started or does not answer in time.
    pub fn connect_or_start(home: &GraftHome) -> Result<Self, ClientError> {
        let socket = paths::socket(home);
        match connect_at(&socket) {
            Ok(c) => return Ok(c),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) => {}
            Err(source) => return Err(ClientError::Io { socket, source }),
        }
        // No socket but the lock held: a graftd is shutting down (it removes
        // its socket, unmounts, then releases the lock). A new one started
        // now would find the lock taken and exit, and the stopping one would
        // take its mounts with it; wait for it to finish first.
        let _ = Self::wait_until_stopped(home, STOPPING_WAIT);
        let graftd = graftd_binary();
        let start_err = |reason: String| ClientError::Start {
            graftd: graftd.clone(),
            reason,
        };
        let run = paths::run_dir(home);
        crate::server::ensure_run_dir(&run).map_err(|e| start_err(e.to_string()))?;
        let log_path = paths::log_file(home);
        rotate_log(&log_path, MAX_LOG_BYTES);
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| start_err(format!("opening {}: {e}", log_path.display())))?;
        let mut child = {
            use std::os::unix::process::CommandExt;
            Command::new(&graftd)
                .arg("--home")
                .arg(home.path())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log)
                // Its own process group: a Ctrl-C in the terminal that ran
                // `graft new` must not take the mounts down.
                .process_group(0)
                .spawn()
                .map_err(|e| start_err(e.to_string()))?
        };
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            match connect_at(&socket) {
                Ok(c) => return Ok(c),
                Err(e) if Instant::now() >= deadline => {
                    return Err(start_err(format!(
                        "no answer within {}s ({e}); see {}",
                        START_TIMEOUT.as_secs(),
                        log_path.display()
                    )));
                }
                Err(_) => {}
            }
            if let Ok(Some(status)) = child.try_wait() {
                // Lost a start race to another graftd: it may be up now.
                if let Ok(c) = connect_at(&socket) {
                    return Ok(c);
                }
                return Err(start_err(format!(
                    "exited with {status}:\n{}",
                    log_tail(&log_path)
                )));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// Calls `method` with `params` and decodes the result.
    ///
    /// # Errors
    ///
    /// I/O errors, error responses, and undecodable results.
    pub fn call<P: Serialize, T: DeserializeOwned>(
        &mut self,
        method: &str,
        params: &P,
    ) -> Result<T, ClientError> {
        let id = self.next_id;
        self.next_id += 1;
        let request = Request {
            jsonrpc: JSONRPC.to_owned(),
            id,
            method: method.to_owned(),
            params: serde_json::to_value(params)
                .map_err(|e| ClientError::Protocol(e.to_string()))?,
            request_id: REQUEST_ID.get().cloned(),
        };
        let io_err = |source| ClientError::Io {
            socket: self.socket.clone(),
            source,
        };
        let mut line =
            serde_json::to_vec(&request).map_err(|e| ClientError::Protocol(e.to_string()))?;
        line.push(b'\n');
        self.writer.write_all(&line).map_err(io_err)?;
        let mut answer = String::new();
        let n = self
            .reader
            .read_line(&mut answer)
            .map_err(|source| ClientError::Io {
                socket: self.socket.clone(),
                source,
            })?;
        if n == 0 {
            return Err(ClientError::Protocol(
                "the connection closed before an answer".to_owned(),
            ));
        }
        let response: Response =
            serde_json::from_str(&answer).map_err(|e| ClientError::Protocol(e.to_string()))?;
        if response.id != Some(id) {
            return Err(ClientError::Protocol(format!(
                "answer for request {:?}, expected {id}",
                response.id
            )));
        }
        if let Some(error) = response.error {
            return Err(ClientError::Rpc(error));
        }
        serde_json::from_value(response.result.unwrap_or(serde_json::Value::Null))
            .map_err(|e| ClientError::Protocol(e.to_string()))
    }

    /// `ping`.
    ///
    /// # Errors
    ///
    /// As for [`Client::call`].
    pub fn ping(&mut self) -> Result<Ping, ClientError> {
        self.call("ping", &())
    }

    /// Mounts a FUSE graft; returns its mount point.
    ///
    /// # Errors
    ///
    /// As for [`Client::call`].
    pub fn mount(&mut self, root: &RootId, graft: &GraftName) -> Result<PathBuf, ClientError> {
        let m: Mounted = self.call("mount", &graft_ref(root, graft))?;
        Ok(m.path)
    }

    /// Unmounts a FUSE graft.
    ///
    /// # Errors
    ///
    /// As for [`Client::call`].
    pub fn unmount(&mut self, root: &RootId, graft: &GraftName) -> Result<Unmounted, ClientError> {
        self.call("unmount", &graft_ref(root, graft))
    }

    /// Prunes a FUSE graft: unmounts it (lazily if busy) and removes its
    /// mount point, staging files, `.git`, delta and unreferenced blobs.
    ///
    /// # Errors
    ///
    /// As for [`Client::call`].
    pub fn prune(&mut self, root: &RootId, graft: &GraftName) -> Result<Unmounted, ClientError> {
        self.call("prune", &graft_ref(root, graft))
    }

    /// Sets the actor recorded on a mounted graft's following ops.
    ///
    /// # Errors
    ///
    /// As for [`Client::call`].
    pub fn set_actor(
        &mut self,
        root: &RootId,
        graft: &GraftName,
        actor: Option<&str>,
    ) -> Result<(), ClientError> {
        self.call(
            "set_actor",
            &ActorParams {
                root: root.to_string(),
                graft: graft.to_string(),
                actor: actor.map(str::to_owned),
            },
        )
    }

    /// Deletes Root `root`'s blobs that no graft references.
    ///
    /// # Errors
    ///
    /// As for [`Client::call`].
    pub fn gc(&mut self, root: &RootId) -> Result<Swept, ClientError> {
        self.call(
            "gc",
            &RootRef {
                root: root.to_string(),
            },
        )
    }

    /// Journals every write a mounted graft has received so far (NFS grafts
    /// journal a file only once it goes idle).
    ///
    /// # Errors
    ///
    /// As for [`Client::call`].
    pub fn sync(&mut self, root: &RootId, graft: &GraftName) -> Result<(), ClientError> {
        self.call("sync", &graft_ref(root, graft))
    }

    /// Every mounted graft.
    ///
    /// # Errors
    ///
    /// As for [`Client::call`].
    pub fn mounts(&mut self) -> Result<Vec<MountInfo>, ClientError> {
        self.call("mounts", &())
    }

    /// `stats`.
    ///
    /// # Errors
    ///
    /// As for [`Client::call`].
    pub fn stats(&mut self) -> Result<Stats, ClientError> {
        self.call("stats", &())
    }

    /// Asks the daemon to unmount everything and exit.
    ///
    /// # Errors
    ///
    /// As for [`Client::call`].
    pub fn shutdown(&mut self) -> Result<(), ClientError> {
        self.call("shutdown", &())
    }
}

/// How long [`Client::connect_or_start`] waits for a stopping graftd.
const STOPPING_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

impl Client {
    /// Waits up to `limit` until no graftd holds this home's lock, that is,
    /// until a graftd asked to shut down has unmounted everything and
    /// exited. Returns whether none is left running.
    #[must_use = "a graftd may still be running"]
    pub fn wait_until_stopped(home: &GraftHome, limit: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + limit;
        loop {
            if !lock_held(home) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}

/// Whether a graftd holds the lock of `home` (the file is only locked
/// while one runs).
fn lock_held(home: &GraftHome) -> bool {
    let Ok(file) = std::fs::OpenOptions::new()
        .write(true)
        .open(paths::lock_file(home))
    else {
        return false;
    };
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            false
        }
        Err(std::fs::TryLockError::WouldBlock) => true,
        Err(std::fs::TryLockError::Error(_)) => false,
    }
}

fn graft_ref(root: &RootId, graft: &GraftName) -> GraftRef {
    GraftRef {
        root: root.to_string(),
        graft: graft.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_only_a_log_at_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("graftd.log");
        let old = dir.path().join("graftd.log.1");
        std::fs::write(&log, "small").unwrap();
        rotate_log(&log, 10);
        assert!(log.exists() && !old.exists());

        std::fs::write(&old, "older").unwrap();
        std::fs::write(&log, "0123456789").unwrap();
        rotate_log(&log, 10);
        assert!(!log.exists());
        assert_eq!(std::fs::read_to_string(&old).unwrap(), "0123456789");

        rotate_log(&log, 10); // missing: nothing to do
    }
}
