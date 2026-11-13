//! The op log decoder: arbitrary rows in `op_log` and `delta` (as a damaged
//! or hostile database would hold them) must decode or fail with
//! `Corrupt`, never panic.
#![no_main]

use std::path::PathBuf;

use arbitrary::Arbitrary;
use graft_core::{BackendKind, RootId, Timestamp};
use graft_store::{RootRecord, Store};
use libfuzzer_sys::fuzz_target;
use rusqlite::params;

#[derive(Debug, Arbitrary)]
struct Row {
    kind: u8,
    raw_kind: String,
    path: Vec<u8>,
    path_to: Option<Vec<u8>>,
    blob: Option<String>,
    mode: Option<i64>,
    mtime: Option<i64>,
    target: Option<Vec<u8>>,
    to_seq: Option<i64>,
    delta_kind: u8,
    lower: Option<Vec<u8>>,
}

const KINDS: &[&str] = &[
    "create", "write", "truncate", "mkdir", "symlink", "unlink", "rmdir", "rename", "chmod",
    "touch", "rewind", "rebase",
];

fuzz_target!(|rows: Vec<Row>| {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, _) = Store::create(
        dir.path(),
        &RootRecord {
            id: RootId::from_bytes([9; 16]),
            path: PathBuf::from("/r"),
            first_commit: "a".repeat(40).parse().unwrap(),
            registered_at: Timestamp::from_millis(0),
        },
    )
    .unwrap();
    let g = store
        .create_graft(&"g".parse().unwrap(), &"a".repeat(40).parse().unwrap(), BackendKind::Fuse)
        .unwrap();
    drop(store);
    let conn = rusqlite::Connection::open(Store::db_path(dir.path())).unwrap();
    let gid = g.id.to_string();
    for (i, r) in rows.iter().take(32).enumerate() {
        let kind = KINDS.get(usize::from(r.kind)).map_or(r.raw_kind.as_str(), |k| k);
        let blob = r.blob.as_ref().map(|b| b.chars().take(64).collect::<String>());
        // The schema's CHECKs reject some rows; that is fine.
        let _ = conn.execute(
            "INSERT INTO op_log (graft_id, seq, at, kind, path, path_to, blob, actor, mode, mtime, target, to_seq)
             VALUES (?1, ?2, 0, ?3, ?4, ?5, ?6, NULL, ?7, ?8, ?9, ?10)",
            params![gid, i as i64 + 1, kind, r.path, r.path_to, blob, r.mode, r.mtime, r.target, r.to_seq],
        );
        let dkind = ["file", "symlink", "dir", "tombstone"][usize::from(r.delta_kind) % 4];
        let _ = conn.execute(
            "INSERT OR REPLACE INTO delta (graft_id, path, kind, blob, lower, target, mode, mtime, opaque)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0)",
            params![gid, r.path, dkind, r.blob, r.lower, r.target, r.mode, r.mtime],
        );
    }
    drop(conn);
    let store = Store::open(dir.path()).unwrap();
    let _ = store.op_log(g.id);
    let _ = store.delta(g.id);
});
