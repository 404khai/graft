//! Measuring the disk space a directory tree does not share with anything.

use std::io;
use std::path::Path;

use crate::sys;

/// Allocated bytes of a file or directory, from `st_blocks`.
fn allocated(meta: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.blocks().saturating_mul(512)
    }
    #[cfg(not(unix))]
    {
        meta.len()
    }
}

/// Bytes of disk used by `path` and everything under it that are not shared
/// with any other file: data blocks a reflink still shares do not count.
///
/// Regular files are measured with `FIEMAP` (Linux) or
/// `ATTR_CMNEXT_PRIVATESIZE` (macOS); where neither is available the
/// allocated size is used, which is exact on filesystems that cannot share
/// blocks. Directories and symlinks count their allocated size. Symlinks
/// are never followed.
///
/// # Errors
///
/// If any entry cannot be read.
pub fn unshared_bytes(path: &Path) -> io::Result<u64> {
    let meta = std::fs::symlink_metadata(path)?;
    let own = if meta.is_file() {
        match sys::unshared_data_bytes(path)? {
            Some(bytes) => bytes,
            None => allocated(&meta),
        }
    } else {
        allocated(&meta)
    };
    if !meta.is_dir() {
        return Ok(own);
    }
    let mut total = own;
    for entry in std::fs::read_dir(path)? {
        total = total.saturating_add(unshared_bytes(&entry?.path())?);
    }
    Ok(total)
}

/// Formats a byte count for people: `0 B`, `4.0 KiB`, `1.2 MiB`.
#[must_use]
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KiB", "MiB", "GiB", "TiB", "PiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    // Display only; precision loss above 2^53 bytes is irrelevant.
    #[allow(clippy::cast_precision_loss)]
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_units() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(1023), "1023 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(5 * 1024 * 1024 + 512 * 1024), "5.5 MiB");
        assert_eq!(human_bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    #[test]
    fn a_written_file_is_unshared() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f"), vec![7u8; 1 << 20]).unwrap();
        let n = unshared_bytes(dir.path()).unwrap();
        assert!(n >= 1 << 20, "{n}");
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let big = tempfile::tempdir().unwrap();
        std::fs::write(big.path().join("f"), vec![7u8; 1 << 20]).unwrap();
        std::os::unix::fs::symlink(big.path(), dir.path().join("link")).unwrap();
        assert!(unshared_bytes(dir.path()).unwrap() < 1 << 20);
    }
}
