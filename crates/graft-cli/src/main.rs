//! `graft`, the command-line interface.
//!
//! Graft gives every agent a copy-on-write view of one shared Root plus a
//! private delta. This binary is how people and scripts drive it. For now it
//! only has `graft doctor`, which reports whether this host can run grafts.

#![forbid(unsafe_code)]

mod doctor;

use std::io::Write;
use std::process::ExitCode;

use anyhow::Context;
use clap::{Parser, Subcommand};
use graft_core::{Config, GraftError, GraftHome};
use tracing_subscriber::EnvFilter;

/// Environment variable holding a `tracing` filter; overrides `config.toml`.
const LOG_ENV: &str = "GRAFT_LOG";
const DEFAULT_LOG_FILTER: &str = "warn";

/// Copy-on-write workspaces for parallel agents.
#[derive(Debug, Parser)]
#[command(name = "graft", version, about, propagate_version = true)]
#[command(arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Report platform, filesystem, reflink and FUSE/NFS support.
    Doctor(doctor::Args),
}

/// What every command gets: `$GRAFT_HOME` and the config, or why they could
/// not be loaded. Kept as results so `doctor` can report the failures.
pub(crate) struct Environment {
    pub home: Result<GraftHome, GraftError>,
    pub config: Option<Result<Config, GraftError>>,
}

impl Environment {
    fn load() -> Self {
        let home = GraftHome::resolve();
        let config = home.as_ref().ok().map(Config::load);
        Self { home, config }
    }
}

fn init_logging(config: Option<&Config>) {
    let configured = config.and_then(|c| c.log.filter.as_deref());
    let (filter, source) = match (std::env::var(LOG_ENV), configured) {
        (Ok(f), _) if !f.is_empty() => (f, LOG_ENV),
        (_, Some(f)) => (f.to_owned(), "config.toml"),
        (_, None) => (DEFAULT_LOG_FILTER.to_owned(), "default"),
    };
    let (env_filter, invalid) = match EnvFilter::try_new(&filter) {
        Ok(f) => (f, None),
        Err(e) => (EnvFilter::new(DEFAULT_LOG_FILTER), Some(e)),
    };
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();
    if let Some(e) = invalid {
        tracing::warn!(filter, source, error = %e, "invalid log filter, using \"warn\"");
    }
}

fn run() -> anyhow::Result<ExitCode> {
    let cli = Cli::parse();
    let env = Environment::load();
    init_logging(env.config.as_ref().and_then(|c| c.as_ref().ok()));

    let (output, code) = match cli.command {
        Command::Doctor(args) => doctor::run(&args, &env),
    };
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(output.as_bytes())
        .and_then(|()| stdout.flush())
        .context("writing to stdout")?;
    Ok(code)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            // Last-resort error report from the binary itself.
            #[allow(clippy::print_stderr)]
            {
                eprintln!("graft: {e:#}");
            }
            ExitCode::FAILURE
        }
    }
}
