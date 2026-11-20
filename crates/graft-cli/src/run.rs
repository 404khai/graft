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
    /// Also allow writes to the toolchain caches outside the graft that
    /// compilers and package managers need: on macOS the per-user cache
    /// and temp directories (clang's module cache, xcrun's database) and
    /// `SwiftPM`'s caches; on Linux `$XDG_CACHE_HOME` (`~/.cache`). Other
    /// grafts' runs can write there too, so it is off by default.
    #[arg(long)]
    dev_caches: bool,
    /// The command and its arguments. Without one, your `$SHELL` (else
    /// `/bin/sh`), confined to the graft: start agents this way.
    #[arg(last = true, value_name = "COMMAND")]
    command: Vec<OsString>,
}

impl RunArgs {
    /// `graft run <name> -- <command>`, for `graft new <name> -- <command>`.
    pub(crate) fn start(name: &str, command: Vec<OsString>) -> Self {
        Self {
            name: name.to_owned(),
            write: Vec::new(),
            dev_caches: false,
            command,
        }
    }
}

/// The command to run: the one given, or the user's shell.
fn command_or_shell(command: &[OsString]) -> Vec<OsString> {
    if command.is_empty() {
        vec![
            std::env::var_os("SHELL")
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| OsString::from("/bin/sh")),
        ]
    } else {
        command.to_vec()
    }
}

/// Arguments for the hidden `graft __confine` helper (Linux).
#[derive(Debug, clap::Args)]
pub(crate) struct ConfineArgs {
    /// Writable paths.
    #[arg(long)]
    write: Vec<PathBuf>,
    /// Paths that may only be added to.
    #[arg(long)]
    create_only: Vec<PathBuf>,
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
    let mut write = args.write.clone();
    if args.dev_caches {
        write.extend(dev_cache_paths());
    }
    let started = std::time::Instant::now();
    let command = command_or_shell(&args.command);
    let ended = run_in(&ctx, &record, &write, &command, &mut out)?;
    let took = started.elapsed();
    let sandbox = if ended.status.success() {
        SandboxReport::default()
    } else {
        explain_failure(&ended, &command, took)
    };
    out.warnings.extend(sandbox.note());
    crate::agent::report_run(ended.status, took, &sandbox);
    Ok((out, exit_code(ended.status)))
}

/// How a confined command ended.
pub(crate) struct Ended {
    pub(crate) status: std::process::ExitStatus,
    /// The tag its sandbox denials are logged with, where the platform
    /// logs them.
    tag: Option<String>,
}

/// What the sandbox may have had to do with a failed run.
#[derive(Debug, Default)]
pub(crate) struct SandboxReport {
    /// Paths the sandbox refused writes to during the run (macOS).
    pub(crate) denied: Vec<String>,
    /// The next step to suggest, if any.
    pub(crate) hint: Option<String>,
}

impl SandboxReport {
    /// The lines `graft run` prints on stderr.
    fn note(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.denied.is_empty() {
            lines.push(format!(
                "the sandbox refused {} write(s) outside the graft during this run: {}",
                self.denied.len(),
                shown(&self.denied)
            ));
        }
        lines.extend(self.hint.clone());
        lines
    }
}

/// The first few of `paths`, joined.
fn shown(paths: &[String]) -> String {
    const SHOWN: usize = 5;
    let mut s = paths
        .iter()
        .take(SHOWN)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if paths.len() > SHOWN {
        s.push_str(", ...");
    }
    s
}

