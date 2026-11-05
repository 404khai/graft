//! The reflink backend: the shared conformance suite plus reflink-specific
//! checks on disk sharing.

// Fixture helpers outside #[test] fns: panicking on setup failure is the point.
#![allow(clippy::unwrap_used, clippy::print_stderr)]

mod conformance;

use conformance::{Fixture, LARGE_FILE_BYTES, create};
use graft_fs::{FsBackend, ReflinkBackend, filesystem_type, human_bytes, probe_reflink};

fn backend_in(parent: &std::path::Path) -> ReflinkBackend {
    ReflinkBackend::new(parent.join("mounts"))
}

#[test]
fn passes_the_conformance_suite() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = Fixture::new_in(tmp.path());
    conformance::run(&backend_in(tmp.path()), &fixture);
}

/// On a reflink filesystem a graft costs almost nothing; elsewhere it falls
/// back to copies and says why.
#[test]
fn shares_blocks_with_root_or_explains_why_not() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = Fixture::new_in(tmp.path());
    let backend = backend_in(tmp.path());
    let reflinks = probe_reflink(tmp.path()).unwrap();
    let fs = filesystem_type(tmp.path()).unwrap();

    let name = "sharing".parse().unwrap();
    let id = graft_core::GraftId::random().unwrap();
    let report = backend
        .create(
            &fixture.root(),
            &graft_fs::GraftSpec {
                id,
                name: &name,
                base: &fixture.base,
            },
        )
        .unwrap();
    let used = backend.disk_usage(&name).unwrap();
    eprintln!(
        "{fs}: graft uses {} unshared ({:?})",
        human_bytes(used),
        report.clone
    );

    if reflinks {
        assert_eq!(report.clone.copied, 0);
        assert_eq!(report.clone.fallback, None);
        assert!(
            used < (LARGE_FILE_BYTES as u64) / 4,
            "{fs}: {} unshared for an 8 MiB tree",
            human_bytes(used)
        );
    } else {
        assert_eq!(report.clone.reflinked, 0);
        assert!(report.clone.copied > 0);
        let reason = report.clone.fallback.as_deref().unwrap();
        assert!(reason.contains("does not support reflinks"), "{reason}");
        assert!(
            used >= LARGE_FILE_BYTES as u64,
            "copies must be counted in full"
        );
    }

    // Editing a file makes only that file unshared.
    std::fs::write(backend.path(&name).join("README.md"), vec![b'x'; 1 << 20]).unwrap();
    assert!(backend.disk_usage(&name).unwrap() >= used + (1 << 20) - 4096);
    backend.prune(&name, id).unwrap();
}

#[test]
fn cleans_up_after_a_failed_create() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = Fixture::new_in(tmp.path());
    let backend = backend_in(tmp.path());
    let name = "fails".parse().unwrap();
    let missing: graft_core::CommitId = "0".repeat(40).parse().unwrap();
    let id = graft_core::GraftId::random().unwrap();
    assert!(
        backend
            .create(
                &fixture.root(),
                &graft_fs::GraftSpec {
                    id,
                    name: &name,
                    base: &missing
                }
            )
            .is_err()
    );
    let left: Vec<_> = std::fs::read_dir(backend.mounts()).unwrap().collect();
    assert!(left.is_empty(), "failed create left {left:?}");
}

#[test]
fn refuses_to_prune_outside_the_mounts_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = Fixture::new_in(tmp.path());
    let backend = backend_in(tmp.path());
    let g = create(&backend, &fixture, "inside");
    // Replace the mounts dir with a symlink to Root: prune must refuse.
    let mounts = backend.mounts().to_owned();
    std::fs::rename(&mounts, tmp.path().join("real-mounts")).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(fixture.dir.path(), &mounts).unwrap();
        let victim = fixture.dir.path().join("src");
        let renamed: graft_core::GraftName = "src".parse().unwrap();
        let err = backend.prune(&renamed, g.id).unwrap_err();
        assert!(
            matches!(err, graft_fs::FsError::OutsideMounts { .. }),
            "{err:?}"
        );
        assert!(
            victim.join("lib.rs").exists(),
            "prune followed a symlinked mounts dir into Root"
        );
        let err = backend
            .create(
                &fixture.root(),
                &graft_fs::GraftSpec {
                    id: g.id,
                    name: &renamed,
                    base: &fixture.base,
                },
            )
            .unwrap_err();
        assert!(
            matches!(err, graft_fs::FsError::OutsideMounts { .. }),
            "{err:?}"
        );
    }
}

/// Pruning a graft with tens of thousands of files (the large fixture's
/// prune failed with `Directory not empty` on the macOS runner) removes all
/// of it, including read-only directories, and nothing outside it: a
/// symlink to Root inside the graft is removed, not followed.
#[test]
fn prunes_a_graft_with_many_files_and_nothing_else() {
    const DIRS: usize = 200;
    const FILES_PER_DIR: usize = 100;
    let tmp = tempfile::tempdir().unwrap();
    let fixture = Fixture::new_in(tmp.path());
    let backend = backend_in(tmp.path());
    let g = create(&backend, &fixture, "many");
    for d in 0..DIRS {
        let dir = g.path.join(format!("gen/{:02}/{d}", d % 16));
        std::fs::create_dir_all(&dir).unwrap();
        for f in 0..FILES_PER_DIR {
            std::fs::write(dir.join(format!("file-{f}.txt")), f.to_string()).unwrap();
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::os::unix::fs::symlink(fixture.dir.path(), g.path.join("root-link")).unwrap();
        let locked = g.path.join("gen/00/0");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    }
    let root_lib = fixture.dir.path().join("src/lib.rs");
    let neighbour = create(&backend, &fixture, "neighbour");

    backend.prune(&g.name, g.id).unwrap();

    assert!(!g.path.exists(), "prune left {}", g.path.display());
    assert!(root_lib.exists(), "prune followed a symlink into Root");
    assert!(
        neighbour.path.join("src/lib.rs").exists(),
        "prune removed another graft"
    );
    let left: Vec<_> = std::fs::read_dir(backend.mounts())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, [std::ffi::OsString::from("neighbour")]);
    backend.prune(&neighbour.name, neighbour.id).unwrap();
}
