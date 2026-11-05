//! Where `graftd` keeps its runtime files (`$GRAFT_HOME/run/`).
//!
//! The socket normally lives in `run/` too, but a Unix socket path is limited
//! to about 100 bytes (104 on macOS, 108 on Linux). When `$GRAFT_HOME` is
//! deep, the socket moves to `/tmp/graft-<uid>/<hash of $GRAFT_HOME>.sock`,
//! a directory only the user can access (checked by the daemon).

use std::path::{Path, PathBuf};

use graft_core::GraftHome;

/// Longest socket path used as is; longer ones move to [`short_socket`].
const MAX_SOCKET_PATH: usize = 100;

/// `$GRAFT_HOME/run`, mode `0700`.
#[must_use]
pub fn run_dir(home: &GraftHome) -> PathBuf {
    home.path().join("run")
}

/// The JSON-RPC socket.
#[must_use]
pub fn socket(home: &GraftHome) -> PathBuf {
    let usual = run_dir(home).join("graftd.sock");
    if graft_core::path_to_bytes(&usual).len() <= MAX_SOCKET_PATH {
        usual
    } else {
        short_socket(home)
    }
}

/// The socket for a `$GRAFT_HOME` too deep for `run/graftd.sock`.
#[must_use]
pub fn short_socket(home: &GraftHome) -> PathBuf {
    let hash = blake3::hash(&graft_core::path_to_bytes(home.path())).to_hex();
    socket_dir(home).join(format!("{}.sock", &hash[..16]))
}

/// `/tmp/graft-<uid>`, where `<uid>` owns `$GRAFT_HOME`.
#[must_use]
pub fn socket_dir(home: &GraftHome) -> PathBuf {
    PathBuf::from(format!("/tmp/graft-{}", owner(home.path())))
}

fn owner(path: &Path) -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).map_or(0, |m| m.uid())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_homes_get_a_short_socket() {
        let dir = tempfile::tempdir().unwrap();
        let home = GraftHome::from_path(dir.path().to_owned()).unwrap();
        assert_eq!(socket(&home), run_dir(&home).join("graftd.sock"));
        let deep = dir.path().join("d".repeat(120));
        std::fs::create_dir(&deep).unwrap();
        let deep = GraftHome::from_path(deep).unwrap();
        let s = socket(&deep);
        assert!(
            graft_core::path_to_bytes(&s).len() <= MAX_SOCKET_PATH,
            "{}",
            s.display()
        );
        assert!(s.starts_with(socket_dir(&deep)));
        // Stable, and distinct per home.
        assert_eq!(s, socket(&deep));
        let other = dir.path().join("e".repeat(120));
        std::fs::create_dir(&other).unwrap();
        assert_ne!(s, socket(&GraftHome::from_path(other).unwrap()));
    }
}
