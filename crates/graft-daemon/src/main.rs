//! `graftd`, the Graft daemon.
//!
//! `graftd` will own every mount, serve the JSON-RPC API on a Unix socket for
//! the CLI and the MCP server, and feed op log events to Radar. The CLI will
//! start it on demand.
//!
//! It is a stub: it parses `--version` and `--help` and otherwise exits with
//! an error, so nothing comes to depend on a daemon that does not exist yet.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use clap::Parser;

/// The Graft daemon. Not implemented yet.
#[derive(Debug, Parser)]
#[command(name = "graftd", version, about)]
struct Args {}

fn main() -> ExitCode {
    let Args {} = Args::parse();
    // A binary reporting its own failure; there is no logger to route through yet.
    #[allow(clippy::print_stderr)]
    {
        eprintln!("graftd: the daemon is not implemented yet");
    }
    ExitCode::FAILURE
}
