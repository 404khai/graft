//! The reflink probe agrees with what is known about the filesystem it runs
//! on. In CI this covers APFS (macOS runners) and ext4 (Ubuntu runners);
//! `privileged.rs` covers btrfs and XFS on loop mounts.

#![allow(clippy::print_stderr)] // Probe results are useful context in CI logs.

use graft_fs::{filesystem_type, probe_reflink};

#[test]
fn probe_matches_the_filesystem_under_test() {
    let dir = tempfile::tempdir().unwrap();
    let fs = filesystem_type(dir.path()).unwrap();
    let supported = probe_reflink(dir.path()).unwrap();
    eprintln!(
        "tempdir {} is {fs}; reflink supported: {supported}",
        dir.path().display()
    );
    if let Some(expected) = fs.reflink_expected() {
        assert_eq!(supported, expected, "reflink probe disagrees on {fs}");
    }
}

#[test]
fn probe_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    probe_reflink(dir.path()).unwrap();
    let leftovers: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert!(leftovers.is_empty(), "probe left files: {leftovers:?}");
}

#[cfg(unix)]
#[test]
fn probe_reports_an_error_for_a_read_only_directory() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let ro = dir.path().join("ro");
    std::fs::create_dir(&ro).unwrap();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = probe_reflink(&ro);
    // Root ignores directory permissions; there is nothing to test then.
    let ignores_permissions = std::fs::write(ro.join("canary"), b"").is_ok();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
    if !ignores_permissions {
        assert!(result.is_err(), "probe should fail in a read-only dir");
    }
}
