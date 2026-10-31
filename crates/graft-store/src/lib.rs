//! Durable state for Graft.
//!
//! This crate will own the per-Root SQLite database (`graft.db`: grafts, op
//! log, claims, integrate queue), the content-addressed delta blob store, and
//! the append-only op log that journals every write, rename and delete in a
//! graft.
//!
//! It is an empty stub until the Root registry lands.

#![forbid(unsafe_code)]
