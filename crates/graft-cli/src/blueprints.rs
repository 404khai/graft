//! Blueprints: provisioning a graft's runtime and releasing it (ADR 0015).
//!
//! `graft new --blueprint <name>` reserves ports, creates services, writes
//! env files and runs setup commands (confined, through `graft run`'s
//! sandbox). Every resource is recorded in the store before it is created,
//! so prune releases whatever exists, even after a failure part-way.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use graft_blueprint::template::{Origin, Template};
use graft_blueprint::{BLUEPRINT_FILE, BlueprintSpec, EnvFileSpec, ServiceKind, parse_blueprints};
use graft_core::{CommandHash, CommitId};
use graft_fs::FsBackend;
use graft_root::Repo;
use graft_store::{GraftRecord, Resource, ResourceKind, Store};

use crate::grafts::{Ctx, find, open, parse_name};
use crate::{Environment, Output};

/// Every blueprint in `graft.toml` at `base`.
pub(crate) fn blueprints_at(
    repo: &Repo,
    base: &CommitId,
) -> anyhow::Result<BTreeMap<String, BlueprintSpec>> {
    let Some(text) = repo.read_file_at(base, Path::new(BLUEPRINT_FILE))? else {
        return Ok(BTreeMap::new());
    };
    let text = String::from_utf8(text).context("graft.toml is not valid UTF-8")?;
    Ok(parse_blueprints(&text, BLUEPRINT_FILE)?)
}

/// Blueprint `name` of `graft.toml` at `base`.
pub(crate) fn spec_at(repo: &Repo, base: &CommitId, name: &str) -> anyhow::Result<BlueprintSpec> {
    let mut all = blueprints_at(repo, base)?;
    let known: Vec<String> = all.keys().cloned().collect();
    all.remove(name).with_context(|| {
        if known.is_empty() {
            format!("graft.toml at {} declares no blueprints", base.short(7))
        } else {
            format!(
                "graft.toml at {} has no blueprint {name:?} (it has: {})",
                base.short(7),
                known.join(", ")
            )
        }
    })
}

/// What `graft trust` approves in a blueprint: each setup command, each
/// service, and each env file copied from Root's working tree.
pub(crate) fn approvals(spec: &BlueprintSpec) -> Vec<(CommandHash, String)> {
    let mut items = Vec::new();
    for cmd in &spec.setup {
        items.push(format!("setup `{cmd}`"));
    }
    for s in &spec.services {
        let mut d = format!("service {} ({}", s.name, s.kind.as_str());
        if let ServiceKind::PostgresTemplate { template } = &s.kind {
            let _ = write!(d, " from {template}");
        }
        d.push(')');
        items.push(d);
    }
    for f in spec.env_files.iter().filter(|f| !f.render) {
        items.push(format!(
            "env file {} copied from Root to {}",
            f.from.display(),
            f.to.display()
        ));
    }
    items
        .into_iter()
        .map(|item| {
            let hash = graft_root::layer::blueprint_item_hash(&spec.name, &item);
            (hash, format!("blueprint {}: {item}", spec.name))
        })
        .collect()
}

/// Refuses a blueprint with items that are not approved yet.
pub(crate) fn check_approved(
    store: &Store,
    spec: &BlueprintSpec,
    from: Option<&str>,
) -> anyhow::Result<()> {
    let mut missing = Vec::new();
    for (hash, what) in approvals(spec) {
        if !store.is_trusted(&hash)? {
            missing.push(what);
        }
    }
    if !missing.is_empty() {
        let trust = from.map_or_else(
            || "graft trust".to_owned(),
            |r| format!("graft trust --from {r}"),
        );
        bail!(
            "blueprint {} declares items that have not been approved, so nothing was created:\n  {}\n\
             Review them, approve with `{trust}`, then rerun graft new.",
            spec.name,
            missing.join("\n  ")
        );
    }
    Ok(())
}

