//! Layers on `graft new`, plus `graft trust` and `graft gc` (ADR 0004).
//!
//! A layer is built at most once per key: the first graft that needs it
//! runs its `build` command in the graft's working tree, the result moves
//! into the cache, and every graft (including the first) gets a reflinked
//! copy. Commands from `graft.toml` only run once approved with
//! `graft trust`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, bail};
use graft_blueprint::{BLUEPRINT_FILE, LayerSpec, parse_layers};
use graft_core::{CommandHash, CommitId, GraftId, GraftName, LayerKey};
use graft_fs::{ReflinkCloner, clone_tree, human_bytes, unshared_bytes};
use graft_root::Repo;
use graft_root::layer::{CacheEntry, KeyInputs, LayerCache, command_hash, layer_key};
use graft_store::Store;

use crate::{Environment, Output};

/// A layer of the base commit's `graft.toml`, with its key worked out.
pub(crate) struct Planned {
    spec: LayerSpec,
    key: LayerKey,
}

/// What happened to one layer of a new graft.
pub(crate) struct Applied {
    pub path: PathBuf,
    pub key: LayerKey,
    pub built: bool,
    pub copied_reason: Option<String>,
}

/// Reads and parses `graft.toml` at `rev`. `None` if there is none.
fn layers_at(repo: &Repo, base: &CommitId) -> anyhow::Result<Option<Vec<LayerSpec>>> {
    let Some(text) = repo.read_file_at(base, Path::new(BLUEPRINT_FILE))? else {
        return Ok(None);
    };
    let text = String::from_utf8(text).context("graft.toml is not valid UTF-8")?;
    Ok(Some(parse_layers(&text, BLUEPRINT_FILE)?))
}

fn describe(spec: &LayerSpec) -> String {
    let mut s = format!("{}: build `{}`", spec.path.display(), spec.build);
    if let Some(t) = &spec.toolchain {
        let _ = write!(s, ", toolchain `{t}`");
    }
    s
}

fn hash_of(spec: &LayerSpec) -> CommandHash {
    command_hash(&spec.path, &spec.build, spec.toolchain.as_deref())
}

fn run_toolchain(repo: &Repo, command: &str) -> anyhow::Result<Vec<u8>> {
    let out = Command::new("sh")
        .args(["-c", command])
        .current_dir(repo.workdir())
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("running toolchain command `{command}`"))?;
    if !out.status.success() {
        bail!("toolchain command `{command}` failed ({})", out.status);
    }
    Ok(out.stdout)
}

/// Works out each layer's key for a graft of `base`, refusing before
/// anything is created if a command would have to run without approval.
pub(crate) fn plan(
    repo: &Repo,
    store: &Store,
    cache: &LayerCache,
    base: &CommitId,
    from: Option<&str>,
) -> anyhow::Result<Vec<Planned>> {
    let Some(specs) = layers_at(repo, base)? else {
        return Ok(Vec::new());
    };
    // Approval is checked first, so an untrusted toolchain command never runs.
    let trusted = |spec: &LayerSpec| store.is_trusted(&hash_of(spec));
    let mut untrusted = Vec::new();
    for spec in &specs {
        if repo.tree_has(base, &spec.path)? {
            bail!(
                "layer {} is tracked in git at {}; layers must be untracked (add it to .gitignore and `git rm -r --cached` it)",
                spec.path.display(),
                base.short(7)
            );
        }
        if spec.toolchain.is_some() && !trusted(spec)? {
            untrusted.push(spec);
        }
    }

    let mut planned = Vec::with_capacity(specs.len());
    for spec in &specs {
        if untrusted.iter().any(|u| u.path == spec.path) {
            continue;
        }
        let mut files = Vec::with_capacity(spec.inputs.len());
        for input in &spec.inputs {
            let contents = repo.read_file_at(base, input)?.with_context(|| {
                format!(
                    "layer {}: input {} does not exist at {}",
                    spec.path.display(),
                    input.display(),
                    base.short(7)
                )
            })?;
            files.push((input.clone(), contents));
        }
        let toolchain_output = match &spec.toolchain {
            Some(t) => run_toolchain(repo, t)?,
            None => Vec::new(),
        };
        let key = layer_key(&KeyInputs {
            path: &spec.path,
            build: &spec.build,
            toolchain: spec.toolchain.as_deref(),
            toolchain_output: &toolchain_output,
            files: &files,
        });
        if !cache.is_ready(&key) && !trusted(spec)? {
            untrusted.push(spec);
            continue;
        }
        planned.push(Planned {
            spec: spec.clone(),
            key,
        });
    }

    if !untrusted.is_empty() {
        let mut msg = String::from(
            "graft.toml declares layer commands that have not been approved, so nothing was created:\n",
        );
        for spec in untrusted {
            let _ = writeln!(msg, "  {}", describe(spec));
        }
        let trust = match from {
            Some(rev) => format!("graft trust --from {rev}"),
            None => "graft trust".to_owned(),
        };
        let _ = write!(
            msg,
            "Review them, approve with `{trust}`, then rerun graft new."
        );
        bail!(msg);
    }
    Ok(planned)
}

