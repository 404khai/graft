//! Detecting reflink support by cloning a scratch file.

use std::io;
use std::path::Path;

use crate::sys;

/// The system call used for reflinks on this platform, or `None` if Graft
/// has no reflink implementation here.
pub const REFLINK_MECHANISM: Option<&str> = if cfg!(target_os = "linux") {
    Some("FICLONE")
} else if cfg!(target_os = "macos") {
    Some("clonefile")
} else {
    None
};

const PROBE_CONTENTS: &[u8] = b"graft reflink probe\n";

/// Tests whether the filesystem containing `dir` supports reflinks.
///
/// Creates a hidden scratch directory inside `dir`, writes a small file,
/// tries to reflink it, and removes everything again. Probing is used
/// instead of trusting the filesystem type because support can depend on how
/// a filesystem was created (XFS `reflink=1`, ZFS block cloning).
///
/// Returns `Ok(false)` when the filesystem reports that it cannot reflink.
///
/// # Errors
///
/// Returns an error if the scratch files cannot be created (for example,
/// `dir` is read-only) or the clone fails for a reason other than lack of
/// support.
pub fn probe_reflink(dir: &Path) -> io::Result<bool> {
    let scratch = tempfile::Builder::new()
        .prefix(".graft-reflink-probe-")
        .tempdir_in(dir)?;
    let src = scratch.path().join("src");
    let dst = scratch.path().join("dst");
    std::fs::write(&src, PROBE_CONTENTS)?;

    let supported = match sys::reflink(&src, &dst) {
        Ok(()) => {
            if std::fs::read(&dst)? != PROBE_CONTENTS {
                return Err(io::Error::other("reflinked file has different contents"));
            }
            true
        }
        Err(e) if sys::is_unsupported(&e) => {
            tracing::debug!(dir = %dir.display(), error = %e, "reflink not supported");
            false
        }
        Err(e) => return Err(e),
    };

    let scratch_path = scratch.path().to_owned();
    if let Err(e) = scratch.close() {
        tracing::warn!(path = %scratch_path.display(), error = %e, "could not remove reflink probe");
    }
    Ok(supported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_fails_cleanly_on_a_missing_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(probe_reflink(&dir.path().join("missing")).is_err());
    }
}
