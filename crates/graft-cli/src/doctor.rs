//! `graft doctor`: report whether this host can run grafts.
//!
//! Doctor is read-only apart from the reflink probe, which creates and
//! removes a hidden scratch directory on the repository's filesystem (inside
//! `.git` when it is a directory, so `git status` never sees it).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use graft_core::{GraftHome, HomeSource};
use graft_fs::{Availability, FsType};

use crate::Environment;

/// Arguments for `graft doctor`.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Directory to check, usually inside the repository that will become a
    /// Root. Defaults to the current directory.
    path: Option<PathBuf>,
}

/// The repository (or plain directory) doctor inspected.
struct Target {
    /// Canonical path to the repository's top level, or the directory itself.
    top: PathBuf,
    /// Whether a `.git` entry was found at `top`.
    is_git: bool,
    /// Where to run the reflink probe.
    probe_dir: PathBuf,
}

/// Walks up from `start` to the first directory containing `.git`.
fn find_target(start: &Path) -> std::io::Result<Target> {
    let start = start.canonicalize()?;
    for dir in start.ancestors() {
        let dot_git = dir.join(".git");
        if let Ok(meta) = std::fs::symlink_metadata(&dot_git) {
            let probe_dir = if meta.is_dir() {
                dot_git
            } else {
                dir.to_owned()
            };
            return Ok(Target {
                top: dir.to_owned(),
                is_git: true,
                probe_dir,
            });
        }
    }
    Ok(Target {
        top: start.clone(),
        is_git: false,
        probe_dir: start,
    })
}

/// The closest ancestor of `path` (or `path` itself) that exists.
fn existing_ancestor(path: &Path) -> Option<&Path> {
    path.ancestors().find(|p| p.exists())
}

/// Renders a row: a fixed-width label column then the value.
fn row(out: &mut String, label: &str, value: impl std::fmt::Display) {
    let _ = writeln!(out, "{label:<14}{value}");
}

fn describe(a: &Availability) -> String {
    match a {
        Availability::Available { detail } => format!("available ({detail})"),
        Availability::Unavailable { reason } => format!("unavailable ({reason})"),
    }
}

fn describe_reflink(result: &std::io::Result<bool>, fs: Option<&FsType>) -> String {
    let mechanism = graft_fs::REFLINK_MECHANISM.unwrap_or("none");
    match result {
        Ok(true) => format!("supported ({mechanism})"),
        Ok(false) => "not supported (grafts will copy files instead)".to_owned(),
        Err(e) => {
            let hint = match fs.and_then(FsType::reflink_expected) {
                Some(true) => format!(
                    "; {} normally supports reflinks",
                    fs.map_or("", FsType::name)
                ),
                _ => String::new(),
            };
            format!("unknown (probe failed: {e}{hint})")
        }
    }
}

/// Runs doctor and returns its report and exit code. The exit code is
/// failure only for problems that will stop Graft from working (an unusable
/// `$GRAFT_HOME`, an invalid config, a path that cannot be inspected), not for
/// missing optional capabilities such as reflinks or FUSE.
pub(crate) fn run(args: &Args, env: &Environment) -> (String, ExitCode) {
    let mut out = String::new();
    let mut healthy = true;

    let _ = writeln!(out, "graft {}\n", env!("CARGO_PKG_VERSION"));
    row(
        &mut out,
        "platform",
        format_args!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
    );

    let start = args.path.clone().unwrap_or_else(|| PathBuf::from("."));
    let target = match find_target(&start) {
        Ok(t) => Some(t),
        Err(e) => {
            healthy = false;
            row(
                &mut out,
                "repository",
                format_args!("error: cannot inspect {}: {e}", start.display()),
            );
            None
        }
    };

    if let Some(t) = &target {
        let kind = if t.is_git {
            ""
        } else {
            " (not a git repository; checked this directory)"
        };
        row(
            &mut out,
            "repository",
            format_args!("{}{kind}", t.top.display()),
        );
        let repo_fs = report_fs(&mut out, &t.top);
        let reflink = graft_fs::probe_reflink(&t.probe_dir);
        row(
            &mut out,
            "  reflink",
            describe_reflink(&reflink, repo_fs.as_ref()),
        );
    }

    match &env.home {
        Ok(home) => {
            report_home(&mut out, home, target.as_ref().map(|t| t.top.as_path()));
        }
        Err(e) => {
            healthy = false;
            row(&mut out, "graft home", format_args!("error: {e}"));
        }
    }

    if let (Ok(home), Some(config)) = (&env.home, &env.config) {
        let path = home.config_file();
        let status = match config {
            Ok(_) if path.is_file() => "loaded".to_owned(),
            Ok(_) => "not present, using defaults".to_owned(),
            Err(e) => {
                healthy = false;
                format!("error: {e}")
            }
        };
        row(
            &mut out,
            "config",
            format_args!("{} ({status})", path.display()),
        );
    }

    row(&mut out, "fuse", describe(&graft_fs::fuse_availability()));
    row(
        &mut out,
        "nfs client",
        describe(&graft_fs::nfs_client_availability()),
    );

    let code = if healthy {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    };
    (out, code)
}

