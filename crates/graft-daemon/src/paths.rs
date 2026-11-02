//! Where `graftd` keeps its runtime files (`$GRAFT_HOME/run/`).

use std::path::PathBuf;

use graft_core::GraftHome;

/// `$GRAFT_HOME/run`, mode `0700`.
#[must_use]
pub fn run_dir(home: &GraftHome) -> PathBuf {
    home.path().join("run")
}

/// The JSON-RPC socket.
#[must_use]
pub fn socket(home: &GraftHome) -> PathBuf {
    run_dir(home).join("graftd.sock")
}

/// The single-instance lock.
#[must_use]
pub fn lock_file(home: &GraftHome) -> PathBuf {
    run_dir(home).join("graftd.lock")
}

/// Where the CLI sends the daemon's stderr when it starts it.
#[must_use]
pub fn log_file(home: &GraftHome) -> PathBuf {
    run_dir(home).join("graftd.log")
}