fn build(spec: &LayerSpec, graft_dir: &Path, name: &GraftName, id: GraftId) -> anyhow::Result<()> {
    let target = graft_dir.join(&spec.path);
    if std::fs::symlink_metadata(&target).is_ok() {
        bail!(
            "layer {}: {} already exists before its build ran",
            spec.path.display(),
            target.display()
        );
    }
    let status = Command::new("sh")
        .args(["-c", &spec.build])
        .current_dir(graft_dir)
        .env("GRAFT_NAME", name.as_str())
        .env("GRAFT_ID", id.to_string())
        .env("GRAFT_LAYER", &spec.path)
        .stdin(Stdio::null())
        // Build output is progress, not graft's result: keep stdout clean.
        .stdout(Stdio::from(std::io::stderr()))
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("layer {}: running `{}`", spec.path.display(), spec.build))?;
    if !status.success() {
        bail!(
            "layer {}: build `{}` failed ({status})",
            spec.path.display(),
            spec.build
        );
    }
    match std::fs::symlink_metadata(&target) {
        Ok(m) if m.is_dir() => Ok(()),
        _ => bail!(
            "layer {}: build `{}` succeeded but did not create the directory {}",
            spec.path.display(),
            spec.build,
            spec.path.display()
        ),
    }
}

/// Builds missing layers and reflinks every layer into the new graft,
/// recording its references. Holds each layer's lock from the readiness
/// check until its reference is recorded, so `graft gc` cannot remove it in
/// between.
pub(crate) fn apply(
    planned: &[Planned],
    cache: &LayerCache,
    store: &mut Store,
    graft_dir: &Path,
    name: &GraftName,
    id: GraftId,
) -> anyhow::Result<Vec<Applied>> {
    let mut applied = Vec::with_capacity(planned.len());
    for p in planned {
        let lock = cache.lock(&p.key)?;
        let mut built = false;
        if !cache.is_ready(&p.key) {
            build(&p.spec, graft_dir, name, id)?;
            cache.install(&p.key, &graft_dir.join(&p.spec.path), &lock)?;
            built = true;
        }
        store.record_layer(&p.key, &p.spec.path, &p.spec.build)?;
        store.add_layer_ref(id, &p.key)?;
        let cloner = match graft_fs::same_filesystem(cache.dir(), graft_dir) {
            Ok(true) => ReflinkCloner::new(),
            _ => ReflinkCloner::copy_only(
                "the layer cache and the graft are on different filesystems",
            ),
        };
        let target = graft_dir.join(&p.spec.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        clone_tree(&cache.path(&p.key), &target, &cloner).with_context(|| {
            format!("layer {}: copying it into the graft", p.spec.path.display())
        })?;
        drop(lock);
        applied.push(Applied {
            path: p.spec.path.clone(),
            key: p.key,
            built,
            copied_reason: cloner.report().fallback,
        });
    }
    let paths: Vec<&Path> = planned.iter().map(|p| p.spec.path.as_path()).collect();
    graft_root::graft::exclude_paths(graft_dir, &paths)?;
    Ok(applied)
}

/// Arguments for `graft trust`.
#[derive(Debug, clap::Args)]
pub(crate) struct TrustArgs {
    /// Revision whose graft.toml to approve. Defaults to Root's HEAD.
    #[arg(long, value_name = "REF")]
    from: Option<String>,
}

/// Approves the layer commands in `graft.toml`.
pub(crate) fn trust(args: &TrustArgs, env: &Environment) -> anyhow::Result<Output> {
    let home = env.require_home()?;
    let mut located = graft_store::registry::locate(home, Path::new("."))?;
    let rev = args.from.as_deref().unwrap_or("HEAD");
    let base = located.repo.resolve_commit(rev)?;
    let Some(specs) = layers_at(&located.repo, &base)? else {
        return Ok(Output::stdout(format!(
            "No {BLUEPRINT_FILE} at {rev} ({}); nothing to approve.\n",
            base.short(7)
        )));
    };
    if specs.is_empty() {
        return Ok(Output::stdout(format!(
            "{BLUEPRINT_FILE} at {rev} declares no layers; nothing to approve.\n"
        )));
    }
    let mut out = String::new();
    for spec in &specs {
        let hash = hash_of(spec);
        let what = describe(spec);
        if located.store.is_trusted(&hash)? {
            let _ = writeln!(out, "Already approved {what}");
        } else {
            located.store.trust(&hash, &what)?;
            let _ = writeln!(out, "Approved {what}");
        }
    }
    Ok(Output::stdout(out))
}

/// Arguments for `graft gc`.
#[derive(Debug, clap::Args)]
pub(crate) struct GcArgs {
    /// Report what would be removed without removing it.
    #[arg(long)]
    dry_run: bool,
}

/// Removes layers no graft references, and leftovers of interrupted builds.
pub(crate) fn gc(args: &GcArgs, env: &Environment) -> anyhow::Result<Output> {
    let home = env.require_home()?;
    let mut located = graft_store::registry::locate(home, Path::new("."))?;
    let cache = LayerCache::new(home.root_dir(&located.record.id).join("layers"));
    let store = &mut located.store;

    let (mut removed, mut freed, mut kept, mut busy) = (0u64, 0u64, 0u64, 0u64);
    for entry in cache.entries()? {
        let key = match &entry {
            CacheEntry::Ready(k) | CacheEntry::Temp { key: k, .. } => *k,
        };
        // A held lock means a graft new is building or adopting this layer.
        let Some(lock) = cache.try_lock(&key)? else {
            busy += 1;
            continue;
        };
        if matches!(entry, CacheEntry::Ready(_)) && store.layer_ref_count(&key)? > 0 {
            kept += 1;
            continue;
        }
        let path = match &entry {
            CacheEntry::Ready(k) => cache.path(k),
            CacheEntry::Temp { path, .. } => path.clone(),
        };
        freed += unshared_bytes(&path).unwrap_or(0);
        removed += 1;
        if !args.dry_run {
            cache.remove(&entry, &lock)?;
            if matches!(entry, CacheEntry::Ready(_)) {
                store.delete_layer(&key)?;
            }
        }
    }
    // Records whose directory is already gone.
    if !args.dry_run {
        for layer in store.layers()? {
            if layer.refs == 0 && !cache.is_ready(&layer.key) {
                store.delete_layer(&layer.key)?;
            }
        }
    }

    let verb = if args.dry_run {
        "Would remove"
    } else {
        "Removed"
    };
    let mut out = format!("{verb} {removed} layer(s), freeing {}.", human_bytes(freed));
    let _ = write!(out, " Kept {kept} in use");
    if busy > 0 {
        let _ = write!(out, "; skipped {busy} being built");
    }
    out.push_str(".\n");
    Ok(Output::stdout(out))
}