/// Runs a CLI tool (`createdb`, `docker`, ...) and returns its stdout.
fn tool(program: &str, args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new(program).args(args).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!("{program} is not on PATH; the blueprint needs it")
        } else {
            anyhow::anyhow!("running {program}: {e}")
        }
    })?;
    if !out.status.success() {
        bail!(
            "`{program} {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A free TCP port on the loopback interface no graft of the Root holds.
fn free_port(store: &Store) -> anyhow::Result<u16> {
    let taken = store.reserved_ports()?;
    for _ in 0..64 {
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .and_then(|l| l.local_addr())
            .context("finding a free port")?
            .port();
        if !taken.contains(&port) {
            return Ok(port);
        }
    }
    bail!("could not find a free port")
}

/// The database a graft gets from a Postgres template.
fn database_name(template: &str, graft: &str) -> String {
    let mut name = format!("{template}_{}", graft.replace('-', "_"));
    name.truncate(63);
    name
}

/// The Docker name of a graft's volume or network.
fn docker_name(ctx: &Ctx, graft: &str, service: &str) -> String {
    let root = ctx.located.record.id.to_string();
    format!("graft-{}-{graft}-{service}", &root[..8])
}

/// The label that marks Docker resources as belonging to a graft.
const DOCKER_LABEL: &str = "dev.graft.id";

fn reserve(
    store: &mut Store,
    record: &GraftRecord,
    kind: ResourceKind,
    name: &str,
    value: String,
) -> anyhow::Result<()> {
    let r = Resource {
        kind,
        name: name.to_owned(),
        value,
    };
    if !store.reserve(record.id, &r)? {
        bail!(
            "{} {} for {name} is already used by another graft of this Root",
            kind.as_str(),
            r.value
        );
    }
    Ok(())
}

/// Provisions blueprint `spec` for the new graft `record` at `graft_dir`.
pub(crate) fn provision(
    ctx: &mut Ctx,
    record: &GraftRecord,
    spec: &BlueprintSpec,
    graft_dir: &Path,
    out: &mut Output,
) -> anyhow::Result<()> {
    ctx.located.store.set_blueprint(record.id, &spec.name)?;
    let record = &GraftRecord {
        blueprint: Some(spec.name.clone()),
        ..record.clone()
    };
    for p in &spec.ports {
        let mut done = false;
        for _ in 0..8 {
            let port = free_port(&ctx.located.store)?;
            let r = Resource {
                kind: ResourceKind::Port,
                name: p.name.clone(),
                value: port.to_string(),
            };
            if ctx.located.store.reserve(record.id, &r)? {
                done = true;
                break;
            }
        }
        if !done {
            bail!("could not reserve a port for {}", p.name);
        }
    }
    let gid = record.id.to_string();
    for s in &spec.services {
        match &s.kind {
            ServiceKind::PostgresTemplate { template } => {
                let db = database_name(template, record.name.as_str());
                reserve(
                    &mut ctx.located.store,
                    record,
                    ResourceKind::PostgresDatabase,
                    &s.name,
                    db.clone(),
                )?;
                tool("createdb", &["--template", template, &db])
                    .with_context(|| format!("creating database {db} for service {}", s.name))?;
            }
            ServiceKind::DockerVolume | ServiceKind::DockerNetwork => {
                let (kind, what) = if s.kind == ServiceKind::DockerVolume {
                    (ResourceKind::DockerVolume, "volume")
                } else {
                    (ResourceKind::DockerNetwork, "network")
                };
                let n = docker_name(ctx, record.name.as_str(), &s.name);
                reserve(&mut ctx.located.store, record, kind, &s.name, n.clone())?;
                let label = format!("{DOCKER_LABEL}={gid}");
                tool("docker", &[what, "create", "--label", &label, &n]).with_context(|| {
                    format!("creating docker {what} {n} for service {}", s.name)
                })?;
            }
        }
    }
    let vars = variables(ctx, record, spec, graft_dir)?;
    for f in &spec.env_files {
        write_env_file(ctx, f, graft_dir, &vars)?;
    }
    for cmd in &spec.setup {
        let command = ["sh", "-c", cmd.as_str()].map(std::ffi::OsString::from);
        let status = crate::run::run_in(ctx, record, &[], &command, out)?;
        if !status.success() {
            bail!("setup command `{cmd}` failed ({status})");
        }
    }
    Ok(())
}

/// `dir` joined with relative `path`, refusing anything that resolves
/// outside `dir` (through `..` or a symlink in the existing part).
fn inside(dir: &Path, path: &Path) -> anyhow::Result<PathBuf> {
    let dir = dir
        .canonicalize()
        .with_context(|| format!("resolving {}", dir.display()))?;
    let full = dir.join(path);
    let mut existing = full.as_path();
    while !existing.exists() {
        existing = existing.parent().unwrap_or(&dir);
    }
    let real = existing
        .canonicalize()
        .with_context(|| format!("resolving {}", existing.display()))?;
    if !real.starts_with(&dir) {
        bail!("{} leaves {}", path.display(), dir.display());
    }
    if std::fs::symlink_metadata(&full).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("{} is a symlink; not following it", full.display());
    }
    Ok(full)
}

fn write_env_file(
    ctx: &Ctx,
    f: &EnvFileSpec,
    graft_dir: &Path,
    vars: &BTreeMap<String, String>,
) -> anyhow::Result<()> {
    let dst = inside(graft_dir, &f.to)?;
    let contents = if f.render {
        let src = inside(graft_dir, &f.from)?;
        let text = std::fs::read_to_string(&src)
            .with_context(|| format!("reading env file template {}", f.from.display()))?;
        let origin = Origin {
            file: f.from.display().to_string(),
            line: 1,
            column: 1,
        };
        Template::parse(&text, &origin)?.render(vars)?.into_bytes()
    } else {
        let src = inside(&ctx.located.record.path, &f.from)?;
        if !src.is_file() {
            bail!(
                "env file {} is not in Root's working tree ({})",
                f.from.display(),
                ctx.located.record.path.display()
            );
        }
        std::fs::read(&src).with_context(|| format!("reading {}", src.display()))?
    };
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&dst, contents).with_context(|| format!("writing {}", dst.display()))
}

