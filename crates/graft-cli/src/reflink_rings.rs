//! Rings and rewind for reflink grafts (ADR 0038).
//!
//! A reflink graft's changes live in a real working tree, not a delta, so a
//! ring is a git snapshot instead of a copy of delta rows: every file git
//! would track (tracked or untracked, minus ignored paths such as layers)
//! is written to a commit whose parent is `HEAD`, kept under the
//! per-worktree ref `refs/worktree/graft/rings/<label>`. Per-worktree refs
//! go away with the graft (its private `.git`, or its worktree
//! registration), and git's object store shares unchanged content.
//!
//! Rewind rings the current state as `undo-<n>` first, then writes the
//! ring's files back, removes files the ring did not have, and moves `HEAD`
//! (and its branch) back to where it was. Ignored files are never touched.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

const RING_REFS: &str = "refs/worktree/graft/rings/";

/// One ring of a reflink graft.
pub(crate) struct Ring {
    pub(crate) label: String,
    pub(crate) commit: String,
    pub(crate) head: String,
    /// Paths that differ from `HEAD` as it was when the ring was taken.
    pub(crate) entries: usize,
    /// When it was taken (ISO 8601).
    pub(crate) at: String,
}

impl Ring {
    pub(crate) fn json(&self, graft: &str) -> serde_json::Value {
        serde_json::json!({
            "graft": graft,
            "label": self.label,
            "seq": null,
            "entries": self.entries,
            "at": self.at,
            "commit": self.commit,
            "head": self.head,
            "uncommitted_paths": self.entries,
        })
    }
}

/// git in `dir`, with an identity for the snapshot commits (they are
/// Graft's, not the user's, and CI runners have none) and optionally its
/// own index.
fn git(dir: &Path, index: Option<&Path>, args: &[&str]) -> anyhow::Result<String> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Graft")
        .env("GIT_AUTHOR_EMAIL", "graft@localhost")
        .env("GIT_COMMITTER_NAME", "Graft")
        .env("GIT_COMMITTER_EMAIL", "graft@localhost");
    if let Some(i) = index {
        cmd.env("GIT_INDEX_FILE", i);
    }
    let out = cmd
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// A scratch index inside the graft's git directory, removed when dropped.
struct ScratchIndex(PathBuf);

impl ScratchIndex {
    fn new(dir: &Path) -> anyhow::Result<Self> {
        let rel = git(dir, None, &["rev-parse", "--git-path", "graft-ring-index"])?;
        let path = dir.join(rel);
        let _ = std::fs::remove_file(&path);
        Ok(Self(path))
    }
}

impl Drop for ScratchIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Commits the working tree as git would see it after `git add -A`, on top
/// of `HEAD`, without touching the real index. Returns the commit.
fn snapshot(dir: &Path, message: &str) -> anyhow::Result<String> {
    let index = ScratchIndex::new(dir)?;
    let head = git(dir, None, &["rev-parse", "--verify", "HEAD"])?;
    git(dir, Some(&index.0), &["read-tree", &head])?;
    git(dir, Some(&index.0), &["add", "-A", "--", "."])?;
    let tree = git(dir, Some(&index.0), &["write-tree"])?;
    git(
        dir,
        None,
        &["commit-tree", &tree, "-p", &head, "-m", message],
    )
}

fn ring_ref(label: &str) -> String {
    format!("{RING_REFS}{label}")
}

/// The commit of ring `label`, if it exists.
fn find(dir: &Path, label: &str) -> Option<String> {
    let reference = ring_ref(label);
    git(dir, None, &["rev-parse", "--verify", "-q", &reference])
        .ok()
        .filter(|c| !c.is_empty())
}

/// A label `<prefix>-<n>` not used yet.
fn next_label(dir: &Path, prefix: &str) -> anyhow::Result<String> {
    let taken: Vec<String> = list(dir)?.into_iter().map(|r| r.label).collect();
    let mut n = taken.len() + 1;
    loop {
        let label = format!("{prefix}-{n}");
        if !taken.contains(&label) {
            return Ok(label);
        }
        n += 1;
    }
}

