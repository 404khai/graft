//! Whether the virtual filesystem backends could run on this host.
//!
//! These checks only look for device nodes and helper binaries. They never
//! mount anything.

use std::path::{Path, PathBuf};

/// Whether a host facility is usable, with a human-readable explanation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    /// Usable. `detail` names what was found.
    Available {
        /// What was found, e.g. the helper binary's path.
        detail: String,
    },
    /// Not usable. `reason` says why and, where possible, how to fix it.
    Unavailable {
        /// Why it is unavailable.
        reason: String,
    },
}

impl Availability {
    /// Whether the facility is usable.
    #[must_use]
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }
}

fn available(detail: impl Into<String>) -> Availability {
    Availability::Available {
        detail: detail.into(),
    }
}

fn unavailable(reason: impl Into<String>) -> Availability {
    Availability::Unavailable {
        reason: reason.into(),
    }
}

/// Finds an executable named `name` in the directories of `$PATH`, then in
/// `extra` (sbin directories are often missing from a user's `$PATH`).
fn find_program(name: &str, extra: &[&str]) -> Option<PathBuf> {
    let path_dirs = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();
    path_dirs
        .into_iter()
        .chain(extra.iter().map(PathBuf::from))
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Whether FUSE mounts are possible.
///
/// On Linux this needs `/dev/fuse` and the `fusermount3` (or `fusermount`)
/// helper. On macOS FUSE needs the third-party macFUSE kernel extension;
/// Graft does not require it there and uses NFS loopback instead.
#[must_use]
pub fn fuse_availability() -> Availability {
    if cfg!(target_os = "linux") {
        if !Path::new("/dev/fuse").exists() {
            return unavailable("/dev/fuse is missing; is the fuse kernel module loaded?");
        }
        match ["fusermount3", "fusermount"]
            .iter()
            .find_map(|name| find_program(name, &["/usr/bin", "/bin"]))
        {
            Some(helper) => available(format!("/dev/fuse, {}", helper.display())),
            None => unavailable("/dev/fuse exists but fusermount3 was not found; install fuse3"),
        }
    } else if cfg!(target_os = "macos") {
        if Path::new("/Library/Filesystems/macfuse.fs").exists() {
            available("macFUSE installed; not needed, Graft uses NFS loopback on macOS")
        } else {
            unavailable("macFUSE not installed; not needed, Graft uses NFS loopback on macOS")
        }
    } else {
        unavailable("unsupported platform")
    }
}

/// Whether the system NFS client can mount a loopback NFS server.
#[must_use]
pub fn nfs_client_availability() -> Availability {
    let found = if cfg!(target_os = "macos") {
        find_program("mount_nfs", &["/sbin"])
    } else if cfg!(target_os = "linux") {
        find_program("mount.nfs", &["/sbin", "/usr/sbin"])
    } else {
        return unavailable("unsupported platform");
    };
    match found {
        Some(path) => available(path.display().to_string()),
        None if cfg!(target_os = "linux") => {
            unavailable("mount.nfs not found; install nfs-common or nfs-utils")
        }
        None => unavailable("mount_nfs not found"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_never_panic_and_explain_themselves() {
        for a in [fuse_availability(), nfs_client_availability()] {
            match a {
                Availability::Available { detail } => assert_ne!(detail, ""),
                Availability::Unavailable { reason } => assert_ne!(reason, ""),
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_ships_an_nfs_client() {
        assert!(nfs_client_availability().is_available());
    }

    #[cfg(unix)]
    #[test]
    fn finds_programs_in_extra_dirs() {
        assert!(find_program("sh", &["/bin"]).is_some());
        assert!(find_program("definitely-not-a-graft-binary", &[]).is_none());
    }
}
