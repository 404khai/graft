//! Bud, Ring and rewind in the store (ADR 0013).

// Fixture helpers outside #[test] fns: panicking on setup failure is the point.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use graft_core::{BackendKind, BlobHash, GraftId, GraftState, RingLabel, RootId, Timestamp};
use graft_store::delta::{Delta, Lower, LowerKind, Op, WriteKind};
use graft_store::{Journaled, RootRecord, Staged, Store, StoreError};

struct NoLower;

impl Lower for NoLower {
    fn kind(&self, path: &[u8]) -> Option<LowerKind> {
        path.is_empty().then_some(LowerKind::Dir { mode: 0o755 })
    }
    fn children(&self, _: &[u8]) -> Vec<Vec<u8>> {
        Vec::new()
    }
}

fn store() -> (tempfile::TempDir, Store, GraftId) {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, _) = Store::create(
        dir.path(),
        &RootRecord {
            id: RootId::from_bytes([3; 16]),
            path: PathBuf::from("/r"),
            first_commit: "a".repeat(40).parse().unwrap(),
            registered_at: Timestamp::from_millis(0),
        },
    )
    .unwrap();
    let g = store
        .create_graft(
            &"src".parse().unwrap(),
            &"b".repeat(40).parse().unwrap(),
            BackendKind::Fuse,
        )
        .unwrap();
    store.set_state(g.id, GraftState::Live).unwrap();
    (dir, store, g.id)
}

fn blob(n: u32) -> BlobHash {
    graft_store::blobs::hash_bytes(&n.to_le_bytes())
}

/// Writes file `f<n>` with blob `n`, journaled like the write path does.
fn write(store: &mut Store, id: GraftId, n: u32) {
    let mut delta = store.delta(id).unwrap();
    let op = Op::Write {
        kind: WriteKind::Create,
        path: format!("f{n}").into_bytes(),
        blob: blob(n),
        mode: 0o644,
    };
    let at = Timestamp::from_millis(i64::from(n));
    let changes = delta.apply(&op, at, &NoLower);
    store
        .record(
            id,
            &Journaled {
                at,
                op: &op,
                actor: None,
                changes: &changes,
                staged: &[],
                sync: false,
            },
        )
        .unwrap();
}

fn unlink(store: &mut Store, id: GraftId, n: u32) {
    let mut delta = store.delta(id).unwrap();
    let op = Op::Unlink {
        path: format!("f{n}").into_bytes(),
    };
    let at = Timestamp::from_millis(1000 + i64::from(n));
    let changes = delta.apply(&op, at, &NoLower);
    store
        .record(
            id,
            &Journaled {
                at,
                op: &op,
                actor: None,
                changes: &changes,
                staged: &[],
                sync: false,
            },
        )
        .unwrap();
}

#[test]
fn a_bud_copies_the_delta_and_op_log_and_shares_blobs() {
    let (_dir, mut store, src) = store();
    for n in 0..20 {
        write(&mut store, src, n);
    }
    let bud = store.bud(src, &"bud".parse().unwrap()).unwrap();
    let source = store.graft_by_id(src).unwrap().unwrap();
    assert_eq!(bud.base_commit, source.base_commit);
    assert_eq!(bud.backend, source.backend);
    assert_eq!(bud.state, GraftState::Created);
    assert_eq!(bud.budded_from, Some(src));
    assert_eq!(store.delta(bud.id).unwrap(), store.delta(src).unwrap());
    let log = store.op_log(bud.id).unwrap();
    assert_eq!(log, store.op_log(src).unwrap());
    assert_eq!(Delta::replay(&log, &NoLower), store.delta(bud.id).unwrap());

    // The two diverge independently.
    write(&mut store, bud.id, 100);
    unlink(&mut store, src, 0);
    assert!(store.delta(bud.id).unwrap().get(b"f0").is_some());
    assert!(store.delta(src).unwrap().get(b"f100").is_none());
}

#[test]
fn bud_names_must_be_free_and_sources_live() {
    let (_dir, mut store, src) = store();
    assert!(matches!(
        store.bud(src, &"src".parse().unwrap()),
        Err(StoreError::NameTaken { .. })
    ));
    store.set_state(src, GraftState::Pruned).unwrap();
    assert!(matches!(
        store.bud(src, &"other".parse().unwrap()),
        Err(StoreError::InvalidTransition { .. })
    ));
    assert_eq!(store.grafts().unwrap().len(), 1);
}