/// The writable paths `--dev-caches` adds: those that exist.
fn dev_cache_paths() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut paths: Vec<PathBuf> = Vec::new();
    #[cfg(target_os = "macos")]
    {
        // clang's module cache and xcrun's database live in these, found
        // with confstr(3), not $TMPDIR.
        for name in ["DARWIN_USER_CACHE_DIR", "DARWIN_USER_TEMP_DIR"] {
            if let Ok(o) = Command::new("/usr/bin/getconf").arg(name).output()
                && o.status.success()
            {
                let dir = String::from_utf8_lossy(&o.stdout).trim().to_owned();
                if dir.starts_with('/') {
                    paths.push(PathBuf::from(dir));
                }
            }
        }
        if let Some(h) = &home {
            for rel in [
                "Library/Caches/org.swift.swiftpm",
                "Library/org.swift.swiftpm",
                ".swiftpm",
                "Library/Developer/Xcode/DerivedData",
            ] {
                paths.push(h.join(rel));
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    match std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from) {
        Some(p) if p.is_absolute() => paths.push(p),
        _ => paths.extend(home.map(|h| h.join(".cache"))),
    }
    paths.retain(|p| p.is_dir());
    paths
}

/// Looks for the sandbox's part in a failed run: the writes it denied
/// (macOS logs them, tagged), and a hint for what to do about them.
fn explain_failure(
    ended: &Ended,
    command: &[OsString],
    took: std::time::Duration,
) -> SandboxReport {
    let denied = ended
        .tag
        .as_deref()
        .map(|tag| logged_denials(tag, took))
        .unwrap_or_default();
    let mut hint = if denied.is_empty() {
        None
    } else {
        Some(crate::agent::suggest(
            "if the command needs them, rerun with --write PATH, --dev-caches for toolchain caches, or add them to [sandbox] write in graft.toml",
            "if the command needs them, pass write: [PATH] or dev_caches: true to graft_run, or add them to [sandbox] write in graft.toml",
        ).to_owned())
    };
    if cfg!(target_os = "macos") && runs_swiftpm(command) {
        let swift = "SwiftPM sandboxes manifests and plugins itself, which macOS refuses inside graft run's sandbox (`sandbox_apply: Operation not permitted`): pass --disable-sandbox to swift (graft run still confines it), with dev caches allowed";
        hint = Some(match hint {
            Some(h) => format!("{h}; {swift}"),
            None => swift.to_owned(),
        });
    }
    SandboxReport { denied, hint }
}

/// Whether `command` runs `SwiftPM`: `swift`, `swift-build` and the like,
/// directly, through `xcrun`, or as a word of a `sh -c` script.
fn runs_swiftpm(command: &[OsString]) -> bool {
    command.iter().any(|arg| {
        arg.to_string_lossy().split_whitespace().any(|word| {
            let base = word.rsplit('/').next().unwrap_or(word);
            base == "swift" || base.starts_with("swift-")
        })
    })
}

/// Paths of the writes Seatbelt denied under `tag` in the last `took`
/// (plus a margin), from the unified log; empty where there is no log.
fn logged_denials(tag: &str, took: std::time::Duration) -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        // The kernel logs a denial as it happens, but `log show` sees it
        // only once logd has stored it: about a second on a loaded CI
        // runner. Ask again with growing pauses (2 s in all, and only after
        // a failed run) until it shows up.
        let secs = took.as_secs() + 10;
        for pause_ms in [300, 700, 1000] {
            std::thread::sleep(std::time::Duration::from_millis(pause_ms));
            let out = Command::new("/usr/bin/log")
                .args(["show", "--style", "ndjson", "--last"])
                .arg(format!("{secs}s"))
                .arg("--predicate")
                .arg(format!("eventMessage CONTAINS \"{tag}\""))
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output();
            match out {
                Ok(o) if o.status.success() => {
                    let denied = parse_denials(&String::from_utf8_lossy(&o.stdout));
                    if !denied.is_empty() {
                        return denied;
                    }
                }
                // No usable log: asking again will not help.
                Ok(_) | Err(_) => return Vec::new(),
            }
        }
        Vec::new()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (tag, took);
        Vec::new()
    }
}

/// Denied paths from `log show --style ndjson` lines such as
/// `Sandbox: touch(42) deny(1) file-write-create /a b/c\n<tag>`, unique,
/// in order.
// Only macOS has the log; the parser is unit-tested everywhere.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_denials(ndjson: &str) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for line in ndjson.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(msg) = v["eventMessage"].as_str() else {
            continue;
        };
        let first = msg.lines().next().unwrap_or_default();
        let Some(at) = first.find(" deny(") else {
            continue;
        };
        let rest = &first[at + 1..];
        // `deny(N) <operation> <path>`; the path may contain spaces.
        let mut parts = rest.splitn(3, ' ');
        let (_, op, path) = (parts.next(), parts.next(), parts.next());
        if let (Some(op), Some(path)) = (op, path)
            && op.starts_with("file-write")
            && !paths.iter().any(|p| p == path)
        {
            paths.push(path.to_owned());
        }
    }
    paths
}

/// A graft registered as a git worktree keeps its git state in Root's
/// `.git`: commits need its registration, its own ref namespace and (to
/// add objects to) Root's object store, and nothing else there (ADR 0036).
/// Nothing for other grafts.
fn registered_writes(
    ctx: &Ctx,
    name: &graft_core::GraftName,
    graft_dir: &Path,
) -> anyhow::Result<graft_root::worktree::RegisteredWrites> {
    Ok(match ctx.located.repo.registration(name, graft_dir) {
        Some(admin) => ctx.located.repo.registered_write_paths(name, &admin)?,
        None => graft_root::worktree::RegisteredWrites {
            writable: Vec::new(),
            create_only: Vec::new(),
        },
    })
}

