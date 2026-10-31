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
