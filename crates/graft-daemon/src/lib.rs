//! `graftd`, the Graft daemon, and the client the CLI uses to reach it.
//!
//! `graftd` owns every FUSE mount: a virtual graft lives as long as the
//! process serving it. It speaks newline-delimited JSON-RPC 2.0 on
//! `$GRAFT_HOME/run/graftd.sock` (ADR 0007), holds a lock so one daemon
//! runs per `$GRAFT_HOME`, and on start-up remounts every live FUSE graft
//! recorded in each Root's `graft.db`.
//!
//! - [`protocol`]: the messages.
//! - [`client`]: a blocking client that starts the daemon on demand.
//! - [`server`]: the daemon itself, generic over a [`server::Mounter`].
//! - [`mounter`]: the platform's mounter (FUSE on Linux).

#![forbid(unsafe_code)]

pub mod admission;
pub mod client;
pub mod mounter;
pub mod paths;
pub mod protocol;
pub mod server;

pub use client::{Client, ClientError};
