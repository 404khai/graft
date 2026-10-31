//! Deterministic fixture repositories.
//!
//! Every fixture is a Node project, so each method has a real install step
//! and a real test:
//!
//! - `src/` holds generated modules (the bulk of the tracked files);
//! - `vendor/` holds dependency tarballs, so `npm ci --offline` needs no
//!   network;
//! - `graft.toml` declares `node_modules` as a layer;
//! - `npm test` requires every dependency and a sample of modules.
//!
//! Contents come from a blake3 stream keyed by path, so a fixture is
//! byte-identical on every machine and every run.

use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

/// Bump to regenerate cached fixtures after changing this module.
const GENERATOR_VERSION: &str = "graft-bench fixtures v1";
const MARKER: &str = ".graft-bench-fixture";

/// What one fixture contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    /// Name, used as directory name and in results.
    pub name: &'static str,
    /// Generated modules under `src/`.
    pub modules: usize,
    /// Bytes of payload per module (plus a little JavaScript around it).
    pub module_bytes: usize,
    /// Vendored dependencies.
    pub deps: usize,
    /// Files in each dependency.
    pub dep_files: usize,
}

/// The three fixtures of AGENTS.md Phase 4.
pub const SPECS: [Spec; 3] = [
    // ≈1k files.
    Spec {
        name: "small",
        modules: 1_000,
        module_bytes: 160,
        deps: 2,
        dep_files: 50,
    },
    // ≈20k tracked files, plus a 10k-file node_modules.
    Spec {
        name: "medium",
        modules: 20_000,
        module_bytes: 160,
        deps: 20,
        dep_files: 500,
    },
    // ≈200k files.
    Spec {
        name: "large",
        modules: 200_000,
        module_bytes: 160,
        deps: 2,
        dep_files: 50,
    },
];

/// Looks up a fixture by name.
///
/// # Errors
///
/// If no fixture has that name.
pub fn spec(name: &str) -> anyhow::Result<Spec> {
    SPECS
        .iter()
        .copied()
        .find(|s| s.name == name)
        .with_context(|| format!("unknown fixture {name:?} (small, medium, large)"))
}

/// Deterministic printable bytes for `key`.
fn payload(key: &str, len: usize) -> String {
    let mut xof = blake3::Hasher::new_derive_key(GENERATOR_VERSION)
        .update(key.as_bytes())
        .finalize_xof();
    let mut raw = vec![0u8; len.div_ceil(2)];
    xof.read_exact(&mut raw).unwrap_or_default();
    let mut out = String::with_capacity(len);
    for b in raw {
        let _ = write!(out, "{b:02x}");
    }
    out.truncate(len);
    out
}

/// The test every method must pass: dependencies load and modules agree
/// with their contents.
fn test_js(spec: &Spec) -> String {
    let mut js = String::from("const assert = require(\"assert\");\n");
    for d in 0..spec.deps {
        let _ = writeln!(
            js,
            "assert.strictEqual(require(\"dep{d}\").files, {});",
            spec.dep_files
        );
    }
    for i in sample(spec.modules) {
        let _ = writeln!(
            js,
            "assert.strictEqual(require(\"./{}\"), \"{}\");",
            module_path(i).display(),
            payload(&format!("module {i}"), spec.module_bytes)
        );
    }
    js.push_str("console.log(\"ok\");\n");
    js
}

fn sample(n: usize) -> impl Iterator<Item = usize> {
    let step = (n / 10).max(1);
    (0..n).step_by(step).take(10)
}

fn module_path(i: usize) -> PathBuf {
    PathBuf::from(format!("src/d{:04}/m{i:06}.js", i / 100))
}

fn git(dir: &Path, args: &[&str]) -> anyhow::Result<()> {
    let status = crate::command("git", dir)
        .args(args)
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .status()
        .with_context(|| format!("running git {args:?}"))?;
    if !status.success() {
        bail!("git {args:?} failed in {}", dir.display());
    }
    Ok(())
}

fn write_dep(vendor: &Path, scratch: &Path, d: usize, files: usize) -> anyhow::Result<()> {
    let pkg = scratch.join(format!("dep{d}/package"));
    std::fs::create_dir_all(pkg.join("lib"))?;
    std::fs::write(
        pkg.join("package.json"),
        format!("{{\"name\":\"dep{d}\",\"version\":\"1.0.0\",\"main\":\"index.js\"}}\n"),
    )?;
    std::fs::write(
        pkg.join("index.js"),
        format!("module.exports = {{ files: {files} }};\n"),
    )?;
    for f in 2..files {
        std::fs::write(
            pkg.join(format!("lib/f{f:04}.js")),
            format!(
                "module.exports = \"{}\";\n",
                payload(&format!("dep {d} {f}"), 120)
            ),
        )?;
    }
    // Fixed mtimes and gzip -n make the tarball, and so the lockfile's
    // integrity hashes, reproducible.
    let epoch = std::time::UNIX_EPOCH + std::time::Duration::from_hours(490_896);
    for entry in walk(&pkg)? {
        std::fs::File::open(&entry)?.set_modified(epoch)?;
    }
    let tarball = vendor.join(format!("dep{d}-1.0.0.tgz"));
    let status = Command::new("sh")
        .arg("-c")
        .arg("tar -cf - package | gzip -n > \"$0\"")
        .arg(&tarball)
        .current_dir(scratch.join(format!("dep{d}")))
        .status()?;
    if !status.success() {
        bail!("packing {}", tarball.display());
    }
    Ok(())
}

