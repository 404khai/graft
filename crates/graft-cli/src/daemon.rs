//! `graft daemon`: inspect or stop `graftd`.

use std::fmt::Write as _;

use graft_daemon::Client;

use crate::{Environment, Output};

/// Arguments for `graft daemon`.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    #[command(subcommand)]
    action: Action,
}

#[derive(Debug, clap::Subcommand)]
enum Action {
    /// Show whether graftd is running and what it has mounted.
    Status,
    /// Unmount every virtual graft and stop graftd. The next command that
    /// needs it starts it again, remounting live grafts.
    Stop,
}

/// Runs `graft daemon`.
pub(crate) fn run(args: &Args, env: &Environment) -> anyhow::Result<Output> {
    let home = env.require_home()?;
    let Ok(mut client) = Client::connect(home) else {
        return Ok(Output::stdout("graftd is not running\n".to_owned()));
    };
    match args.action {
        Action::Status => {
            let ping = client.ping()?;
            let mut out = format!("graftd {} running (pid {})\n", ping.version, ping.pid);
            let mounts = client.mounts()?;
            if mounts.is_empty() {
                out.push_str("  no grafts mounted\n");
            }
            for m in mounts {
                let _ = writeln!(out, "  {} {} at {}", m.root, m.graft, m.path.display());
            }
            Ok(Output::stdout(out))
        }
        Action::Stop => {
            client.shutdown()?;
            Ok(Output::stdout("Stopped graftd\n".to_owned()))
        }
    }
}
