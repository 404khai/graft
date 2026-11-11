//! `graft`, the command-line interface.
//!
//! Graft gives every agent a copy-on-write view of one shared Root plus a
//! private delta. This binary is how people and scripts drive it.

#![forbid(unsafe_code)]

mod blueprints;
mod daemon;
mod doctor;
mod grafts;
mod integrate;
mod layers;
mod plan;
mod radar;
mod rings;
mod root;
mod run;
mod timing;
mod virtual_grafts;

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
    /// Print a graft's blueprint environment (secrets redacted).
    Env(blueprints::EnvArgs),
    /// Show what changed in a graft since its base.
    Status(grafts::StatusArgs),
    /// Show a graft's changes as a unified diff.
    Diff(grafts::NameArgs),
    /// Show a graft's op log: every write, rename and delete, in order.
    Log(grafts::LogArgs),
    /// Remove a graft. Refuses if work would be lost, unless --force.
    Prune(grafts::PruneArgs),
    /// Fork a graft in constant time: same base, a copy of its delta.
    Bud(rings::BudArgs),
    /// Checkpoint a graft's delta as a named ring, or list its rings.
    Ring(rings::RingArgs),
    /// Return a graft to one of its rings (reversible: it rings first).
    Rewind(rings::RewindArgs),
    /// Split a task into an ordered DAG of grafts along the code's
    /// dependency graph; --apply creates the ready ones.
    Plan(plan::PlanArgs),
    /// Claim files or symbols for a graft, if no other live graft holds them.
    Claim(plan::ClaimArgs),
    /// List live claims and recently refused writes.
    Claims(plan::ClaimsArgs),
    /// Show overlapping work across live grafts: same file, same symbol,
    /// merge conflicts, or tests that fail only when merged.
    Radar(radar::RadarArgs),
    /// Run a command confined to a graft: it can only write inside it.
    Run(run::RunArgs),
    /// Internal: confine this process, then run the command (Linux).
    #[command(name = "__confine", hide = true)]
    Confine(run::ConfineArgs),
    /// Land grafts in Root through the queue: merge, test, commit.
    Integrate(integrate::IntegrateArgs),
    /// Show the integrate queue: what landed, what is waiting, what stopped.
    Queue(integrate::QueueArgs),
    /// Move a graft onto Root's current head, merging its changes.
    Rebase(integrate::RebaseArgs),
    /// Approve the layer commands and sandbox paths declared in graft.toml.
    Trust(layers::TrustArgs),
    /// Remove layers no graft uses.
    Gc(layers::GcArgs),
    /// Show or stop graftd, the daemon that serves virtual grafts.
    Daemon(daemon::Args),
    /// Serve Graft's tools to an agent over MCP (stdio), for this Root.
    Mcp(McpArgs),
    /// Report platform, filesystem, reflink and FUSE/NFS support.
    Doctor(doctor::Args),
}

/// Arguments for `graft mcp`.
#[derive(Debug, clap::Args)]
struct McpArgs {
    /// The repository to serve (default: the current directory). MCP
    /// clients do not all start servers in the project's directory.
    #[arg(long, value_name = "PATH")]
    root: Option<std::path::PathBuf>,
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
        Command::Env(args) => ok(blueprints::env(&args, &env))?,
        Command::Diff(args) => ok(grafts::diff(&args, &env))?,
        Command::Log(args) => ok(grafts::log(&args, &env))?,
        Command::Prune(args) => ok(grafts::prune(&args, &env))?,
        Command::Bud(args) => ok(rings::bud(&args, &env))?,
        Command::Ring(args) => ok(rings::ring(&args, &env))?,
        Command::Rewind(args) => ok(rings::rewind(&args, &env))?,
        Command::Plan(args) => ok(plan::run_plan(&args, &env))?,
        Command::Claim(args) => plan::claim(&args, &env)?,
        Command::Claims(args) => ok(plan::claims(&args, &env))?,
        Command::Radar(args) => ok(radar::radar(&args, &env))?,
        Command::Run(args) => run::run(&args, &env)?,
        Command::Confine(args) => (Output::default(), run::confine(&args)?),
        Command::Integrate(args) => integrate::integrate(&args, &env)?,
        Command::Queue(args) => ok(integrate::queue(&args, &env))?,
        Command::Rebase(args) => ok(integrate::rebase(&args, &env))?,
        Command::Trust(args) => ok(layers::trust(&args, &env))?,
        Command::Gc(args) => ok(layers::gc(&args, &env))?,
        Command::Daemon(args) => ok(daemon::run(&args, &env))?,
        Command::Mcp(args) => {
            if let Some(root) = &args.root {
                std::env::set_current_dir(root)
                    .with_context(|| format!("entering {}", root.display()))?;
            }
            // Fail early, with a message the client shows, outside a Root.
            let home = env.require_home()?;
            graft_store::registry::locate(home, std::path::Path::new("."))?;
            let bin = std::env::current_exe().context("finding the graft binary")?;
            let cwd = std::env::current_dir().context("reading the current directory")?;
            graft_mcp::serve_stdio(bin, cwd).map_err(|e| anyhow::anyhow!("MCP server: {e}"))?;
            (Output::default(), ExitCode::SUCCESS)
        }
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
