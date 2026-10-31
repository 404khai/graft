//! Process confinement for `graft run`.
//!
//! This crate will confine an agent process so it can only write inside its
//! own graft: Landlock and namespaces on Linux, a generated Seatbelt profile
//! on macOS.
//!
//! It is an empty stub. It is one of the two crates allowed to contain
//! `unsafe` (see AGENTS.md §6); until it needs any, `unsafe` is denied at the
//! crate root and must be allowed per module, each block with a `SAFETY`
//! comment.

#![deny(unsafe_code)]