/// Runs `command` confined to graft `record`, with its blueprint's
/// environment, and waits for it. `extra_write` paths are writable too.
pub(crate) fn run_in(
    ctx: &Ctx,
    record: &GraftRecord,
    extra_write: &[PathBuf],
    command: &[OsString],
    out: &mut Output,
) -> anyhow::Result<Ended> {
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
    let git = registered_writes(ctx, name, &graft_dir)?;
    writable.extend(git.writable);
    let session = GraftId::random()?.to_string();
    let policy = Policy::new(&writable)?
        .with_create_only(&git.create_only)?
        .tagged(&format!("graft-run-{session}"));

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
    Ok(Ended {
        status,
        tag: policy.tag().map(str::to_owned),
    })
}

/// Runs shell command `script` confined to scratch checkout `dir`, with
/// `env`, a private `TMPDIR` inside `dir`, and a timeout. Returns its exit
/// status (`None`: killed at the timeout) and the last 4,000 bytes of its
/// output. Used by Radar's checks and Integrate's gate and resolver.
pub(crate) fn run_scratch(
    dir: &Path,
    script: &str,
    env: &[(&str, &str)],
    timeout: std::time::Duration,
) -> anyhow::Result<(Option<std::process::ExitStatus>, String)> {
    use std::process::Stdio;
    let tmp = dir.join(".graft-scratch-tmp");
    create_private_dir(&tmp)?;
    let policy = Policy::new([dir])?;
    let log_path = tmp.join("output");
    let log = std::fs::File::create(&log_path)?;
    let command = ["-c", script].map(OsString::from);
    let mut cmd = confined(&policy, &OsString::from("sh"), &command)?;
    cmd.current_dir(dir)
        .envs(env.iter().copied())
        .env("TMPDIR", &tmp)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    let mut child = cmd
        .spawn()
        .with_context(|| format!("starting `{script}`"))?;
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break Some(s);
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let output = std::fs::read(&log_path).unwrap_or_default();
    let tail = String::from_utf8_lossy(&output[output.len().saturating_sub(4000)..]).into_owned();
    std::fs::remove_dir_all(&tmp).with_context(|| format!("removing {}", tmp.display()))?;
    Ok((status, tail))
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
        for c in policy.create_only() {
            cmd.arg("--create-only").arg(c);
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
    let policy = Policy::new(&args.write)?.with_create_only(&args.create_only)?;
    match graft_sandbox::confine_self(&policy)? {
        graft_sandbox::Enforcement::Full => {}
        graft_sandbox::Enforcement::Partial { missing } => tracing::warn!(
            "this kernel's Landlock cannot fully enforce the requested write policy; unsupported rights: {missing}"
        ),
    }
    let (program, rest) = args.command.split_first().context("no command")?;
    let err = Command::new(program).args(rest).exec();
    bail!("running {}: {err}", program.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denials_are_parsed_from_the_unified_log() {
        let log = [
            r#"{"eventMessage":"Sandbox: touch(42) deny(1) file-write-create /Users/a/Library/Caches/x y\ngraft-run-t"}"#,
            r#"{"eventMessage":"Sandbox: clang(43) deny(3) file-write-data /var/folders/c0/C/clang/ModuleCache\ngraft-run-t"}"#,
            r#"{"eventMessage":"Sandbox: touch(44) deny(1) file-write-create /Users/a/Library/Caches/x y\ngraft-run-t"}"#,
            r#"{"eventMessage":"Sandbox: cat(45) deny(1) file-read-data /secret\ngraft-run-t"}"#,
            r#"{"eventMessage":"log run noninteractively"}"#,
            "not json",
        ]
        .join("\n");
        assert_eq!(
            parse_denials(&log),
            [
                "/Users/a/Library/Caches/x y",
                "/var/folders/c0/C/clang/ModuleCache"
            ]
        );
    }

    #[test]
    fn swiftpm_commands_are_recognised() {
        let cmd = |a: &[&str]| a.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(runs_swiftpm(&cmd(&["swift", "test"])));
        assert!(runs_swiftpm(&cmd(&["xcrun", "swift-build"])));
        assert!(runs_swiftpm(&cmd(&[
            "sh",
            "-c",
            "cd pkg && /usr/bin/swift test | tail"
        ])));
        assert!(!runs_swiftpm(&cmd(&["cargo", "test", "swiftly"])));
        assert!(!runs_swiftpm(&cmd(&["echo", "swifty"])));
    }
}
