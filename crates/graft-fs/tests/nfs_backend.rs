//! The NFS backend, mounted for real (macOS, ADR 0009).
//!
//! Enabled with `--features nfs-tests` and `GRAFT_TEST_NFS=1`:
//!
//! ```text
//! GRAFT_TEST_NFS=1 cargo test -p graft-fs --features nfs-tests --test nfs_backend
//! ```

#![cfg(all(feature = "nfs-tests", target_os = "macos"))]
// Fixture helpers outside #[test] fns: panicking on setup failure is the point.
#![allow(clippy::unwrap_used, clippy::print_stderr)]

mod conformance;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use conformance::{Fixture, create};
use graft_fs::view::LayerMount;
use graft_fs::{FsBackend, GraftSpec, NfsBackend, Unmounted};

fn require_nfs() {
    assert_eq!(
        std::env::var("GRAFT_TEST_NFS").as_deref(),
        Ok("1"),
        "nfs-tests needs GRAFT_TEST_NFS=1 and permission to run mount_nfs"
    );
}

fn backend_in(parent: &Path) -> NfsBackend {
    NfsBackend::new(parent.join("mounts"))
}

fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk_files(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn passes_the_conformance_suite() {
    require_nfs();
    let tmp = tempfile::tempdir().unwrap();
    let fixture = Fixture::new_in(tmp.path());
    let backend = backend_in(tmp.path());
    conformance::run(&backend, &fixture);
    let leftovers: Vec<_> = walk_files(&tmp.path().join("blobs"))
        .into_iter()
        .chain(walk_files(&tmp.path().join("grafts")))
        .collect();
    assert_eq!(
        leftovers,
        Vec::<PathBuf>::new(),
        "prune left blobs or graft state"
    );
}

/// `git status` inside a graft agrees with what the delta says changed.
#[test]
fn git_status_matches_graft_status() {
    use std::os::unix::fs::PermissionsExt;
    require_nfs();
    let tmp = tempfile::tempdir().unwrap();
    let fixture = Fixture::new_in(tmp.path());
    let backend = backend_in(tmp.path());
    let g = create(&backend, &fixture, "status");
    let p = &g.path;
    std::fs::write(p.join("README.md"), "changed\n").unwrap();
    std::fs::write(p.join("src/new.rs"), "new\n").unwrap();
    std::fs::create_dir(p.join("dir")).unwrap();
    std::fs::write(p.join("dir/nested.txt"), "n").unwrap();
    std::fs::rename(p.join("src/lib.rs"), p.join("src/moved.rs")).unwrap();
    std::fs::set_permissions(p.join("large.bin"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink("README.md", p.join("link")).unwrap();
    std::fs::write(p.join(".gitignore"), "*.log\ntarget/\n").unwrap();
    std::fs::write(p.join("debug.log"), "x").unwrap();
    std::fs::create_dir_all(p.join("target/debug")).unwrap();
    std::fs::write(p.join("target/debug/out"), "x").unwrap();

    let out = Command::new("git")
        .args([
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--no-renames",
        ])
        .current_dir(p)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut from_git: Vec<(String, char)> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| {
            let code = match &l[..2] {
                "??" => 'A',
                c if c.contains('D') => 'D',
                _ => 'M',
            };
            (l[3..].to_owned(), code)
        })
        .collect();
    from_git.sort();
    let mut from_delta: Vec<(String, char)> = backend
        .diff(&g.name, &fixture.base)
        .unwrap()
        .into_iter()
        .map(|c| (c.path.display().to_string(), c.kind.code()))
        .collect();
    from_delta.sort();
    assert_eq!(from_delta, from_git);
    backend.prune(&g.name, g.id).unwrap();
}

#[test]
fn git_commit_inside_a_graft() {
    require_nfs();
    let tmp = tempfile::tempdir().unwrap();
    let fixture = Fixture::new_in(tmp.path());
    let backend = backend_in(tmp.path());
    let g = create(&backend, &fixture, "commit");
    std::fs::write(g.path.join("README.md"), "committed\n").unwrap();
    conformance::git(&g.path, &["add", "-A"]);
    conformance::git(&g.path, &["commit", "-q", "-m", "in a graft"]);
    assert_eq!(conformance::git(&g.path, &["status", "--porcelain"]), "");
    let head = conformance::git(&g.path, &["rev-parse", "HEAD"]);
    let root_has = Command::new("git")
        .args(["cat-file", "-e", &head])
        .current_dir(fixture.dir.path())
        .status()
        .unwrap();
    assert!(!root_has.success(), "the commit must not land in Root");
    backend.prune(&g.name, g.id).unwrap();
}

#[test]
fn reads_match_root_with_common_tools() {
    require_nfs();
    let tmp = tempfile::tempdir().unwrap();
    let fixture = Fixture::new_in(tmp.path());
    let backend = backend_in(tmp.path());
    let g = create(&backend, &fixture, "tools");
    for (cmd, args) in [
        ("cat", vec!["README.md", "src/lib.rs"]),
        ("shasum", vec!["-a", "256", "large.bin"]),
        ("grep", vec!["-rn", "pub fn", "src"]),
    ] {
        let run = |dir: &Path| {
            Command::new(cmd)
                .args(&args)
                .current_dir(dir)
                .output()
                .unwrap()
                .stdout
        };
        assert_eq!(run(&g.path), run(fixture.dir.path()), "{cmd} {args:?}");
    }
    backend.prune(&g.name, g.id).unwrap();
}

#[test]
fn mounting_does_not_depend_on_repo_size() {
    require_nfs();
    let tmp = tempfile::tempdir().unwrap();
    let fixture = Fixture::new_in(tmp.path());
    let root = &fixture.dir.path().to_owned();
    for d in 0..50 {
        let dir = root.join(format!("gen/d{d}"));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..200 {
            std::fs::write(dir.join(format!("f{i}.txt")), format!("{d}/{i}")).unwrap();
        }
    }
    conformance::git(root, &["add", "-A"]);
    conformance::git(root, &["commit", "-q", "-m", "10k files"]);
    let base = conformance::git(root, &["rev-parse", "HEAD"])
        .parse()
        .unwrap();
    let fixture = Fixture { base, ..fixture };
    let backend = backend_in(tmp.path());
    let started = Instant::now();
    let g = create(&backend, &fixture, "big");
    let took = started.elapsed();
    eprintln!("mounted a 10k-file graft over NFS in {took:?}");
    assert!(took.as_secs_f64() < 1.0, "{took:?}");
    assert_eq!(
        std::fs::read_to_string(g.path.join("gen/d49/f199.txt")).unwrap(),
        "49/199"
    );
    backend.prune(&g.name, g.id).unwrap();
}

#[test]
fn layer_writes_stay_private() {
    require_nfs();
    let tmp = tempfile::tempdir().unwrap();
    let fixture = Fixture::new_in(tmp.path());
    let layer = tmp.path().join("layer");
    std::fs::create_dir_all(layer.join("pkg")).unwrap();
    std::fs::write(layer.join("pkg/index.js"), "ok").unwrap();
    let backend = backend_in(tmp.path());
    let name = "layers".parse().unwrap();
    let id = graft_core::GraftId::random().unwrap();
    let path = backend
        .mount(
            &fixture.root(),
            &GraftSpec {
                id,
                name: &name,
                base: &fixture.base,
            },
            vec![LayerMount {
                path: "node_modules".into(),
                source: layer.clone(),
            }],
        )
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(path.join("node_modules/pkg/index.js")).unwrap(),
        "ok"
    );
    std::fs::write(path.join("node_modules/pkg/index.js"), "mine").unwrap();
    assert_eq!(
        std::fs::read_to_string(path.join("node_modules/pkg/index.js")).unwrap(),
        "mine"
    );
    assert_eq!(
        std::fs::read_to_string(layer.join("pkg/index.js")).unwrap(),
        "ok",
        "the layer cache is never written"
    );
    backend.prune(&name, id).unwrap();
}

#[test]
fn unmounts_forcibly_when_busy() {
    require_nfs();
    let tmp = tempfile::tempdir().unwrap();
    let fixture = Fixture::new_in(tmp.path());
    let backend = backend_in(tmp.path());
    let g = create(&backend, &fixture, "busy");
    // A process whose working directory is inside the graft keeps it busy.
    let mut child = Command::new("sleep")
        .arg("30")
        .current_dir(&g.path)
        .spawn()
        .unwrap();
    let how = backend.unmount(&g.name).unwrap();
    assert!(
        matches!(how, Some(Unmounted::Lazy | Unmounted::Clean)),
        "{how:?}"
    );
    backend.remove_mount_point(&g.name).unwrap();
    assert!(!g.path.exists());
    let _ = child.kill();
    let _ = child.wait();
    let g = create(&backend, &fixture, "idle");
    assert_eq!(backend.unmount(&g.name).unwrap(), Some(Unmounted::Clean));
    assert_eq!(backend.unmount(&g.name).unwrap(), None);
    backend.remove_mount_point(&g.name).unwrap();
}

/// macOS keeps extended attributes in `._name` files on NFS servers without
/// xattrs. Grafts refuse them (no xattrs, as on FUSE), so writing files
/// never adds hidden changes.
#[test]
fn no_appledouble_files_appear() {
    require_nfs();
    let tmp = tempfile::tempdir().unwrap();
    let fixture = Fixture::new_in(tmp.path());
    let backend = backend_in(tmp.path());
    let g = create(&backend, &fixture, "xattrs");
    std::fs::write(g.path.join("plain.txt"), "x").unwrap();
    let xattr = Command::new("xattr")
        .args(["-w", "user.graft", "v"])
        .arg(g.path.join("plain.txt"))
        .output()
        .unwrap();
    assert!(!xattr.status.success(), "xattrs are not supported");
    assert_eq!(
        std::fs::read_to_string(g.path.join("plain.txt")).unwrap(),
        "x"
    );
    let names: Vec<String> = std::fs::read_dir(&g.path)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.starts_with("._"))
        .collect();
    assert_eq!(names, Vec::<String>::new());
    let changes: Vec<_> = backend
        .diff(&g.name, &fixture.base)
        .unwrap()
        .into_iter()
        .map(|c| c.path.display().to_string())
        .collect();
    assert_eq!(changes, ["plain.txt"]);
    backend.prune(&g.name, g.id).unwrap();
}