/// The template variables of graft `record`.
fn variables(
    ctx: &Ctx,
    record: &GraftRecord,
    spec: &BlueprintSpec,
    graft_dir: &Path,
) -> anyhow::Result<BTreeMap<String, String>> {
    let mut v = BTreeMap::new();
    v.insert("graft.name".to_owned(), record.name.to_string());
    v.insert("graft.id".to_owned(), record.id.to_string());
    v.insert("graft.path".to_owned(), graft_dir.display().to_string());
    v.insert("graft.blueprint".to_owned(), spec.name.clone());
    v.insert(
        "root.path".to_owned(),
        ctx.located.record.path.display().to_string(),
    );
    for r in ctx.located.store.resources(record.id)? {
        let key = match r.kind {
            ResourceKind::Port => format!("port.{}", r.name),
            ResourceKind::PostgresDatabase => format!("service.{}.database", r.name),
            ResourceKind::DockerVolume | ResourceKind::DockerNetwork => {
                format!("service.{}.name", r.name)
            }
        };
        v.insert(key, r.value);
    }
    for (k, val) in std::env::vars() {
        v.insert(format!("env.{k}"), val);
    }
    Ok(v)
}

/// The environment graft `record`'s blueprint gives its commands, in
/// declaration order, plus `COMPOSE_PROJECT_NAME`. Empty without one.
pub(crate) fn environment(
    ctx: &Ctx,
    record: &GraftRecord,
    graft_dir: &Path,
) -> anyhow::Result<Vec<(String, String)>> {
    let Some(name) = &record.blueprint else {
        return Ok(Vec::new());
    };
    let spec = spec_at(&ctx.located.repo, &record.base_commit, name)?;
    let vars = variables(ctx, record, &spec, graft_dir)?;
    let mut env = Vec::new();
    for (k, t) in &spec.env {
        env.push((k.clone(), t.render(&vars)?));
    }
    if !env.iter().any(|(k, _)| k == "COMPOSE_PROJECT_NAME") {
        env.push((
            "COMPOSE_PROJECT_NAME".to_owned(),
            format!("graft-{}", record.name),
        ));
    }
    Ok(env)
}

