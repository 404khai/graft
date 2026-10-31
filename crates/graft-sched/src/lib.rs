//! Scheduler: splitting a task into grafts that rarely collide.
//!
//! This crate will build a file- and symbol-level dependency graph, partition
//! it into cohesive nodes, order them so interfaces land first, and enforce
//! symbol claims at write time.
//!
//! It is an empty stub.

#![forbid(unsafe_code)]
