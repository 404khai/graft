//! Integrate: landing grafts into Root (ADR 0017).
//!
//! This crate holds the file-level merge that the integrate queue and
//! `graft rebase` use: for each file a graft changed, combine Root's
//! current version ("ours") with the graft's ("theirs") against the
//! graft's base. The cheapest method that works wins: take the side that
//! changed, then `git merge-file`, then Mergiraf's structural merge.
//! Anything left is a conflict, with git's conflict markers when both
//! sides still have the file.
//!
//! The queue itself, the test gate and landing live in the CLI.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// How a file was merged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Only one side changed it (or both made the same change).
    Taken,
    /// `git merge-file`, line by line.
    Line,
    /// Mergiraf, by syntax tree.
    Structural,
}

/// The result of merging one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Merged {
    /// Merged: the new contents, or `None` if the file is deleted.
    Clean {
        /// The merged contents (`None`: deleted).
        contents: Option<Vec<u8>>,
        /// How.
        method: Method,
    },
    /// Not merged.
    Conflict {
        /// The file with conflict markers, when both sides still have it.
        markers: Option<Vec<u8>>,
        /// Why, for people.
        reason: String,
    },
}

/// Errors running the merge tools.
#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    /// A scratch file could not be written or read.
    #[error("{action} {}: {source}", path.display())]
    Io {
        /// What was being done.
        action: &'static str,
        /// The path.
        path: PathBuf,
        /// The cause.
        #[source]
        source: std::io::Error,
    },
    /// A tool failed for a reason other than conflicts.
    #[error("{tool} failed: {message}")]
    Tool {
        /// The tool.
        tool: &'static str,
        /// What it said.
        message: String,
    },
}

/// The environment variable naming the Mergiraf binary.
pub const MERGIRAF_ENV: &str = "GRAFT_MERGIRAF";

/// Merges files, with Mergiraf if it is available.
#[derive(Debug, Clone)]
pub struct Merger {
    mergiraf: Option<PathBuf>,
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

impl Merger {
    /// A merger using Mergiraf from `$GRAFT_MERGIRAF` or `PATH`, if any.
    #[must_use]
    pub fn detect() -> Self {
        let mergiraf = std::env::var_os(MERGIRAF_ENV)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| find_on_path("mergiraf"));
        Self { mergiraf }
    }

    /// A merger that never uses Mergiraf.
    #[must_use]
    pub fn line_only() -> Self {
        Self { mergiraf: None }
    }

    /// The Mergiraf binary, if one is used.
    #[must_use]
    pub fn mergiraf(&self) -> Option<&Path> {
        self.mergiraf.as_deref()
    }

