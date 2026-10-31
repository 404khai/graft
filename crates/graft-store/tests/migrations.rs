//! Schema migrations, starting from nothing.

// Fixture helpers outside #[test] fns: panicking on setup failure is the point.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use graft_core::{RootId, Timestamp};
use graft_store::{RootRecord, SCHEMA_VERSION, Store, StoreError};
use rusqlite::Connection;

const V1_SQL: &str = include_str!("../src/schema/v1.sql");
const V2_SQL: &str = include_str!("../src/schema/v2.sql");

fn record() -> RootRecord {
    RootRecord {
        id: RootId::from_bytes([7; 16]),
        path: PathBuf::from("/src/app"),
        first_commit: "c".repeat(40).parse().unwrap(),
        registered_at: Timestamp::from_millis(42),
    }
}

fn objects(path: &std::path::Path) -> Vec<(String, String)> {
    let conn = Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT type, name FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn columns(path: &std::path::Path, table: &str) -> Vec<String> {
    let conn = Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap();
    stmt.query_map([], |r| r.get(1))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn released_migrations_are_never_edited() {
    // If this fails you changed a shipped migration. Revert it and add a new
    // migration instead (AGENTS.md §5, docs/on-disk-format.md).
    assert_eq!(SCHEMA_VERSION, 2);
    let hash = |sql: &str| blake3::hash(sql.as_bytes()).to_hex().to_string();
    assert_eq!(hash(V1_SQL), EXPECTED_V1_HASH, "schema/v1.sql changed");
    assert_eq!(hash(V2_SQL), EXPECTED_V2_HASH, "schema/v2.sql changed");
}

const EXPECTED_V2_HASH: &str = "e7d72e92eb8803a978cb5eeb4e055d3119472058ffd86341c0cbb0f468c819ae";
const EXPECTED_V1_HASH: &str = "89777df5ad80faa2b1b85f821edcaa4612e800384cef5ad65943e63718322927";

#[test]
fn migrates_an_empty_directory_to_the_latest_schema() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = Store::create(dir.path(), &record()).unwrap();
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    let db = Store::db_path(dir.path());
    assert_eq!(
        objects(&db),
        [
            ("table", "grafts"),
            ("index", "grafts_name_unique"),
            ("table", "layer_refs"),
            ("index", "layer_refs_by_layer"),
            ("table", "layers"),
            ("table", "op_log"),
            ("table", "roots"),
            ("table", "trusted_commands"),
        ]
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
    );
    assert_eq!(
        columns(&db, "grafts"),
        [
            "graft_id",
            "name",
            "base_commit",
            "state",
            "created_at",
            "updated_at"
        ]
    );
    assert_eq!(
        columns(&db, "op_log"),
        [
            "graft_id", "seq", "at", "kind", "path", "path_to", "blob", "actor"
        ]
    );
    assert_eq!(
        columns(&db, "roots"),
        [
            "singleton",
            "root_id",
            "path",
            "first_commit",
            "registered_at"
        ]
    );
}

#[test]
fn migrates_an_existing_empty_database_file() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::db_path(dir.path());
    std::fs::write(&db, b"").unwrap();
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    assert!(matches!(
        store.root(),
        Err(StoreError::NotInitialised { .. })
    ));
}

#[test]
fn reopening_is_a_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = Store::create(dir.path(), &record()).unwrap();
    drop(store);
    let before = objects(&Store::db_path(dir.path()));
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    assert_eq!(store.root().unwrap(), record());
    assert_eq!(objects(&Store::db_path(dir.path())), before);
}

#[test]
fn refuses_a_database_from_a_newer_graft() {
    let dir = tempfile::tempdir().unwrap();
    Store::create(dir.path(), &record()).unwrap();
    let conn = Connection::open(Store::db_path(dir.path())).unwrap();
    conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    drop(conn);
    let err = Store::open(dir.path()).unwrap_err();
    assert!(
        matches!(err, StoreError::SchemaTooNew { found, supported, .. } if found == SCHEMA_VERSION + 1 && supported == SCHEMA_VERSION),
        "{err:?}"
    );
    assert!(err.to_string().contains("upgrade graft"), "{err}");
}

#[test]
fn a_failed_migration_leaves_the_database_untouched() {
    // A half-applied v1 (a conflicting table) must roll back entirely.
    let dir = tempfile::tempdir().unwrap();
    let db = Store::db_path(dir.path());
    let conn = Connection::open(&db).unwrap();
    conn.execute_batch("CREATE TABLE op_log (x INTEGER);")
        .unwrap();
    drop(conn);
    assert!(Store::open(dir.path()).is_err());
    assert_eq!(objects(&db), [("table".to_owned(), "op_log".to_owned())]);
    let conn = Connection::open(&db).unwrap();
    let v: u32 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(v, 0);
}

#[test]
fn upgrades_a_v1_database_with_data_in_place() {
    // Build a real v1 database by hand, as a v1 graft would have left it.
    let dir = tempfile::tempdir().unwrap();
    let db = Store::db_path(dir.path());
    let conn = Connection::open(&db).unwrap();
    conn.execute_batch(V1_SQL).unwrap();
    conn.pragma_update(None, "user_version", 1).unwrap();
    conn.execute(
        "INSERT INTO roots VALUES (1, ?1, ?2, ?3, 42)",
        (
            record().id.to_string(),
            b"/src/app".to_vec(),
            "c".repeat(40),
        ),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO grafts VALUES (?1, 'auth', ?2, 'live', 1, 2)",
        ("1".repeat(32), "d".repeat(40)),
    )
    .unwrap();
    drop(conn);

    let mut store = Store::open(dir.path()).unwrap();
    assert_eq!(store.schema_version().unwrap(), 2);
    assert_eq!(store.root().unwrap(), record());
    let grafts = store.grafts().unwrap();
    assert_eq!(grafts.len(), 1);
    assert_eq!(grafts[0].name.as_str(), "auth");
    // The new tables work against the old rows.
    let key = graft_core::LayerKey::from_bytes([1; 32]);
    store
        .record_layer(&key, std::path::Path::new("node_modules"), "npm ci")
        .unwrap();
    store.add_layer_ref(grafts[0].id, &key).unwrap();
    assert_eq!(store.layer_ref_count(&key).unwrap(), 1);
    assert_eq!(store.integrity_check().unwrap(), Vec::<String>::new());
}
