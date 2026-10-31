//! Root access for Graft.
//!
//! A Root is the main repository: its git object store plus the layer cache
//! of shared, content-addressed directory trees (for example `node_modules`).
//! This crate will open repositories with `gix`, derive `RootId`s, resolve
//! refs and commits lazily, and own the `LayerCache`.
//!
//! It is an empty stub until the Root registry lands.

#![forbid(unsafe_code)]
