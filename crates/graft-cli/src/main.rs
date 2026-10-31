//! `graft`, the command-line interface.
//!
//! Graft gives every agent a copy-on-write view of one shared Root plus a
//! private delta. This binary is how people and scripts drive it.

#![forbid(unsafe_code)]

mod doctor;
mod grafts;
mod root;

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
    /// Register the current git repository as a Root.
    Init(root::InitArgs),
    /// Show the Root for the current git repository.
    Root(root::RootArgs),
    /// Create a graft: a workspace of its own, checked out from Root.
    New(grafts::NewArgs),
    /// List grafts with their state, changes and disk use.
    Ls,
    /// Print a graft's working-tree path, e.g. `cd "$(graft path auth)"`.
    Path(grafts::NameArgs),
    /// Show what changed in a graft since its base.
    Status(grafts::StatusArgs),
    /// Show a graft's changes as a unified diff.
    Diff(grafts::NameArgs),
    /// Remove a graft. Refuses if work would be lost, unless --force.
    Prune(grafts::PruneArgs),
    /// Report platform, filesystem, reflink and FUSE/NFS support.
    Doctor(doctor::Args),
}

/// What a command prints: `stdout` as is, each warning on stderr.
#[derive(Debug, Default)]
pub(crate) struct Output {
    pub stdout: String,
    pub warnings: Vec<String>,
}

impl Output {
    pub(crate) fn stdout(stdout: String) -> Self {
        Self {
            stdout,
            warnings: Vec::new(),
        }
    }
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

    /// `$GRAFT_HOME`, for commands that cannot run without a usable home and
    /// a valid config. `doctor` reports these problems instead.
    pub(crate) fn require_home(&self) -> anyhow::Result<&GraftHome> {
        let home = self.home.as_ref().map_err(|e| anyhow::anyhow!("{e}"))?;
        if let Some(Err(e)) = &self.config {
            anyhow::bail!("{e}");
        }
        Ok(home)
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

    let ok = |r: anyhow::Result<Output>| r.map(|o| (o, ExitCode::SUCCESS));
    let (output, code) = match cli.command {
        Command::Init(args) => ok(root::init(&args, &env).map(Output::stdout))?,
        Command::Root(args) => ok(root::show(&args, &env).map(Output::stdout))?,
        Command::New(args) => ok(grafts::new(&args, &env))?,
        Command::Ls => ok(grafts::ls(&env))?,
        Command::Path(args) => ok(grafts::path(&args, &env))?,
        Command::Status(args) => ok(grafts::status(&args, &env))?,
        Command::Diff(args) => ok(grafts::diff(&args, &env))?,
        Command::Prune(args) => ok(grafts::prune(&args, &env))?,
        Command::Doctor(args) => {
            let (stdout, code) = doctor::run(&args, &env);
            (Output::stdout(stdout), code)
        }
    };
    {
        let mut stderr = std::io::stderr().lock();
        for warning in &output.warnings {
            let _ = writeln!(stderr, "graft: warning: {warning}");
        }
    }
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(output.stdout.as_bytes())
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
