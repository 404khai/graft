//! `graft init` and `graft root`.

use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::Context;
use graft_core::GraftState;
use graft_root::Repo;
use graft_store::{Registration, registry};

use crate::Environment;

/// Arguments for `graft init`.
#[derive(Debug, clap::Args)]
pub(crate) struct InitArgs {
    /// A path inside the repository to register. Defaults to the current
    /// directory.
    path: Option<PathBuf>,
}

/// Arguments for `graft root`.
#[derive(Debug, clap::Args)]
pub(crate) struct RootArgs {
    /// A path inside the repository. Defaults to the current directory.
    path: Option<PathBuf>,
}

fn discover(path: Option<&PathBuf>) -> anyhow::Result<Repo> {
    let start = path.cloned().unwrap_or_else(|| PathBuf::from("."));
    Ok(Repo::discover(&start)?)
}

fn row(out: &mut String, label: &str, value: impl std::fmt::Display) {
    let _ = writeln!(out, "{label:<14}{value}");
}

/// Registers the repository as a Root. Running it again is a no-op.
pub(crate) fn init(args: &InitArgs, env: &Environment) -> anyhow::Result<String> {
    let home = env.require_home()?;
    let repo = discover(args.path.as_ref())?;
    let (record, _store, registration) =
        registry::register(home, &repo).context("registering the Root")?;
    Ok(match registration {
        Registration::Created => format!(
            "Registered Root {} for {}\n",
            record.id,
            record.path.display()
        ),
        Registration::Existing => format!(
            "{} is already registered as Root {}\n",
            record.path.display(),
            record.id
        ),
    })
}

/// Prints the Root for the repository.
pub(crate) fn show(args: &RootArgs, env: &Environment) -> anyhow::Result<String> {
    let home = env.require_home()?;
    let start = args.path.clone().unwrap_or_else(|| PathBuf::from("."));
    let registry::Located {
        record,
        store,
        repo,
        ..
    } = registry::locate(home, &start)?;

    let head = match (repo.head_commit(), repo.head_ref()) {
        (Ok(commit), Ok(Some(name))) => {
            let branch = name.strip_prefix("refs/heads/").unwrap_or(&name);
            format!("{commit} ({branch})")
        }
        (Ok(commit), Ok(None)) => format!("{commit} (detached)"),
        (Err(e), _) | (_, Err(e)) => format!("unknown ({e})"),
    };
    let grafts = store
        .grafts()?
        .into_iter()
        .filter(|g| g.state != GraftState::Pruned)
        .count();

    let mut out = String::new();
    row(&mut out, "root", record.id);
    row(&mut out, "path", record.path.display());
    row(&mut out, "first commit", &record.first_commit);
    row(&mut out, "head", head);
    row(&mut out, "registered", record.registered_at);
    row(&mut out, "grafts", grafts);
    row(
        &mut out,
        "schema",
        format_args!("v{}", store.schema_version()?),
    );
    row(&mut out, "database", store.path().display());
    Ok(out)
}