#[test]
fn pruning_a_source_keeps_the_blobs_its_bud_uses() {
    let (_dir, mut store, src) = store();
    write(&mut store, src, 1);
    write(&mut store, src, 2);
    let bud = store.bud(src, &"bud".parse().unwrap()).unwrap();
    unlink(&mut store, bud.id, 2);
    store.delete_graft(src).unwrap();
    let kept = store.referenced_blobs().unwrap();
    assert!(kept.contains(&blob(1)));
    assert!(
        !kept.contains(&blob(2)),
        "nothing references blob 2 any more"
    );
}

#[test]
fn rings_keep_their_blobs_until_their_graft_goes() {
    let (_dir, mut store, src) = store();
    write(&mut store, src, 1);
    let ring = store.ring(src, Some("before".parse().unwrap())).unwrap();
    assert_eq!((ring.seq, ring.paths), (1, 1));
    unlink(&mut store, src, 1);
    assert!(store.delta(src).unwrap().is_empty());
    assert!(store.referenced_blobs().unwrap().contains(&blob(1)));

    // A bud's rings protect blobs after its source is pruned.
    let bud = store.bud(src, &"bud".parse().unwrap()).unwrap();
    store.set_state(bud.id, GraftState::Live).unwrap();
    // A bud does not inherit its source's rings.
    assert!(matches!(
        store.rewind(
            bud.id,
            &"before".parse().unwrap(),
            Timestamp::from_millis(9)
        ),
        Err(StoreError::NoSuchRing { .. })
    ));
    write(&mut store, bud.id, 7);
    store.ring(bud.id, None).unwrap();
    unlink(&mut store, bud.id, 7);
    store.delete_graft(src).unwrap();
    let kept = store.referenced_blobs().unwrap();
    assert!(kept.contains(&blob(7)), "the bud's ring holds blob 7");
    assert!(
        !kept.contains(&blob(1)),
        "the pruned source's ring went with it"
    );
}

#[test]
fn rewind_restores_a_ring_and_its_undo_ring_reapplies() {
    let (_dir, mut store, src) = store();
    write(&mut store, src, 1);
    let label: RingLabel = "one".parse().unwrap();
    store.ring(src, Some(label.clone())).unwrap();
    let at_ring = store.delta(src).unwrap();
    write(&mut store, src, 2);
    unlink(&mut store, src, 1);
    let before = store.delta(src).unwrap();

    let rewound = store
        .rewind(src, &label, Timestamp::from_millis(50))
        .unwrap();
    assert_eq!(rewound.undo.as_str(), "undo-3");
    assert_eq!(rewound.seq, 4);
    assert_eq!(store.delta(src).unwrap(), at_ring);
    let log = store.op_log(src).unwrap();
    assert_eq!(log.last().unwrap().op, Op::Rewind { to: 1 });
    assert_eq!(Delta::replay(&log, &NoLower), at_ring);

    store
        .rewind(src, &rewound.undo, Timestamp::from_millis(60))
        .unwrap();
    assert_eq!(store.delta(src).unwrap(), before);
    assert_eq!(Delta::replay(&store.op_log(src).unwrap(), &NoLower), before);
    let labels: Vec<_> = store
        .rings(src)
        .unwrap()
        .into_iter()
        .map(|r| r.label.to_string())
        .collect();
    assert_eq!(labels, ["one", "undo-3", "undo-4"]);
}

#[test]
fn rings_and_rewinds_refuse_what_they_cannot_do() {
    let (_dir, mut store, src) = store();
    store.ring(src, Some("a".parse().unwrap())).unwrap();
    assert!(matches!(
        store.ring(src, Some("a".parse().unwrap())),
        Err(StoreError::RingTaken { .. })
    ));
    assert!(matches!(
        store.rewind(src, &"nope".parse().unwrap(), Timestamp::from_millis(1)),
        Err(StoreError::NoSuchRing { .. })
    ));
    store
        .stage(
            src,
            &Staged {
                id: 1,
                path: b"open".to_vec(),
                mode: 0o644,
            },
        )
        .unwrap();
    assert!(matches!(
        store.rewind(src, &"a".parse().unwrap(), Timestamp::from_millis(1)),
        Err(StoreError::StillWriting { files: 1, .. })
    ));
    assert_eq!(store.op_log(src).unwrap(), Vec::new());
    // A ring of an empty graft rewinds to the empty delta.
    store.unstage(src, 1).unwrap();
    write(&mut store, src, 5);
    store
        .rewind(src, &"a".parse().unwrap(), Timestamp::from_millis(2))
        .unwrap();
    assert!(store.delta(src).unwrap().is_empty());
}
