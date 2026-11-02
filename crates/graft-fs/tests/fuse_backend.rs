//! The FUSE backend, mounted for real (Linux).
//!
//! Enabled with `--features fuse-tests` and `GRAFT_TEST_FUSE=1`; needs
//! `/dev/fuse` and `fusermount3`:
//!
//! ```text
//! GRAFT_TEST_FUSE=1 cargo test -p graft-fs --features fuse-tests --test fuse_backend
//! ```

#![cfg(all(feature = "fuse-tests", target_os = "linux"))]
// Fixture helpers outside #[test] fns: panicking on setup failure is the point.
#![allow(clippy::unwrap_used, clippy::print_stderr)]

mod conformance;

use std::path::Path;
use std::process::Command;
use std::time::Instant;

use conformance::{Fixture, create};
use graft_fs::view::LayerMount;
use graft_fs::{FsBackend, FuseBackend, GraftSpec, Unmounted};

fn require_fuse() {
    assert_eq!(
        std::env::var("GRAFT_TEST_FUSE").as_deref(),
        Ok("1"),
        "fuse-tests needs GRAFT_TEST_FUSE=1 and a usable /dev/fuse"
    );
}

fn backend_in(parent: &Path) -> FuseBackend {
    FuseBackend::new(parent.join("mounts"))
}

#[test]
fn passes_the_read_only_conformance_suite() {
    require_fuse();
    let tmp = tempfile::tempdir().unwrap();
    let fixture = Fixture::new_in(tmp.path());
    conformance::run_read_only(&backend_in(tmp.path()), &fixture);
}

/// Output of `cmd` run in `dir`, which must succeed.
fn output(dir: &Path, cmd: &str, args: &[&str]) -> Vec<u8> {
    let out = Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{cmd} {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

fn on_path(cmd: &str) -> bool {
    Command::new(cmd).arg("--version").output().is_ok()
}

/// `ls -R`, `cat`, `grep -r` and `rg` see the same tree in a graft as in
/// Root (Root's `.git` aside).
#[test]
fn tools_see_the_same_tree_as_root() {
    require_fuse();
    let tmp = tempfile::tempdir().unwrap();
    let fixture = Fixture::new_in(tmp.path());
    let root = &fixture.dir.path().to_owned();
    std::fs::create_dir_all(root.join("docs/deep")).unwrap();
    std::fs::write(
        root.join("docs/deep/todo.md"),
        "TODO: one\nfine\nTODO: two\n",
    )
    .unwrap();
    std::os::unix::fs::symlink("../../README.md", root.join("docs/deep/readme")).unwrap();
    conformance::git(root, &["add", "-A"]);
    conformance::git(root, &["commit", "-q", "-m", "more"]);
    let base = conformance::git(root, &["rev-parse", "HEAD"])
        .parse()
        .unwrap();
    let fixture = Fixture { base, ..fixture };

    let backend = backend_in(tmp.path());
    let g = create(&backend, &fixture, "tools");
    // Compare against a copy of Root without .git, so both trees match.
    let plain = tmp.path().join("plain");
    std::fs::create_dir(&plain).unwrap();
    for e in std::fs::read_dir(root).unwrap() {
        let e = e.unwrap();
        if e.file_name() != ".git" {
            assert!(
                Command::new("cp")
                    .arg("-a")
                    .arg(e.path())
                    .arg(&plain)
                    .status()
                    .unwrap()
                    .success()
            );
        }
    }
    let runs: &[(&str, &[&str])] = &[
        ("ls", &["-R"]),
        ("ls", &["-lRn", "--time-style=+"]),
        ("cat", &["README.md", "src/lib.rs", "docs/deep/readme"]),
        ("grep", &["-rn", "TODO", "."]),
        ("sha256sum", &["large.bin"]),
        ("find", &[".", "-type", "f"]),
    ];
    for (cmd, args) in runs {
        let mut ours = output(&g.path, cmd, args);
        let mut theirs = output(&plain, cmd, args);
        if *cmd == "find" || args.contains(&"-lRn") {
            // Order (find) and owner/size columns of directories differ by
            // design; compare file lines only.
            let norm = |b: &[u8]| {
                let mut lines: Vec<String> = String::from_utf8_lossy(b)
                    .lines()
                    .filter(|l| !l.starts_with('d') && !l.starts_with("total"))
                    .map(|l| {
                        let cols: Vec<_> = l.split_whitespace().collect();
                        if cols.len() > 5 && *cmd != "find" {
                            // perms, size, name
                            format!("{} {} {}", cols[0], cols[4], cols[5..].join(" "))
                        } else {
                            l.to_owned()
                        }
                    })
                    .collect();
                lines.sort();
                lines.join("\n").into_bytes()
            };
            ours = norm(&ours);
            theirs = norm(&theirs);
        }
        assert_eq!(
            String::from_utf8_lossy(&ours),
            String::from_utf8_lossy(&theirs),
            "{cmd} {args:?}"
        );
    }
    if on_path("rg") {
        let args = ["--sort", "path", "-n", "TODO"];
        assert_eq!(output(&g.path, "rg", &args), output(&plain, "rg", &args));
    } else {
        eprintln!("rg not installed; skipped the rg comparison");
        assert!(
            std::env::var_os("GRAFT_TEST_REQUIRE_RG").is_none(),
            "rg is required"
        );
    }
    backend.prune(&g.name, g.id).unwrap();
}

#[test]
fn mounting_does_not_depend_on_repo_size() {
    require_fuse();
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
    eprintln!("mounted a 10k-file graft in {took:?}");
    assert!(took.as_secs_f64() < 1.0, "{took:?}");
    assert_eq!(
        std::fs::read_to_string(g.path.join("gen/d49/f199.txt")).unwrap(),
        "49/199"
    );
    backend.prune(&g.name, g.id).unwrap();
}

#[test]
fn serves_layers_read_only() {
    require_fuse();
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
    let err = std::fs::write(path.join("node_modules/pkg/index.js"), "no").unwrap_err();
    assert_eq!(err.raw_os_error(), Some(graft_fs::view::Errno::EROFS.0));
    assert_eq!(
        std::fs::read_to_string(layer.join("pkg/index.js")).unwrap(),
        "ok"
    );
    backend.prune(&name, id).unwrap();
}

#[test]
fn unmounts_lazily_when_a_file_is_open() {
    require_fuse();
    let tmp = tempfile::tempdir().unwrap();
    let fixture = Fixture::new_in(tmp.path());
    let backend = backend_in(tmp.path());
    let g = create(&backend, &fixture, "busy");
    let mut open = std::fs::File::open(g.path.join("README.md")).unwrap();
    assert_eq!(backend.unmount(&g.name).unwrap(), Some(Unmounted::Lazy));
    backend.remove_mount_point(&g.name).unwrap();
    assert!(!g.path.exists(), "the mount point is gone");
    // The open file keeps working until it is closed.
    let mut text = String::new();
    std::io::Read::read_to_string(&mut open, &mut text).unwrap();
    assert_eq!(text, "hello\n");
    drop(open);

    let g = create(&backend, &fixture, "idle");
    assert_eq!(backend.unmount(&g.name).unwrap(), Some(Unmounted::Clean));
    assert_eq!(backend.unmount(&g.name).unwrap(), None);
    backend.remove_mount_point(&g.name).unwrap();
}
