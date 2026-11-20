//! `graft exec`: run a trusted command in a graft's directory (ADR 0039).

use std::ffi::OsString;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, bail};
use graft_core::GraftState;
use graft_fs::FsBackend;

use crate::grafts::{daemon, find, open_at, parse_name};
use crate::{Environment, Output, emit};

#[derive(Debug, clap::Args)]
pub(crate) struct ExecArgs {
    /// The live graft to run in.
    name: String,
    /// A path inside the registered Root. Defaults to the current directory.
    #[arg(long, value_name = "PATH")]
    root: Option<PathBuf>,
    /// The program and its arguments (no shell). Use `graft run` for confinement.
    #[arg(last = true, required = true, value_name = "COMMAND")]
    command: Vec<OsString>,
}

pub(crate) fn exec(args: &ExecArgs, env: &Environment) -> anyhow::Result<ExitCode> {
    let name = parse_name(&args.name)?;
    let ctx = open_at(env, args.root.as_deref().unwrap_or_else(|| Path::new(".")))?;
    let record = find(&ctx.located.store, &name)?;
    if record.state != GraftState::Live {
        bail!(
            "graft {name} is {}; only live grafts can run commands",
            record.state
        );
    }

    let mut out = Output::default();
    let mount = if record.backend.is_virtual() {
        daemon(&ctx.home, &mut out)?.mount(&ctx.located.record.id, &name)?
    } else {
        ctx.backend.path(&name)
    };
    let dir = mount
        .canonicalize()
        .with_context(|| format!("resolving graft {name}'s directory at {}", mount.display()))?;
    let expected = ctx
        .root_dir()
        .canonicalize()
        .context("resolving Root's storage directory")?
        .join("mounts")
        .join(name.as_str());
    if dir != expected || !dir.is_dir() {
        bail!("graft {name} is not a directory at {}", expected.display());
    }
    drop(ctx);
    emit(out)?;

    let (program, rest) = args
        .command
        .split_first()
        .context("graft exec needs a command after --")?;
    let error = Command::new(program)
        .args(rest)
        .current_dir(&dir)
        .env("PWD", &dir)
        .exec();
    Err(error).with_context(|| format!("starting {} in graft {name}", program.to_string_lossy()))
}
