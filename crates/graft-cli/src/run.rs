//! `graft run`: a command confined to its graft (ADR 0014).
//!
//! The command may write only to the graft, its private temp dir and the
//! approved extra paths. On Linux, `graft run` starts `graft __confine`,
//! which applies Landlock to itself and `exec`s the command; on macOS it
//! starts the command under `sandbox-exec`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, bail};
use graft_blueprint::{BLUEPRINT_FILE, SandboxSpec, expand_home, parse_sandbox};
use graft_core::{CommitId, GraftId, GraftState};
use graft_fs::FsBackend;
use graft_root::Repo;
use graft_sandbox::Policy;
use graft_store::GraftRecord;

use crate::grafts::{Ctx, daemon, find, open, parse_name};
use crate::{Environment, Output};

/// Arguments for `graft run`.
#[derive(Debug, clap::Args)]
pub(crate) struct RunArgs {
    /// The graft to run in.
    name: String,
    /// Also allow writes below this absolute path, for this run only.
    #[arg(long, value_name = "PATH")]
    write: Vec<PathBuf>,
    /// The command and its arguments.
    #[arg(last = true, required = true, value_name = "COMMAND")]
    command: Vec<OsString>,
}

/// Arguments for the hidden `graft __confine` helper (Linux).
#[derive(Debug, clap::Args)]
pub(crate) struct ConfineArgs {
    /// Writable paths.
    #[arg(long)]
    write: Vec<PathBuf>,
    /// The command and its arguments.
    #[arg(last = true, required = true)]
    command: Vec<OsString>,
}

/// The `[sandbox]` table of `graft.toml` at `base` (empty if none).
pub(crate) fn sandbox_at(repo: &Repo, base: &CommitId) -> anyhow::Result<SandboxSpec> {
    let Some(text) = repo.read_file_at(base, Path::new(BLUEPRINT_FILE))? else {
        return Ok(SandboxSpec::default());
    };
    let text = String::from_utf8(text).context("graft.toml is not valid UTF-8")?;
    Ok(parse_sandbox(&text, BLUEPRINT_FILE)?)
}

