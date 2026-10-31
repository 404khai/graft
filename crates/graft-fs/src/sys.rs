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
    // clonefile copies permissions; FICLONE only copies data.
    dst_file.set_permissions(src_file.metadata()?.permissions())?;
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

/// Bytes of `path`'s data that no other file shares, or `None` if this
/// filesystem cannot tell (the caller falls back to allocated size).
///
/// Uses `FIEMAP`: every extent not flagged `FIEMAP_EXTENT_SHARED` counts.
#[cfg(target_os = "linux")]
pub(crate) fn unshared_data_bytes(path: &Path) -> io::Result<Option<u64>> {
    use std::os::fd::AsRawFd;

    const FIEMAP_FLAG_SYNC: u32 = 0x1;
    const FIEMAP_EXTENT_LAST: u32 = 0x1;
    const FIEMAP_EXTENT_SHARED: u32 = 0x2000;
    const EXTENTS: usize = 128;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Extent {
        logical: u64,
        physical: u64,
        length: u64,
        reserved64: [u64; 2],
        flags: u32,
        reserved: [u32; 3],
    }
    #[repr(C)]
    struct Header {
        start: u64,
        length: u64,
        flags: u32,
        mapped_extents: u32,
        extent_count: u32,
        reserved: u32,
    }
    #[repr(C)]
    struct Request {
        header: Header,
        extents: [Extent; EXTENTS],
    }
    // The request code encodes the size of the header only (struct fiemap).
    const FS_IOC_FIEMAP: libc::Ioctl = libc::_IOWR::<Header>(b'f' as u32, 11);

    let file = std::fs::File::open(path)?;
    let zero = Extent {
        logical: 0,
        physical: 0,
        length: 0,
        reserved64: [0; 2],
        flags: 0,
        reserved: [0; 3],
    };
    let mut request = Request {
        header: Header {
            start: 0,
            length: u64::MAX,
            flags: FIEMAP_FLAG_SYNC,
            mapped_extents: 0,
            extent_count: 0,
            reserved: 0,
        },
        extents: [zero; EXTENTS],
    };
    let mut unshared = 0u64;
    let mut start = 0u64;
    loop {
        request.header.start = start;
        request.header.length = u64::MAX - start;
        request.header.mapped_extents = 0;
        #[allow(clippy::cast_possible_truncation)] // EXTENTS is 128
        {
            request.header.extent_count = EXTENTS as u32;
        }
        // SAFETY: `request` is a properly laid out `struct fiemap` followed
        // by room for `extent_count` extents, valid for reads and writes for
        // the duration of the call; the descriptor is open.
        let rc = unsafe { libc::ioctl(file.as_raw_fd(), FS_IOC_FIEMAP, &raw mut request) };
        if rc != 0 {
            let err = io::Error::last_os_error();
            return match err.raw_os_error() {
                Some(libc::EOPNOTSUPP | libc::ENOTTY | libc::EINVAL) => Ok(None),
                _ => Err(err),
            };
        }
        let mapped = (request.header.mapped_extents as usize).min(EXTENTS);
        if mapped == 0 {
            return Ok(Some(unshared));
        }
        for extent in &request.extents[..mapped] {
            if extent.flags & FIEMAP_EXTENT_SHARED == 0 {
                unshared = unshared.saturating_add(extent.length);
            }
            if extent.flags & FIEMAP_EXTENT_LAST != 0 {
                return Ok(Some(unshared));
            }
        }
        let last = request.extents[mapped - 1];
        start = last.logical.saturating_add(last.length);
    }
}

/// Bytes of `path`'s data that no other file shares, or `None` if this
/// filesystem cannot tell (the caller falls back to allocated size).
///
/// Uses APFS's `ATTR_CMNEXT_PRIVATESIZE` through `getattrlist(2)`.
#[cfg(target_os = "macos")]
pub(crate) fn unshared_data_bytes(path: &Path) -> io::Result<Option<u64>> {
    #[repr(C, packed(4))]
    struct Reply {
        length: u32,
        returned: libc::attribute_set_t,
        private_size: libc::off_t,
    }
    let c = c_path(path)?;
    let mut request = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: libc::ATTR_CMN_RETURNED_ATTRS,
        volattr: 0,
        dirattr: 0,
        fileattr: 0,
        forkattr: libc::ATTR_CMNEXT_PRIVATESIZE,
    };
    let mut reply = std::mem::MaybeUninit::<Reply>::zeroed();
    // SAFETY: `c` is NUL-terminated; `request` is a valid attrlist; `reply`
    // is writable for `size_of::<Reply>()` bytes, which is the size passed.
    let rc = unsafe {
        libc::getattrlist(
            c.as_ptr(),
            (&raw mut request).cast(),
            reply.as_mut_ptr().cast(),
            std::mem::size_of::<Reply>(),
            libc::FSOPT_ATTR_CMN_EXTENDED | libc::FSOPT_NOFOLLOW,
        )
    };
    if rc != 0 {
        let err = io::Error::last_os_error();
        return match err.raw_os_error() {
            Some(libc::ENOTSUP | libc::EINVAL) => Ok(None),
            _ => Err(err),
        };
    }
    // SAFETY: getattrlist succeeded, and the reply was zero-initialised, so
    // every field holds a valid value even if the kernel wrote fewer bytes.
    let reply = unsafe { reply.assume_init() };
    let returned = reply.returned.forkattr;
    if returned & libc::ATTR_CMNEXT_PRIVATESIZE == 0 {
        return Ok(None);
    }
    let size = reply.private_size;
    Ok(Some(u64::try_from(size).unwrap_or(0)))
}

/// Unshared sizes are not implemented on other platforms.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn unshared_data_bytes(_path: &Path) -> io::Result<Option<u64>> {
    Ok(None)
}
