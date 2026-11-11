//! Filesystem backends for grafts.
//!
//! [`FsBackend`] is how grafts are materialised. [`ReflinkBackend`] gives
//! each graft a real working tree whose unchanged files are reflinks of
//! Root's (ADR 0003). `VirtualBackend` serves grafts lazily from
//! Root's object store, with writes going to a private delta, through
//! [`view::GraftView`], the platform-independent tree model: over FUSE on
//! Linux (ADRs 0006, 0008) and NFS loopback on macOS (ADR 0009).
//! [`unshared_bytes`] measures what a graft really costs on disk.
//!
//! It also provides the host probing that `graft doctor` reports:
//!
//! - [`filesystem_type`]: which filesystem a path lives on.
//! - [`probe_reflink`]: whether that filesystem can clone files by reference
//!   (`FICLONE` on Linux, `clonefile(2)` on macOS), tested by actually
//!   cloning a scratch file rather than trusting the filesystem name.
//! - [`same_filesystem`]: reflinks only work within one filesystem.
//! - [`fuse_availability`] and [`nfs_client_availability`]: whether the
//!   virtual backends could run on this host.
//!
//! `graft-fs` is one of two crates allowed to contain `unsafe` (see
//! AGENTS.md §6). It is denied at the crate root and allowed only in the
//! private `sys` module, where every block carries a `SAFETY` comment.

#![deny(unsafe_code)]

mod backend;
mod capabilities;
mod changes;
mod clone;
mod fstype;
#[cfg(target_os = "linux")]
mod fuse;
mod lower;
#[cfg(target_os = "macos")]
mod nfs;
mod reflink;
#[allow(unsafe_code)] // FFI to statfs, clonefile and ioctl; see each SAFETY comment.
mod sys;
mod usage;
pub mod view;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod virtual_backend;
mod write;

pub use backend::{CreateReport, FsBackend, FsError, GraftSpec, ReflinkBackend, Result};
pub use capabilities::{Availability, fuse_availability, nfs_client_availability};
pub use changes::{
    ChangesError, RebasedFile, layer_mounts, rebase_ops, virtual_changes, virtual_diffs,
    virtual_disk_usage, without_ignored,
};
pub use clone::{CloneReport, ReflinkCloner, clone_tree};
pub use fstype::{FsType, available_bytes, filesystem_type, same_filesystem};
pub use reflink::{REFLINK_MECHANISM, probe_reflink};
pub use usage::{human_bytes, unshared_bytes};
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use virtual_backend::{Unmounted, VirtualBackend};
/// The virtual backend on Linux: grafts served over FUSE (ADR 0006).
#[cfg(target_os = "linux")]
pub type FuseBackend = VirtualBackend;
/// The virtual backend on macOS: grafts served over NFS loopback (ADR 0009).
#[cfg(target_os = "macos")]
pub type NfsBackend = VirtualBackend;
pub use view::Admission;
pub use write::{OpenMode, SetAttr};
