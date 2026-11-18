//! Measuring the disk space a directory tree does not share with anything.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};

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
/// Directories are walked on several threads: a build tree has hundreds of
/// thousands of files, each needing its own system call.
///
/// # Errors
///
/// If any entry cannot be read.
pub fn unshared_bytes(path: &Path) -> io::Result<u64> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(8));
    unshared_bytes_with(path, threads)
}

/// One entry's own bytes, and whether it is a directory to descend into.
fn own_bytes(path: &Path) -> io::Result<(u64, bool)> {
    let meta = std::fs::symlink_metadata(path)?;
    let own = if meta.is_file() {
        match sys::unshared_data_bytes(path)? {
            Some(bytes) => bytes,
            None => allocated(&meta),
        }
    } else {
        allocated(&meta)
    };
    Ok((own, meta.is_dir()))
}

/// Shared state of a parallel walk.
struct Walk {
    /// Directories still to list, and how many workers are busy.
    queue: Mutex<(Vec<PathBuf>, usize)>,
    ready: Condvar,
    total: AtomicU64,
    error: Mutex<Option<io::Error>>,
}

impl Walk {
    /// The next directory to list, or `None` when the walk is over (no
    /// work left and nobody busy who could add more, or an error).
    fn next(&self) -> Option<PathBuf> {
        let mut q = lock(&self.queue);
        loop {
            if lock(&self.error).is_some() {
                return None;
            }
            if let Some(dir) = q.0.pop() {
                q.1 += 1;
                return Some(dir);
            }
            if q.1 == 0 {
                return None;
            }
            q = self.ready.wait(q).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Lists `dir`: adds its entries' bytes and queues its subdirectories.
    fn list(&self, dir: &Path) -> io::Result<()> {
        let mut found = Vec::new();
        let mut bytes = 0u64;
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            let (own, is_dir) = own_bytes(&path)?;
            bytes = bytes.saturating_add(own);
            if is_dir {
                found.push(path);
            }
        }
        self.total.fetch_add(bytes, Ordering::Relaxed);
        if !found.is_empty() {
            lock(&self.queue).0.extend(found);
            self.ready.notify_all();
        }
        Ok(())
    }

    fn work(&self) {
        while let Some(dir) = self.next() {
            let result = self.list(&dir);
            if let Err(e) = result {
                lock(&self.error).get_or_insert(e);
            }
            lock(&self.queue).1 -= 1;
            self.ready.notify_all();
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// [`unshared_bytes`] on `threads` threads.
fn unshared_bytes_with(path: &Path, threads: usize) -> io::Result<u64> {
    let (own, is_dir) = own_bytes(path)?;
    if !is_dir {
        return Ok(own);
    }
    let walk = Walk {
        queue: Mutex::new((vec![path.to_path_buf()], 0)),
        ready: Condvar::new(),
        total: AtomicU64::new(own),
        error: Mutex::new(None),
    };
    std::thread::scope(|s| {
        for _ in 1..threads.max(1) {
            s.spawn(|| walk.work());
        }
        walk.work();
    });
    if let Some(e) = walk
        .error
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner)
    {
        return Err(e);
    }
    Ok(walk.total.into_inner())
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

    #[test]
    fn parallel_and_serial_walks_agree() {
        let dir = tempfile::tempdir().unwrap();
        for d in 0..20 {
            let sub = dir.path().join(format!("d{d}/e{}", d % 3));
            std::fs::create_dir_all(&sub).unwrap();
            for f in 0..15 {
                std::fs::write(sub.join(format!("f{f}")), vec![7u8; 5000 * (f + 1)]).unwrap();
            }
        }
        std::os::unix::fs::symlink("/", dir.path().join("d0/up")).unwrap();
        let serial = unshared_bytes_with(dir.path(), 1).unwrap();
        assert!(serial >= 20 * 15 * 5000, "{serial}");
        for threads in [2, 4, 8, 32] {
            assert_eq!(
                unshared_bytes_with(dir.path(), threads).unwrap(),
                serial,
                "{threads} threads"
            );
        }
    }

    #[test]
    fn a_missing_tree_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(unshared_bytes_with(&dir.path().join("gone"), 4).is_err());
    }
}