fn info(dir: &Path, label: &str, commit: &str) -> anyhow::Result<Ring> {
    let at = git(dir, None, &["log", "-1", "--format=%cI", commit])?;
    let parent = format!("{commit}^");
    let head = git(dir, None, &["rev-parse", &parent])?;
    let changed = git(dir, None, &["diff", "--name-only", &parent, commit])?;
    Ok(Ring {
        label: label.to_owned(),
        commit: commit.to_owned(),
        head,
        entries: changed.lines().filter(|l| !l.is_empty()).count(),
        at,
    })
}

/// Takes ring `label` (or the next `ring-<n>`) of the graft at `dir`.
pub(crate) fn ring(dir: &Path, label: Option<&str>) -> anyhow::Result<Ring> {
    let label = match label {
        Some(l) => l.to_owned(),
        None => next_label(dir, "ring")?,
    };
    if find(dir, &label).is_some() {
        bail!("a ring named {label} exists already");
    }
    let commit = snapshot(dir, &format!("graft ring {label}"))?;
    // An all-zero old value: never replace a ring that appeared meanwhile.
    let zero = "0".repeat(commit.len());
    git(
        dir,
        None,
        &["update-ref", &ring_ref(&label), &commit, &zero],
    )?;
    info(dir, &label, &commit)
}

/// The graft's rings, oldest first.
pub(crate) fn list(dir: &Path) -> anyhow::Result<Vec<Ring>> {
    let out = git(
        dir,
        None,
        &[
            "for-each-ref",
            "--sort=committerdate",
            "--format=%(refname) %(objectname)",
            RING_REFS,
        ],
    )?;
    out.lines()
        .filter_map(|l| l.split_once(' '))
        .filter_map(|(r, c)| r.strip_prefix(RING_REFS).map(|label| (label, c)))
        .map(|(label, c)| info(dir, label, c))
        .collect()
}

/// Returns the graft at `dir` to ring `label`; returns the undo ring's
/// label. Errors if the ring does not exist.
pub(crate) fn rewind(dir: &Path, label: &str) -> anyhow::Result<String> {
    let Some(target) = find(dir, label) else {
        bail!("no ring named {label}");
    };
    let undo = ring(dir, Some(&next_label(dir, "undo")?))?;
    // Files the current state has and the ring does not: remove them.
    let gone = git(
        dir,
        None,
        &[
            "diff",
            "--name-only",
            "--no-renames",
            "--diff-filter=D",
            "-z",
            &undo.commit,
            &target,
        ],
    )?;
    for rel in gone.split('\0').filter(|p| !p.is_empty()) {
        remove_in(dir, rel)?;
    }
    // Write the ring's files, then move HEAD (and its branch) back to where
    // it was when the ring was taken; the index follows HEAD.
    let index = ScratchIndex::new(dir)?;
    git(dir, Some(&index.0), &["read-tree", &target])?;
    git(dir, Some(&index.0), &["checkout-index", "-a", "-f"])?;
    let head = format!("{target}^");
    git(dir, None, &["reset", "-q", "--mixed", &head])?;
    Ok(undo.label)
}

/// Removes `rel` (a path git reported) inside `dir`, never through a
/// symlink, then any directories that became empty.
fn remove_in(dir: &Path, rel: &str) -> anyhow::Result<()> {
    let path = dir.join(rel);
    let mut checked = dir.to_path_buf();
    if let Some(parent) = Path::new(rel).parent() {
        for c in parent.components() {
            checked.push(c);
            if std::fs::symlink_metadata(&checked).is_ok_and(|m| m.file_type().is_symlink()) {
                bail!(
                    "{} is a symlink; not removing {rel} through it",
                    checked.display()
                );
            }
        }
    }
    match std::fs::symlink_metadata(&path) {
        Ok(m) if m.is_dir() => return Ok(()),
        Ok(_) => {
            std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        }
        Err(_) => return Ok(()),
    }
    let mut parent = path.parent();
    while let Some(p) = parent {
        if p == dir || std::fs::remove_dir(p).is_err() {
            break;
        }
        parent = p.parent();
    }
    Ok(())
}

/// `graft ring --list` text for reflink rings.
pub(crate) fn list_text(rings: &[Ring]) -> String {
    let mut out = String::new();
    for r in rings {
        let _ = writeln!(
            out,
            "{:<40}  {}  {:>6} entries  {}",
            r.label,
            &r.commit[..12.min(r.commit.len())],
            r.entries,
            r.at
        );
    }
    out
}
