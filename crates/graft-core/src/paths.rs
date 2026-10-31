//! Lossless conversion between paths and bytes, for persisting paths.
//!
//! Paths are stored as their raw OS bytes, never as lossy UTF-8, so a
//! non-UTF-8 path round-trips exactly.

use std::path::{Path, PathBuf};

/// The bytes of a path, exactly as the OS stores them.
#[must_use]
pub fn path_to_bytes(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        path.to_string_lossy().into_owned().into_bytes()
    }
}

/// The inverse of [`path_to_bytes`].
#[must_use]
pub fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let p = Path::new("/srv/repo with spaces/ünï");
        assert_eq!(path_from_bytes(path_to_bytes(p)), p);
    }

    #[cfg(unix)]
    #[test]
    fn round_trips_non_utf8() {
        use std::os::unix::ffi::OsStrExt;
        let p = Path::new(std::ffi::OsStr::from_bytes(b"/srv/\xff\xfe"));
        assert_eq!(path_to_bytes(p), b"/srv/\xff\xfe");
        assert_eq!(path_from_bytes(path_to_bytes(p)), p);
    }
}
