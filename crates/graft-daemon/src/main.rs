//! `graftd`, the Graft daemon. See the `graft_daemon` library docs.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use graft_core::GraftHome;
use graft_daemon::server::{ServeError, serve};
use tracing_subscriber::EnvFilter;

/// The Graft daemon: owns virtual graft mounts and serves the CLI.
///
/// The CLI starts it on demand; there is no need to run it by hand.
#[derive(Debug, Parser)]
#[command(name = "graftd", version, about)]
struct Args {
    /// `$GRAFT_HOME` to serve. Defaults to the same resolution as `graft`.
    #[arg(long, value_name = "DIR")]
    home: Option<PathBuf>,
}

fn run() -> anyhow::Result<ExitCode> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("GRAFT_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    let home = match args.home {
        Some(p) => GraftHome::from_path(p)?,
        None => GraftHome::resolve()?,
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    match runtime.block_on(serve(home, graft_daemon::mounter::platform())) {
        Ok(()) => Ok(ExitCode::SUCCESS),
        // Lost a start race: the other daemon serves this home.
        Err(e @ ServeError::AlreadyRunning { .. }) => {
            tracing::info!("{e}");
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => Err(e.into()),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            // Last-resort report; logging may not be set up.
            #[allow(clippy::print_stderr)]
            {
                eprintln!("graftd: {e:#}");
            }
            ExitCode::FAILURE
        }
    }
}
