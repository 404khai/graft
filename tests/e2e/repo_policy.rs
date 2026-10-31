//! Workspace rules from AGENTS.md that the compiler cannot check by itself.

use std::path::{Path, PathBuf};

/// Crates allowed to contain `unsafe` (AGENTS.md §6). They must deny it at
/// the crate root and allow it only in specific modules.
const UNSAFE_ALLOWED: &[&str] = &["graft-fs", "graft-sandbox"];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn crates() -> Vec<PathBuf> {
    let mut dirs: Vec<_> = std::fs::read_dir(workspace_root().join("crates"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("Cargo.toml").is_file())
        .collect();
    dirs.sort();
    dirs
}

fn crate_roots(dir: &Path) -> Vec<PathBuf> {
    ["src/lib.rs", "src/main.rs"]
        .iter()
        .map(|f| dir.join(f))
        .filter(|p| p.is_file())
        .collect()
}

#[test]
fn every_planned_crate_exists() {
    let names: Vec<String> = crates()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    for expected in [
        "graft-core",
        "graft-root",
        "graft-store",
        "graft-fs",
        "graft-sandbox",
        "graft-blueprint",
        "graft-radar",
        "graft-integrate",
        "graft-sched",
        "graft-daemon",
        "graft-mcp",
        "graft-cli",
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "missing crate {expected}"
        );
    }
}

#[test]
fn unsafe_code_is_forbidden_outside_fs_and_sandbox() {
    for dir in crates() {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let roots = crate_roots(&dir);
        assert!(!roots.is_empty(), "{name} has no crate root");
        let attr = if UNSAFE_ALLOWED.contains(&name.as_str()) {
            "#![deny(unsafe_code)]"
        } else {
            "#![forbid(unsafe_code)]"
        };
        for root in roots {
            let text = std::fs::read_to_string(&root).unwrap();
            assert!(
                text.lines().any(|l| l.trim() == attr),
                "{} must contain {attr}",
                root.display()
            );
        }
    }
}

#[test]
fn every_crate_has_crate_docs_and_workspace_lints() {
    for dir in crates() {
        for root in crate_roots(&dir) {
            let text = std::fs::read_to_string(&root).unwrap();
            assert!(
                text.starts_with("//!"),
                "{} needs crate docs",
                root.display()
            );
        }
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(
            manifest.contains("[lints]\nworkspace = true"),
            "{} must inherit workspace lints",
            dir.display()
        );
    }
}

#[test]
fn the_repositorys_own_graft_toml_is_valid() {
    let text = std::fs::read_to_string(workspace_root().join("graft.toml")).unwrap();
    let layers = graft_blueprint::parse_layers(&text, "graft.toml").unwrap();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].path, std::path::Path::new("target"));
}