/// The exit code for a finished child: its own, or 128 + the signal.
fn exit_code(status: std::process::ExitStatus) -> ExitCode {
    use std::os::unix::process::ExitStatusExt as _;
    let code = status
        .code()
        .or_else(|| status.signal().map(|s| 128 + s))
        .unwrap_or(1);
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

/// Runs a command confined to a graft and returns its exit code.
pub(crate) fn run(args: &RunArgs, env: &Environment) -> anyhow::Result<(Output, ExitCode)> {
    let name = parse_name(&args.name)?;
    let ctx = open(env)?;
    let record = find(&ctx.located.store, &name)?;
    if record.state != GraftState::Live {
        bail!(
            "graft {name} is {}; only live grafts can run commands",
            record.state
        );
    }
    let mut out = Output::default();
    let status = run_in(&ctx, &record, &args.write, &args.command, &mut out)?;
    Ok((out, exit_code(status)))
}

/// Runs `command` confined to graft `record`, with its blueprint's
/// environment, and waits for it. `extra_write` paths are writable too.
pub(crate) fn run_in(
    ctx: &Ctx,
    record: &GraftRecord,
    extra_write: &[PathBuf],
    command: &[OsString],
    out: &mut Output,
) -> anyhow::Result<std::process::ExitStatus> {
    let name = &record.name;
    // Extra paths from graft.toml at the base must be approved.
    let spec = sandbox_at(&ctx.located.repo, &record.base_commit)?;
    let mut unapproved = Vec::new();
    for p in &spec.write {
        if !ctx
            .located
            .store
            .is_trusted(&graft_root::layer::sandbox_write_hash(p))?
        {
            unapproved.push(p.as_str());
        }
    }
    if !unapproved.is_empty() {
        bail!(
            "graft.toml asks `graft run` to allow writes to paths that have not been approved:\n  {}\n\
             Review them, approve with `graft trust`, then rerun.",
            unapproved.join("\n  ")
        );
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut extra = Vec::new();
    for p in &spec.write {
        if p.starts_with("~/") && home.is_none() {
            bail!("graft.toml's sandbox path {p} needs $HOME, which is not set");
        }
        let path = expand_home(p, home.as_deref().unwrap_or(Path::new("/")));
        if path.exists() {
            extra.push(path);
        } else {
            out.warnings.push(format!(
                "sandbox path {} does not exist, so it stays read-only",
                path.display()
            ));
        }
    }
    extra.extend(extra_write.iter().cloned());

    // Where the graft is, mounting a virtual one if needed.
    let mut client = if record.backend.is_virtual() {
        Some(daemon(&ctx.home, out)?)
    } else {
        None
    };
    let graft_dir = match &mut client {
        Some(c) => c.mount(&ctx.located.record.id, name)?,
        None => ctx.backend.path(name),
    };
    let tmp = tmp_dir(&ctx.root_dir(), record.id);
    create_private_dir(&tmp)?;
    let mut writable = vec![graft_dir.clone(), tmp.clone()];
    writable.extend(extra);
    let policy = Policy::new(&writable)?;

    let session = GraftId::random()?.to_string();
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|d| d.canonicalize().ok())
        .filter(|d| d.starts_with(&policy.writable()[0]))
        .unwrap_or_else(|| policy.writable()[0].clone());
    let blueprint_env = crate::blueprints::environment(ctx, record, &graft_dir)?;
    let (program, rest) = command
        .split_first()
        .context("graft run needs a command after --")?;
    let mut cmd = confined(&policy, program, rest)?;
    cmd.current_dir(&cwd)
        .envs(blueprint_env)
        .env("GRAFT_NAME", name.as_str())
        .env("GRAFT_ID", record.id.to_string())
        .env("GRAFT_RUN", &session)
        .env("TMPDIR", &policy.writable()[1])
        .env("TMP", &policy.writable()[1])
        .env("TEMP", &policy.writable()[1]);

    let actor = format!("run:{session}");
    if let Some(c) = &mut client {
        c.set_actor(&ctx.located.record.id, name, Some(&actor))
            .context("tagging the op log with the run session")?;
    }
    tracing::debug!(graft = %name, session, "starting confined command");
    let mut child = cmd
        .spawn()
        .with_context(|| format!("starting {}", program.to_string_lossy()))?;
    let status = {
        let _keep_running = graft_sandbox::IgnoreInterrupts::new();
        child.wait().context("waiting for the command")?
    };
    if let Some(c) = &mut client {
        // Writes from the run are journaled under its actor before it goes.
        let _ = c.sync(&ctx.located.record.id, name);
        if let Err(e) = c.set_actor(&ctx.located.record.id, name, None) {
            out.warnings
                .push(format!("could not clear the op log actor {actor}: {e}"));
        }
    }
    Ok(status)
}

/// `$GRAFT_HOME/roots/<root-id>/tmp/<graft-id>/`.
pub(crate) fn tmp_dir(root_dir: &Path, id: GraftId) -> PathBuf {
    root_dir.join("tmp").join(id.to_string())
}

pub(crate) fn create_private_dir(dir: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .with_context(|| format!("creating {}", dir.display()))
}

/// The command that runs `program` under `policy` on this platform.
pub(crate) fn confined(
    policy: &Policy,
    program: &OsString,
    args: &[OsString],
) -> anyhow::Result<Command> {
    #[cfg(target_os = "linux")]
    {
        let (mechanism, usable) = graft_sandbox::mechanism();
        if let Err(why) = usable {
            bail!(
                "{}\n({mechanism}: {why})",
                graft_sandbox::SandboxError::Unsupported
            );
        }
        let exe = std::env::current_exe().context("finding the graft binary")?;
        let mut cmd = Command::new(exe);
        cmd.arg("__confine");
        for w in policy.writable() {
            cmd.arg("--write").arg(w);
        }
        cmd.arg("--").arg(program).args(args);
        Ok(cmd)
    }
    #[cfg(target_os = "macos")]
    {
        let (mechanism, usable) = graft_sandbox::mechanism();
        if let Err(why) = usable {
            bail!("graft run cannot confine the command ({mechanism}: {why})");
        }
        Ok(graft_sandbox::seatbelt_command(policy, program, args))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (policy, program, args);
        bail!(graft_sandbox::SandboxError::Platform)
    }
}

/// `graft __confine`: restricts this process, then becomes the command.
/// Only returns on failure.
pub(crate) fn confine(args: &ConfineArgs) -> anyhow::Result<ExitCode> {
    use std::os::unix::process::CommandExt as _;
    let policy = Policy::new(&args.write)?;
    match graft_sandbox::confine_self(&policy)? {
        graft_sandbox::Enforcement::Full => {}
        graft_sandbox::Enforcement::Partial => tracing::warn!(
            "this kernel's Landlock is older than graft knows: renames across directories and truncation are not restricted"
        ),
    }
    let (program, rest) = args.command.split_first().context("no command")?;
    let err = Command::new(program).args(rest).exec();
    bail!("running {}: {err}", program.to_string_lossy())
}
