//! Thin, platform-specific FFI wrappers. All `unsafe` in the crate lives here.

use std::io;
use std::path::Path;

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn c_path(path: &Path) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    Ok(std::ffi::CString::new(path.as_os_str().as_bytes())?)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn statfs(path: &Path) -> io::Result<libc::statfs> {
    let c = c_path(path)?;
    let mut buf = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: `c` is a valid NUL-terminated string that outlives the call,
    // and `buf` is valid for writes of one `libc::statfs`.
    let rc = unsafe { libc::statfs(c.as_ptr(), buf.as_mut_ptr()) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: statfs returned 0, which guarantees it filled `buf`.
    Ok(unsafe { buf.assume_init() })
}

/// The Linux `statfs` magic number for the filesystem containing `path`.
#[cfg(target_os = "linux")]
pub(crate) fn fs_magic(path: &Path) -> io::Result<u32> {
    let st = statfs(path)?;
    // `f_type` is `c_long`, `c_uint` or `__fsword_t` depending on the target.
    // Every magic number is defined as 32 bits, so keeping the low 32 bits
    // is exact and also undoes sign extension on targets where it is signed.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(st.f_type as u32)
}

/// The macOS filesystem type name (`f_fstypename`) for `path`, e.g. `apfs`.
#[cfg(target_os = "macos")]
pub(crate) fn fs_type_name(path: &Path) -> io::Result<String> {
    let st = statfs(path)?;
    // `c_char` is `i8` here; reinterpret each byte rather than value-convert.
    #[allow(clippy::cast_sign_loss)]
    let bytes: Vec<u8> = st
        .f_fstypename
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Clones `src` to `dst` (which must not exist) sharing data blocks.
#[cfg(target_os = "macos")]
pub(crate) fn reflink(src: &Path, dst: &Path) -> io::Result<()> {
    let s = c_path(src)?;
    let d = c_path(dst)?;
    // SAFETY: both pointers are valid NUL-terminated strings for the whole
    // call, and clonefile does not retain them.
    let rc = unsafe { libc::clonefile(s.as_ptr(), d.as_ptr(), 0) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Clones `src` to `dst` (which must not exist) sharing data blocks.
#[cfg(target_os = "linux")]
pub(crate) fn reflink(src: &Path, dst: &Path) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    let src_file = std::fs::File::open(src)?;
    let dst_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)?;
    // SAFETY: both descriptors are open for the duration of the call (the
    // `File`s are dropped afterwards), and FICLONE takes the source
    // descriptor as its integer argument without retaining it.
    let rc = unsafe { libc::ioctl(dst_file.as_raw_fd(), libc::FICLONE, src_file.as_raw_fd()) };
    if rc != 0 {
        let err = io::Error::last_os_error();
        drop(dst_file);
        // Best effort: leave no empty destination behind.
        let _ = std::fs::remove_file(dst);
        return Err(err);
    }
    Ok(())
}

/// Reflinks are not implemented on other platforms.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn reflink(_src: &Path, _dst: &Path) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

/// Whether `err` means "this filesystem cannot reflink", as opposed to a
/// real failure such as a permissions problem.
pub(crate) fn is_unsupported(err: &io::Error) -> bool {
    if err.kind() == io::ErrorKind::Unsupported {
        return true;
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        // EOPNOTSUPP/ENOTSUP: filesystem lacks the feature (ext4, HFS+).
        // EINVAL/ENOTTY: filesystem does not implement the ioctl at all.
        // EXDEV: source and destination on different filesystems.
        // ENOSYS: kernel lacks the call.
        // (EOPNOTSUPP and ENOTSUP are the same value on Linux, not on macOS.)
        const UNSUPPORTED: [i32; 6] = [
            libc::EOPNOTSUPP,
            libc::ENOTSUP,
            libc::EINVAL,
            libc::ENOTTY,
            libc::EXDEV,
            libc::ENOSYS,
        ];
        err.raw_os_error()
            .is_some_and(|code| UNSUPPORTED.contains(&code))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    false
}
