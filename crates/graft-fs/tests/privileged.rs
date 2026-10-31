//! Reflink detection on real filesystems mounted by a privileged CI job.
//!
//! Enabled with `--features privileged-tests` and configured with
//! `GRAFT_TEST_FS_MOUNTS`, a comma-separated list of `<fs>=<dir>` pairs where
//! `<dir>` is a writable directory on a filesystem of type `<fs>`:
//!
//! ```text
//! GRAFT_TEST_FS_MOUNTS=btrfs=/mnt/btrfs,xfs=/mnt/xfs,ext4=/mnt/ext4 \
//!     cargo test -p graft-fs --features privileged-tests --test privileged
//! ```
//!
//! `scripts/mount-test-filesystems.sh` prepares those mounts.

#![cfg(feature = "privileged-tests")]
#![allow(clippy::print_stderr)] // Probe results are useful context in CI logs.

use std::path::Path;

use graft_fs::{filesystem_type, probe_reflink};

/// Whether each filesystem must support reflinks, as created by
/// `scripts/mount-test-filesystems.sh` (XFS is formatted with `reflink=1`).
fn expected_reflink(fs: &str) -> bool {
    match fs {
        "apfs" | "btrfs" | "xfs" | "bcachefs" => true,
        "ext4" | "tmpfs" => false,
        other => panic!("no reflink expectation for {other}"),
    }
}

#[test]
fn reflink_detection_on_mounted_filesystems() {
    let mounts = std::env::var("GRAFT_TEST_FS_MOUNTS")
        .expect("privileged-tests needs GRAFT_TEST_FS_MOUNTS=<fs>=<dir>,...");
    let mut checked = 0;
    for entry in mounts.split(',').filter(|e| !e.is_empty()) {
        let (fs, dir) = entry
            .split_once('=')
            .unwrap_or_else(|| panic!("bad entry {entry:?}; expected <fs>=<dir>"));
        let dir = Path::new(dir);
        let detected = filesystem_type(dir).unwrap();
        assert_eq!(detected.name(), fs, "{} is not {fs}", dir.display());
        let supported = probe_reflink(dir).unwrap();
        assert_eq!(supported, expected_reflink(fs), "reflink probe on {fs}");
        eprintln!("{fs} at {}: reflink {supported}", dir.display());
        checked += 1;
    }
    assert!(checked > 0, "GRAFT_TEST_FS_MOUNTS listed no filesystems");
}
