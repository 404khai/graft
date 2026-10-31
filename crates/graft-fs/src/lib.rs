//! Filesystem backends for grafts.
//!
//! This crate will hold the `FsBackend` trait and its implementations
//! (reflink, FUSE, NFS loopback). For now it provides the host probing that
//! `graft doctor` reports and that backend selection will rely on:
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

mod capabilities;
mod fstype;
mod reflink;
#[allow(unsafe_code)] // FFI to statfs, clonefile and ioctl; see each SAFETY comment.
mod sys;

pub use capabilities::{Availability, fuse_availability, nfs_client_availability};
pub use fstype::{FsType, filesystem_type, same_filesystem};
pub use reflink::{REFLINK_MECHANISM, probe_reflink};