fn walk(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            out.extend(walk(&path)?);
        }
        out.push(path);
    }
    Ok(out)
}

/// Creates the fixture `spec` as a committed git repository at
/// `root/<name>`, reusing it if an identical one is already there.
///
/// # Errors
///
/// If any file, git or npm step fails.
pub fn ensure(root: &Path, spec: &Spec) -> anyhow::Result<PathBuf> {
    let dir = root.join(spec.name);
    let marker = format!("{GENERATOR_VERSION}\n{spec:?}\n");
    if std::fs::read_to_string(dir.join(".git").join(MARKER)).is_ok_and(|m| m == marker) {
        return Ok(dir);
    }
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .with_context(|| format!("removing stale {}", dir.display()))?;
    }
    eprintln!(
        "graft-bench: generating fixture {} ({} modules)",
        spec.name, spec.modules
    );
    std::fs::create_dir_all(dir.join("vendor"))?;

    let deps: Vec<String> = (0..spec.deps)
        .map(|d| format!("\"dep{d}\": \"file:vendor/dep{d}-1.0.0.tgz\""))
        .collect();
    std::fs::write(
        dir.join("package.json"),
        format!(
            "{{\n  \"name\": \"bench-{}\",\n  \"version\": \"1.0.0\",\n  \"private\": true,\n  \"scripts\": {{ \"test\": \"node test.js\" }},\n  \"dependencies\": {{ {} }}\n}}\n",
            spec.name,
            deps.join(", ")
        ),
    )?;
    std::fs::write(dir.join(".gitignore"), "node_modules/\n")?;
    std::fs::write(
        dir.join("graft.toml"),
        "[[layer]]\npath = \"node_modules\"\ninputs = [\"package.json\", \"package-lock.json\"]\nbuild = \"npm ci --offline --no-audit --no-fund --ignore-scripts\"\n",
    )?;
    std::fs::write(dir.join("test.js"), test_js(spec))?;
    for i in 0..spec.modules {
        let path = dir.join(module_path(i));
        if i % 100 == 0 {
            std::fs::create_dir_all(path.parent().unwrap_or(&dir))?;
        }
        std::fs::write(
            &path,
            format!(
                "module.exports = \"{}\";\n",
                payload(&format!("module {i}"), spec.module_bytes)
            ),
        )?;
    }
    let scratch = scratch_dir(root)?;
    for d in 0..spec.deps {
        write_dep(&dir.join("vendor"), &scratch, d, spec.dep_files)?;
    }
    std::fs::remove_dir_all(&scratch)?;

    let status = crate::command("npm", &dir)
        .args([
            "install",
            "--offline",
            "--no-audit",
            "--no-fund",
            "--ignore-scripts",
            "--package-lock-only",
        ])
        .status()
        .context("running npm (is Node installed?)")?;
    if !status.success() {
        bail!(
            "npm install --package-lock-only failed for fixture {}",
            spec.name
        );
    }
    git(&dir, &["init", "-q", "-b", "main"])?;
    git(&dir, &["add", "-A"])?;
    git(
        &dir,
        &[
            "commit",
            "-q",
            "-m",
            &format!("bench fixture {}", spec.name),
        ],
    )?;
    std::fs::write(dir.join(".git").join(MARKER), marker)?;
    Ok(dir)
}

fn scratch_dir(root: &Path) -> anyhow::Result<PathBuf> {
    let dir = root.join(format!(".scratch-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Tracked files in a fixture, as the summary reports them.
#[must_use]
pub fn tracked_files(spec: &Spec) -> usize {
    // package.json, package-lock.json, .gitignore, graft.toml, test.js
    spec.modules + spec.deps + 5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_is_deterministic_and_sized() {
        assert_eq!(payload("a", 10), payload("a", 10));
        assert_ne!(payload("a", 10), payload("b", 10));
        assert_eq!(payload("a", 7).len(), 7);
        assert!(payload("a", 64).bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn fixtures_match_the_phase_sizes() {
        let files = |n| tracked_files(&spec(n).unwrap());
        assert!((900..=1_200).contains(&files("small")));
        assert!((19_000..=21_000).contains(&files("medium")));
        assert!((190_000..=210_000).contains(&files("large")));
        assert!(spec("huge").is_err());
    }

    #[test]
    fn module_paths_spread_over_directories() {
        assert_eq!(module_path(0), Path::new("src/d0000/m000000.js"));
        assert_eq!(module_path(199_999), Path::new("src/d1999/m199999.js"));
        assert_eq!(sample(1000).count(), 10);
    }
}
