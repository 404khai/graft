//! Identifying the filesystem a path lives on.

use std::fmt;
use std::io;
use std::path::Path;

/// A filesystem type, as reported by the kernel.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FsType {
    /// Apple File System.
    Apfs,
    /// HFS+ (macOS, legacy).
    Hfs,
    /// Btrfs.
    Btrfs,
    /// XFS.
    Xfs,
    /// The ext family. ext2, ext3 and ext4 share one magic number, so this
    /// variant covers all three.
    Ext4,
    /// ZFS.
    Zfs,
    /// bcachefs.
    Bcachefs,
    /// tmpfs (Linux, in memory).
    Tmpfs,
    /// overlayfs (common inside containers).
    Overlay,
    /// An NFS mount.
    Nfs,
    /// An SMB/CIFS mount.
    Smb,
    /// A FUSE mount.
    Fuse,
    /// Anything else: the macOS type name, or the Linux magic number in hex.
    Other(String),
}

impl FsType {
    /// The lowercase name shown to users.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Apfs => "apfs",
            Self::Hfs => "hfs",
            Self::Btrfs => "btrfs",
            Self::Xfs => "xfs",
            Self::Ext4 => "ext4",
            Self::Zfs => "zfs",
            Self::Bcachefs => "bcachefs",
            Self::Tmpfs => "tmpfs",
            Self::Overlay => "overlay",
            Self::Nfs => "nfs",
            Self::Smb => "smb",
            Self::Fuse => "fuse",
            Self::Other(name) => name,
        }
    }

    /// Whether this filesystem type always (`Some(true)`) or never
    /// (`Some(false)`) supports reflinks. `None` means it depends on how the
    /// filesystem was created or configured, so only a probe can tell: XFS
    /// needs `reflink=1`, ZFS needs block cloning enabled.
    #[must_use]
    pub fn reflink_expected(&self) -> Option<bool> {
        match self {
            Self::Apfs | Self::Btrfs | Self::Bcachefs => Some(true),
            Self::Ext4 | Self::Hfs | Self::Tmpfs => Some(false),
            _ => None,
        }
    }

    /// Maps a Linux `statfs` magic number (see `statfs(2)`).
    #[must_use]
    pub fn from_linux_magic(magic: u32) -> Self {
        match magic {
            0x9123_683E => Self::Btrfs,
            0x5846_5342 => Self::Xfs,
            0x0000_EF53 => Self::Ext4,
            0x2FC1_2FC1 => Self::Zfs,
            0xCA45_1A4E => Self::Bcachefs,
            0x0102_1994 => Self::Tmpfs,
            0x794C_7630 => Self::Overlay,
            0x0000_6969 => Self::Nfs,
            0xFF53_4D42 | 0xFE53_4D42 => Self::Smb,
            0x6573_5546 => Self::Fuse,
            other => Self::Other(format!("0x{other:x}")),
        }
    }

    /// Maps a macOS `f_fstypename`.
    #[must_use]
    pub fn from_macos_name(name: &str) -> Self {
        match name {
            "apfs" => Self::Apfs,
            "hfs" => Self::Hfs,
            "zfs" => Self::Zfs,
            "nfs" => Self::Nfs,
            "smbfs" => Self::Smb,
            "macfuse" | "osxfuse" | "fusefs" => Self::Fuse,
            other => Self::Other(other.to_owned()),
        }
    }
}

impl fmt::Display for FsType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Returns the type of the filesystem containing `path`.
///
/// # Errors
///
/// Returns the OS error if `path` does not exist or cannot be queried, and
/// [`io::ErrorKind::Unsupported`] on platforms other than Linux and macOS.
pub fn filesystem_type(path: &Path) -> io::Result<FsType> {
    #[cfg(target_os = "linux")]
    {
        crate::sys::fs_magic(path).map(FsType::from_linux_magic)
    }
    #[cfg(target_os = "macos")]
    {
        crate::sys::fs_type_name(path).map(|name| FsType::from_macos_name(&name))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = path;
        Err(io::ErrorKind::Unsupported.into())
    }
}

/// Bytes available to unprivileged users on the filesystem containing
/// `path`, as `df` reports it.
///
/// # Errors
///
/// Returns the OS error if `path` cannot be queried, and
/// [`io::ErrorKind::Unsupported`] on platforms other than Linux and macOS.
pub fn available_bytes(path: &Path) -> io::Result<u64> {
    crate::sys::available_bytes(path)
}

/// Whether `a` and `b` are on the same filesystem (same device ID).
///
/// # Errors
///
/// Returns the OS error if either path cannot be queried.
pub fn same_filesystem(a: &Path, b: &Path) -> io::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(std::fs::metadata(a)?.dev() == std::fs::metadata(b)?.dev())
    }
    #[cfg(not(unix))]
    {
        let _ = (a, b);
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_magic_numbers() {
        assert_eq!(FsType::from_linux_magic(0x9123_683E), FsType::Btrfs);
        assert_eq!(FsType::from_linux_magic(0xEF53), FsType::Ext4);
        assert_eq!(FsType::from_linux_magic(0x5846_5342), FsType::Xfs);
        assert_eq!(
            FsType::from_linux_magic(0x1234),
            FsType::Other("0x1234".into())
        );
    }

    #[test]
    fn macos_names() {
        assert_eq!(FsType::from_macos_name("apfs"), FsType::Apfs);
        assert_eq!(FsType::from_macos_name("macfuse"), FsType::Fuse);
        assert_eq!(FsType::from_macos_name("devfs").name(), "devfs");
    }

    #[test]
    fn reflink_expectations() {
        assert_eq!(FsType::Apfs.reflink_expected(), Some(true));
        assert_eq!(FsType::Btrfs.reflink_expected(), Some(true));
        assert_eq!(FsType::Ext4.reflink_expected(), Some(false));
        assert_eq!(FsType::Xfs.reflink_expected(), None);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn detects_a_real_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let fs = filesystem_type(dir.path()).unwrap();
        assert_ne!(fs.name(), "");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn reports_available_space() {
        let dir = tempfile::tempdir().unwrap();
        assert!(available_bytes(dir.path()).unwrap() > 0);
        assert!(available_bytes(&dir.path().join("missing")).is_err());
    }

    #[test]
    fn missing_path_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(filesystem_type(&dir.path().join("missing")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_is_on_its_own_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        assert!(same_filesystem(dir.path(), &sub).unwrap());
    }
}