/// Releases everything graft `record`'s blueprint provisioned. Stops at the
/// first failure, leaving the rest recorded for a retry.
pub(crate) fn release(ctx: &mut Ctx, record: &GraftRecord, out: &mut Output) -> anyhow::Result<()> {
    let gid = record.id.to_string();
    for r in ctx.located.store.resources(record.id)? {
        match r.kind {
            ResourceKind::Port => {}
            ResourceKind::PostgresDatabase => {
                tool("dropdb", &["--if-exists", &r.value]).with_context(|| {
                    format!("dropping database {} of graft {}", r.value, record.name)
                })?;
            }
            ResourceKind::DockerVolume | ResourceKind::DockerNetwork => {
                let what = if r.kind == ResourceKind::DockerVolume {
                    "volume"
                } else {
                    "network"
                };
                let format = format!("{{{{index .Labels \"{DOCKER_LABEL}\"}}}}");
                match tool("docker", &[what, "inspect", "--format", &format, &r.value]) {
                    Ok(label) if label.trim() == gid => {
                        tool("docker", &[what, "rm", &r.value]).with_context(|| {
                            format!(
                                "removing docker {what} {} of graft {}",
                                r.value, record.name
                            )
                        })?;
                    }
                    Ok(_) => out.warnings.push(format!(
                        "docker {what} {} does not carry graft {}'s label, so it was left alone",
                        r.value, record.name
                    )),
                    Err(e)
                        if e.to_string().contains("no such")
                            || e.to_string().contains("No such") => {}
                    Err(e) => {
                        return Err(e.context(format!("inspecting docker {what} {}", r.value)));
                    }
                }
            }
        }
        ctx.located.store.release(record.id, r.kind, &r.name)?;
    }
    Ok(())
}

/// Arguments for `graft env`.
#[derive(Debug, clap::Args)]
pub(crate) struct EnvArgs {
    /// The graft's name.
    name: String,
    /// Show values of secret-looking names (`*KEY*`, `*TOKEN*`, `*SECRET*`,
    /// `*PASSWORD*`) instead of redacting them.
    #[arg(long)]
    reveal: bool,
}

/// Whether a variable's value must not be shown by default (AGENTS.md §11).
pub(crate) fn is_secret(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASSWORD"]
        .iter()
        .any(|s| n.contains(s))
}

/// Prints a graft's resolved blueprint environment.
pub(crate) fn env(args: &EnvArgs, env: &Environment) -> anyhow::Result<Output> {
    let name = parse_name(&args.name)?;
    let ctx = open(env)?;
    let record = find(&ctx.located.store, &name)?;
    if record.blueprint.is_none() {
        return Ok(Output::stdout(format!("graft {name} has no blueprint\n")));
    }
    let graft_dir = ctx.backend.path(&name);
    let mut out = String::new();
    for (k, v) in environment(&ctx, &record, &graft_dir)? {
        let shown = if is_secret(&k) && !args.reveal {
            "<redacted>"
        } else {
            v.as_str()
        };
        let _ = writeln!(out, "{k}={shown}");
    }
    Ok(Output::stdout(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_derived_and_bounded() {
        assert_eq!(database_name("app_dev", "auth-v2"), "app_dev_auth_v2");
        assert_eq!(database_name(&"t".repeat(60), "abcdef").len(), 63);
        for (n, secret) in [
            ("API_KEY", true),
            ("github_token", true),
            ("DB_PASSWORD", true),
            ("PORT", false),
            ("DATABASE_URL", false),
        ] {
            assert_eq!(is_secret(n), secret, "{n}");
        }
    }
}
