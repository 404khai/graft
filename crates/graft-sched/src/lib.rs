//! Scheduler: splitting a task into grafts that rarely collide (ADR 0018).
//!
//! [`graph::build`] reads a tree into a file dependency graph (imports and
//! the names used from them, with tree-sitter); [`plan::plan`] partitions a
//! task's [`plan::seeds`] along that graph's communities
//! ([`community`]: Louvain, then Leiden's connectivity refinement) into an
//! ordered DAG whose first node holds hub changes; [`admission::admit`]
//! decides whether a write may change symbols other grafts claim.

#![forbid(unsafe_code)]

pub mod admission;
pub mod analyze;
pub mod community;
pub mod graph;
pub mod plan;
