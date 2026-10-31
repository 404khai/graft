//! Many processes writing one database at once.
//!
//! The parent test re-runs this test binary as child processes (running the
//! ignored `child_writer` test), so the writers really are separate
//! processes with separate SQLite connections and file locks.

// Fixture helpers outside #[test] fns: panicking on setup failure is the point.
#![allow(clippy::unwrap_used)]
#![allow(clippy::print_stdout)] // Children report results to the parent on stdout.

use std::path::{Path, PathBuf};
use std::process::Command;

use graft_core::{GraftName, RootId, Timestamp};
use graft_store::{Registration, RootRecord, Store, StoreError};

const CHILD_DIR_ENV: &str = "GRAFT_STORE_TEST_CHILD_DIR";
const CHILD_INDEX_ENV: &str = "GRAFT_STORE_TEST_CHILD_INDEX";
const CHILDREN: usize = 8;
const GRAFTS_PER_CHILD: usize = 25;

fn record() -> RootRecord {
    RootRecord {
        id: RootId::from_bytes([3; 16]),
        path: PathBuf::from("/src/app"),
        first_commit: "d".repeat(40).parse().unwrap(),
        registered_at: Timestamp::from_millis(1),
    }
}

/// Runs only as a child of `parallel_processes_do_not_corrupt_the_database`.
#[test]
#[ignore = "spawned by parallel_processes_do_not_corrupt_the_database"]
fn child_writer() {
    let (Ok(dir), Ok(index)) = (std::env::var(CHILD_DIR_ENV), std::env::var(CHILD_INDEX_ENV))
    else {
        return;
    };
    let dir = Path::new(&dir);
    // Every child races to create the database and register the Root.
    let (mut store, registration) = Store::create(dir, &record()).unwrap();
    println!("registration={registration:?}");
    let base = "e".repeat(40).parse().unwrap();
    for i in 0..GRAFTS_PER_CHILD {
        let name: GraftName = format!("c{index}-g{i}").parse().unwrap();
        store.create_graft(&name, &base).unwrap();
    }
    // All children contend for one name; exactly one may win.
    match store.create_graft(&"shared".parse().unwrap(), &base) {
        Ok(_) => println!("shared=won"),
        Err(StoreError::NameTaken { .. }) => println!("shared=taken"),
        Err(e) => panic!("unexpected error: {e}"),
    }
}

#[test]
fn parallel_processes_do_not_corrupt_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let exe = std::env::current_exe().unwrap();
    let children: Vec<_> = (0..CHILDREN)
        .map(|i| {
            Command::new(&exe)
                .args([
                    "--exact",
                    "child_writer",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(CHILD_DIR_ENV, dir.path())
                .env(CHILD_INDEX_ENV, i.to_string())
                .output_async()
        })
        .collect();
    let outputs: Vec<String> = children
        .into_iter()
        .map(|child| {
            let out = child.wait_with_output().unwrap();
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            assert!(
                out.status.success(),
                "child failed:\n{stdout}\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
            stdout
        })
        .collect();

    let count = |needle: &str| outputs.iter().filter(|o| o.contains(needle)).count();
    assert_eq!(
        count(&format!("registration={:?}", Registration::Created)),
        1
    );
    assert_eq!(
        count(&format!("registration={:?}", Registration::Existing)),
        CHILDREN - 1
    );
    assert_eq!(count("shared=won"), 1);
    assert_eq!(count("shared=taken"), CHILDREN - 1);

    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.integrity_check().unwrap(), Vec::<String>::new());
    assert_eq!(store.root().unwrap(), record());
    let grafts = store.grafts().unwrap();
    assert_eq!(grafts.len(), CHILDREN * GRAFTS_PER_CHILD + 1);
    let mut ids: Vec<_> = grafts.iter().map(|g| g.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), grafts.len(), "graft ids must be unique");
}

/// `Command::spawn` with piped output, named for readability above.
trait SpawnPiped {
    fn output_async(&mut self) -> std::process::Child;
}

impl SpawnPiped for Command {
    fn output_async(&mut self) -> std::process::Child {
        self.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    }
}
