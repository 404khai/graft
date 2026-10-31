//! Filesystem backends for grafts.
//!
//! [`FsBackend`] is how grafts are materialised. [`ReflinkBackend`] gives
//! each graft a real working tree whose unchanged files are reflinks of
//! Root's (ADR 0003); FUSE and NFS backends arrive in later phases.
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
mod clone;
mod fstype;
mod reflink;
#[allow(unsafe_code)] // FFI to statfs, clonefile and ioctl; see each SAFETY comment.
mod sys;
mod usage;

pub use backend::{CreateReport, FsBackend, FsError, GraftSpec, ReflinkBackend, Result};
pub use capabilities::{Availability, fuse_availability, nfs_client_availability};
pub use clone::{CloneReport, ReflinkCloner, clone_tree};
pub use fstype::{FsType, filesystem_type, same_filesystem};
pub use reflink::{REFLINK_MECHANISM, probe_reflink};
pub use usage::{human_bytes, unshared_bytes};
