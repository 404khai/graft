//! The three ways to give N agents a workspace.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

/// A way of giving agents workspaces of one fixture.
pub trait Method {
    /// Creates workspace `i` and returns its path.
    ///
    /// # Errors
    ///
    /// If the underlying command fails.
    fn create(&mut self, i: usize) -> anyhow::Result<PathBuf>;

    /// Whether a workspace needs its own `npm ci` before tests can run.
    fn needs_install(&self) -> bool;

    /// Removes workspace `i`.
    ///
    /// # Errors
    ///
    /// If the underlying command fails.
    fn prune(&mut self, i: usize, path: &Path) -> anyhow::Result<()>;

    /// Returns to the state before the first `create`, between repetitions
    /// of a cell (for grafts: drop the now-unused layer, so the next first
    /// agent builds it cold again).
    ///
    /// # Errors
    ///
    /// If cleanup fails.
    fn reset(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Undoes any setup, leaving the fixture as it was.
    ///
    /// # Errors
    ///
    /// If cleanup fails.
    fn finish(&mut self) -> anyhow::Result<()>;
}

fn run(cmd: &mut std::process::Command, what: &str) -> anyhow::Result<String> {
    let out = cmd.output().with_context(|| format!("running {what}"))?;
    if !out.status.success() {
        bail!(
            "{what} failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_owned())
}

/// `graft new`, with a `$GRAFT_HOME` private to the cell so the layer cache
/// starts cold: the first agent pays for the install, the rest reuse it.
pub struct GraftMethod {
    graft: PathBuf,
    fixture: PathBuf,
    home: PathBuf,
}

impl GraftMethod {
    /// Registers `fixture` with a fresh `$GRAFT_HOME` under `cell` and
    /// approves its layer build.
    ///
    /// # Errors
    ///
    /// If `graft init` or `graft trust` fails.
    pub fn new(graft: &Path, fixture: &Path, cell: &Path) -> anyhow::Result<Self> {
        let m = Self {
            graft: graft.to_owned(),
            fixture: fixture.to_owned(),
            home: cell.join("graft-home"),
        };
        run(&mut m.cmd(&["init"]), "graft init")?;
        run(&mut m.cmd(&["trust"]), "graft trust")?;
        Ok(m)
    }

    fn cmd(&self, args: &[&str]) -> std::process::Command {
        let mut c = crate::command(&self.graft, &self.fixture);
        c.args(args)
            .env("GRAFT_HOME", &self.home)
            .env_remove("GRAFT_LOG");
        c
    }
}

impl Method for GraftMethod {
    fn create(&mut self, i: usize) -> anyhow::Result<PathBuf> {
        let name = format!("agent{i}");
        run(&mut self.cmd(&["new", &name]), "graft new")?;
        Ok(PathBuf::from(run(
            &mut self.cmd(&["path", &name]),
            "graft path",
        )?))
    }

    fn needs_install(&self) -> bool {
        false
    }

    fn prune(&mut self, i: usize, _path: &Path) -> anyhow::Result<()> {
        run(
            &mut self.cmd(&["prune", &format!("agent{i}"), "--force"]),
            "graft prune",
        )?;
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        run(&mut self.cmd(&["gc"]), "graft gc")?;
        Ok(())
    }

    fn finish(&mut self) -> anyhow::Result<()> {
        run(&mut self.cmd(&["gc"]), "graft gc")?;
        std::fs::remove_dir_all(&self.home).context("removing the cell's GRAFT_HOME")
    }
}

/// `git worktree add --detach`, then `npm ci` in each worktree.
pub struct WorktreeMethod {
    fixture: PathBuf,
    cell: PathBuf,
}

impl WorktreeMethod {
    /// Worktrees of `fixture` under `cell`.
    #[must_use]
    pub fn new(fixture: &Path, cell: &Path) -> Self {
        Self {
            fixture: fixture.to_owned(),
            cell: cell.to_owned(),
        }
    }
}

impl Method for WorktreeMethod {
    fn create(&mut self, i: usize) -> anyhow::Result<PathBuf> {
        let path = self.cell.join(format!("agent{i}"));
        run(
            crate::command("git", &self.fixture)
                .args(["worktree", "add", "--quiet", "--detach"])
                .arg(&path)
                .arg("HEAD"),
            "git worktree add",
        )?;
        Ok(path)
    }

    fn needs_install(&self) -> bool {
        true
    }

    fn prune(&mut self, _i: usize, path: &Path) -> anyhow::Result<()> {
        run(
            crate::command("git", &self.fixture)
                .args(["worktree", "remove", "--force"])
                .arg(path),
            "git worktree remove",
        )?;
        Ok(())
    }

    fn finish(&mut self) -> anyhow::Result<()> {
        run(
            crate::command("git", &self.fixture).args(["worktree", "prune"]),
            "git worktree prune",
        )?;
        Ok(())
    }
}

/// `git clone` (a local clone: objects are hardlinked, as git does by
/// default), then `npm ci` in each clone.
pub struct CloneMethod {
    fixture: PathBuf,
    cell: PathBuf,
}

impl CloneMethod {
    /// Clones of `fixture` under `cell`.
    #[must_use]
    pub fn new(fixture: &Path, cell: &Path) -> Self {
        Self {
            fixture: fixture.to_owned(),
            cell: cell.to_owned(),
        }
    }
}

impl Method for CloneMethod {
    fn create(&mut self, i: usize) -> anyhow::Result<PathBuf> {
        let path = self.cell.join(format!("agent{i}"));
        run(
            crate::command("git", &self.cell)
                .args(["clone", "--quiet"])
                .arg(&self.fixture)
                .arg(&path),
            "git clone",
        )?;
        Ok(path)
    }

    fn needs_install(&self) -> bool {
        true
    }

    fn prune(&mut self, _i: usize, path: &Path) -> anyhow::Result<()> {
        std::fs::remove_dir_all(path).with_context(|| format!("removing {}", path.display()))
    }

    fn finish(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
