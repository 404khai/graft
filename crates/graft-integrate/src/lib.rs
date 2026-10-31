//! Integrate: landing grafts into Root.
//!
//! This crate will own the persistent integrate queue: rebase a graft's
//! delta onto Root, merge structurally (Mergiraf as an external merge
//! driver), run the test gate and produce ordinary git commits.
//!
//! It is an empty stub.

#![forbid(unsafe_code)]