    /// Merges `path`: `base` is the graft's base version, `ours` Root's
    /// current one, `theirs` the graft's (`None`: absent). Scratch files go
    /// in `scratch`, which must exist and is left empty.
    ///
    /// # Errors
    ///
    /// I/O errors in `scratch`, or a merge tool failing other than by
    /// reporting conflicts.
    pub fn merge(
        &self,
        path: &Path,
        base: Option<&[u8]>,
        ours: Option<&[u8]>,
        theirs: Option<&[u8]>,
        scratch: &Path,
    ) -> Result<Merged, MergeError> {
        let taken = |contents: Option<&[u8]>| Merged::Clean {
            contents: contents.map(<[u8]>::to_vec),
            method: Method::Taken,
        };
        if ours == theirs || theirs == base {
            return Ok(taken(ours));
        }
        if ours == base {
            return Ok(taken(theirs));
        }
        let (Some(ours), Some(theirs)) = (ours, theirs) else {
            return Ok(Merged::Conflict {
                markers: None,
                reason: if ours.is_none() {
                    "deleted in Root, changed in the graft".to_owned()
                } else {
                    "changed in Root, deleted in the graft".to_owned()
                },
            });
        };
        let base = base.unwrap_or_default();
        let dir = scratch.join(".graft-merge");
        let io = |action, path: &Path| {
            let path = path.to_owned();
            move |source| MergeError::Io {
                action,
                path,
                source,
            }
        };
        std::fs::create_dir_all(&dir).map_err(io("creating", &dir))?;
        // Keep the extension so Mergiraf can tell the language.
        let ext = path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let files: Vec<PathBuf> = ["ours", "base", "theirs"]
            .iter()
            .map(|n| dir.join(format!("{n}{ext}")))
            .collect();
        let result = self.run_tools(&dir, &ext, &files, [ours, base, theirs]);
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    /// Writes the three versions to `files` and runs `git merge-file`, then
    /// Mergiraf on a conflict.
    fn run_tools(
        &self,
        dir: &Path,
        ext: &str,
        files: &[PathBuf],
        contents: [&[u8]; 3],
    ) -> Result<Merged, MergeError> {
        let io = |action, path: &Path| {
            let path = path.to_owned();
            move |source| MergeError::Io {
                action,
                path,
                source,
            }
        };
        for (f, c) in files.iter().zip(contents) {
            std::fs::write(f, c).map_err(io("writing", f))?;
        }
        let out = Command::new("git")
            .args([
                "merge-file",
                "-p",
                "-L",
                "Root",
                "-L",
                "base",
                "-L",
                "graft",
            ])
            .args(files)
            .output()
            .map_err(|e| MergeError::Tool {
                tool: "git merge-file",
                message: e.to_string(),
            })?;
        match out.status.code() {
            Some(0) => {
                return Ok(Merged::Clean {
                    contents: Some(out.stdout),
                    method: Method::Line,
                });
            }
            Some(n) if n > 0 => {}
            _ => {
                return Err(MergeError::Tool {
                    tool: "git merge-file",
                    message: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
                });
            }
        }
        let markers = out.stdout;
        if let Some(m) = &self.mergiraf {
            let merged = dir.join(format!("merged{ext}"));
            // Mergiraf keeps a record of each merge in its data directory;
            // keep it in the scratch directory, never in the user's home.
            let status = Command::new(m)
                .env("XDG_DATA_HOME", dir.join("mergiraf-data"))
                .arg("merge")
                .args([&files[1], &files[0], &files[2]])
                .arg("-o")
                .arg(&merged)
                .output()
                .map_err(|e| MergeError::Tool {
                    tool: "mergiraf",
                    message: e.to_string(),
                })?;
            if status.status.success() {
                let contents = std::fs::read(&merged).map_err(io("reading", &merged))?;
                return Ok(Merged::Clean {
                    contents: Some(contents),
                    method: Method::Structural,
                });
            }
            return Ok(Merged::Conflict {
                markers: Some(markers),
                reason: "conflicts that neither a line merge nor Mergiraf resolved".to_owned(),
            });
        }
        Ok(Merged::Conflict {
            markers: Some(markers),
            reason: "line merge conflicts (Mergiraf is not installed)".to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn merge(base: &str, ours: &str, theirs: &str) -> Merged {
        let dir = tempfile::tempdir().unwrap();
        let m = Merger::line_only()
            .merge(
                Path::new("f.txt"),
                Some(base.as_bytes()),
                Some(ours.as_bytes()),
                Some(theirs.as_bytes()),
                dir.path(),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "scratch left"
        );
        m
    }

    #[test]
    fn takes_the_changed_side() {
        assert_eq!(
            merge("a\n", "a\n", "b\n"),
            Merged::Clean {
                contents: Some(b"b\n".to_vec()),
                method: Method::Taken
            }
        );
        assert_eq!(
            merge("a\n", "c\n", "a\n"),
            Merged::Clean {
                contents: Some(b"c\n".to_vec()),
                method: Method::Taken
            }
        );
    }

    #[test]
    fn merges_lines_and_reports_conflicts() {
        assert_eq!(
            merge("1\n2\n3\n", "ONE\n2\n3\n", "1\n2\nTHREE\n"),
            Merged::Clean {
                contents: Some(b"ONE\n2\nTHREE\n".to_vec()),
                method: Method::Line
            }
        );
        let Merged::Conflict { markers, reason } = merge("1\n", "2\n", "3\n") else {
            panic!("expected a conflict");
        };
        let markers = String::from_utf8(markers.unwrap()).unwrap();
        assert!(
            markers.contains("<<<<<<< Root") && markers.contains(">>>>>>> graft"),
            "{markers}"
        );
        assert!(reason.contains("Mergiraf is not installed"));
    }

    /// Runs only where Mergiraf is installed (`$GRAFT_MERGIRAF` or `PATH`).
    #[test]
    fn mergiraf_resolves_what_a_line_merge_cannot() {
        let m = Merger::detect();
        if m.mergiraf().is_none() {
            assert_ne!(
                std::env::var("GRAFT_TEST_MERGIRAF").as_deref(),
                Ok("1"),
                "GRAFT_TEST_MERGIRAF=1 but no mergiraf in $GRAFT_MERGIRAF or PATH"
            );
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let base = "pub struct Config {\n    pub name: String,\n}\n";
        let ours = "pub struct Config {\n    pub name: String,\n    pub port: u16,\n}\n";
        let theirs = "pub struct Config {\n    pub name: String,\n    pub verbose: bool,\n}\n";
        let merged = m
            .merge(
                Path::new("src/config.rs"),
                Some(base.as_bytes()),
                Some(ours.as_bytes()),
                Some(theirs.as_bytes()),
                dir.path(),
            )
            .unwrap();
        let Merged::Clean { contents, method } = merged else {
            panic!("expected a clean merge, got {merged:?}");
        };
        assert_eq!(method, Method::Structural);
        let text = String::from_utf8(contents.unwrap()).unwrap();
        assert!(
            text.contains("pub port: u16,") && text.contains("pub verbose: bool,"),
            "{text}"
        );
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "scratch left"
        );
    }

    #[test]
    fn delete_against_change_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let m = Merger::line_only()
            .merge(Path::new("f"), Some(b"a"), None, Some(b"b"), dir.path())
            .unwrap();
        assert!(matches!(m, Merged::Conflict { markers: None, .. }));
    }
}