fn report_fs(out: &mut String, path: &Path) -> Option<FsType> {
    match graft_fs::filesystem_type(path) {
        Ok(fs) => {
            row(out, "  filesystem", &fs);
            Some(fs)
        }
        Err(e) => {
            row(out, "  filesystem", format_args!("unknown ({e})"));
            None
        }
    }
}

fn report_home(out: &mut String, home: &GraftHome, repo: Option<&Path>) {
    let source = match home.source() {
        HomeSource::Env => "from $GRAFT_HOME",
        HomeSource::PlatformDefault => "default",
    };
    let exists = if home.path().is_dir() {
        "exists"
    } else {
        "not created yet"
    };
    row(
        out,
        "graft home",
        format_args!("{} ({source}, {exists})", home.path().display()),
    );

    // Graft never creates $GRAFT_HOME just to inspect it; measure the
    // filesystem it will be created on instead.
    let Some(existing) = existing_ancestor(home.path()) else {
        row(out, "  filesystem", "unknown (no existing ancestor)");
        return;
    };
    report_fs(out, existing);
    if let Some(repo) = repo {
        let same = match graft_fs::same_filesystem(existing, repo) {
            Ok(true) => "yes".to_owned(),
            Ok(false) => {
                "no (reflinks cannot cross filesystems; grafts will copy files)".to_owned()
            }
            Err(e) => format!("unknown ({e})"),
        };
        row(out, "  same fs", same);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_repository_top_from_a_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        let top = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(top.join(".git")).unwrap();
        std::fs::create_dir_all(top.join("src/deep")).unwrap();
        let t = find_target(&top.join("src/deep")).unwrap();
        assert!(t.is_git);
        assert_eq!(t.top, top);
        assert_eq!(t.probe_dir, top.join(".git"));
    }

    #[test]
    fn a_dot_git_file_probes_in_the_top_level() {
        let dir = tempfile::tempdir().unwrap();
        let top = dir.path().canonicalize().unwrap();
        std::fs::write(top.join(".git"), "gitdir: elsewhere\n").unwrap();
        let t = find_target(&top).unwrap();
        assert!(t.is_git);
        assert_eq!(t.probe_dir, top);
    }

    #[test]
    fn existing_ancestor_skips_missing_components() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("a/b/c");
        assert_eq!(existing_ancestor(&missing), Some(dir.path()));
    }

    #[test]
    fn reflink_descriptions() {
        assert!(describe_reflink(&Ok(true), None).starts_with("supported"));
        assert!(describe_reflink(&Ok(false), None).starts_with("not supported"));
        let err = Err(std::io::Error::other("boom"));
        let text = describe_reflink(&err, Some(&FsType::Apfs));
        assert!(
            text.contains("boom") && text.contains("apfs normally"),
            "{text}"
        );
    }
}
