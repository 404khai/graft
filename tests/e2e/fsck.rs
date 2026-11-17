//! `graft fsck`: finds damage to a Root's stored state, and changes nothing.

use std::path::Path;

use graft_core::{BackendKind, GraftHome, Timestamp};
use graft_store::Journaled;
use graft_store::blobs::BlobStore;
use graft_store::delta::{Delta, Lower, LowerKind, Op, WriteKind};
use predicates::prelude::*;

use crate::{git_repo, graft};

/// A base where nothing exists: enough for ops on new paths.
struct Empty;

impl Lower for Empty {
    fn kind(&self, _: &[u8]) -> Option<LowerKind> {
        None
    }
    fn children(&self, _: &[u8]) -> Vec<Vec<u8>> {
        Vec::new()
    }
}

fn fsck(home: &Path, repo: &Path) -> assert_cmd::assert::Assert {
    graft(home)
        .args(["fsck", "--json"])
        .current_dir(repo)
        .assert()
}

fn findings(a: &assert_cmd::assert::Assert) -> serde_json::Value {
    serde_json::from_slice(&a.get_output().stdout).unwrap()
}

/// A virtual graft's records written straight to the store, as graftd
/// would journal them, so no mount is needed. Returns the blob's path.
fn journal_a_write(home: &Path, repo: &Path) -> std::path::PathBuf {
    let home = GraftHome::from_path(home).unwrap();
    let mut located = graft_store::registry::locate(&home, repo).unwrap();
    let root_dir = home.root_dir(&located.record.id);
    let head = located.repo.head_commit().unwrap();
    let g = located
        .store
        .create_graft(&"virt".parse().unwrap(), &head, BackendKind::Fuse)
        .unwrap();
    let blobs = BlobStore::new(root_dir.join("blobs"));
    let blob = blobs.put_bytes(b"journaled\n").unwrap();
    let op = Op::Write {
        kind: WriteKind::Create,
        path: b"new.txt".to_vec(),
        blob,
        mode: 0o644,
    };
    let at = Timestamp::now();
    let changes = Delta::default().apply(&op, at, &Empty);
    located
        .store
        .record(
            g.id,
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
    blobs.path(&blob)
}

fn init(home: &Path, repo: &Path) {
    graft(home).arg("init").current_dir(repo).assert().success();
}

#[test]
fn a_healthy_root_has_no_findings() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    init(home.path(), repo.path());
    graft(home.path())
        .args(["new", "a"])
        .current_dir(repo.path())
        .assert()
        .success();
    journal_a_write(home.path(), repo.path());
    let a = fsck(home.path(), repo.path()).success();
    let v = findings(&a);
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["findings"], serde_json::json!([]), "{v}");
    assert_eq!(v["checked"]["grafts"], 2);
    assert_eq!(v["checked"]["ops_replayed"], 1);
    assert_eq!(v["checked"]["blobs_hashed"], 1);
    graft(home.path())
        .arg("fsck")
        .current_dir(repo.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("no problems found"));
}

#[test]
fn finds_a_corrupt_or_missing_blob() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    init(home.path(), repo.path());
    let blob = journal_a_write(home.path(), repo.path());
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&blob, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    std::fs::write(&blob, "bit rot\n").unwrap();

    let v = findings(&fsck(home.path(), repo.path()).failure());
    assert_eq!(v["ok"], false);
    let f = &v["findings"][0];
    assert_eq!(f["check"], "blobs", "{v}");
    assert!(
        f["message"]
            .as_str()
            .unwrap()
            .contains("do not match their hash"),
        "{v}"
    );

    // --quick only checks that it exists.
    graft(home.path())
        .args(["fsck", "--quick"])
        .current_dir(repo.path())
        .assert()
        .success();
    std::fs::remove_file(&blob).unwrap();
    let v = findings(&fsck(home.path(), repo.path()).failure());
    assert!(
        v["findings"][0]["message"]
            .as_str()
            .unwrap()
            .contains("missing"),
        "{v}"
    );
}

#[test]
fn finds_a_delta_the_op_log_does_not_reproduce() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    init(home.path(), repo.path());
    journal_a_write(home.path(), repo.path());
    // Lose the delta but keep the op log, as a bug in the journal would.
    let gh = GraftHome::from_path(home.path()).unwrap();
    let mut located = graft_store::registry::locate(&gh, repo.path()).unwrap();
    let g = located
        .store
        .graft(&"virt".parse().unwrap())
        .unwrap()
        .unwrap();
    located.store.clear_delta(g.id).unwrap();

    let v = findings(&fsck(home.path(), repo.path()).failure());
    let f = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["check"] == "op_log_replay")
        .unwrap_or_else(|| panic!("no replay finding in {v}"));
    assert_eq!(f["graft"], "virt");
    assert!(f["message"].as_str().unwrap().contains("new.txt"), "{f}");
}

#[test]
fn finds_a_missing_working_tree_and_stray_mounts() {
    let home = tempfile::tempdir().unwrap();
    let repo = git_repo();
    init(home.path(), repo.path());
    graft(home.path())
        .args(["new", "a"])
        .current_dir(repo.path())
        .assert()
        .success();
    let out = graft(home.path())
        .args(["path", "a"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let tree = std::path::PathBuf::from(String::from_utf8(out.stdout).unwrap().trim());
    let mounts = tree.parent().unwrap().to_owned();
    std::fs::create_dir(mounts.join("leftover")).unwrap();

    // A stray directory is a warning, not a failure.
    let v = findings(&fsck(home.path(), repo.path()).success());
    assert_eq!(v["findings"][0]["severity"], "warning", "{v}");
    assert!(
        v["findings"][0]["message"]
            .as_str()
            .unwrap()
            .contains("leftover")
    );

    std::fs::remove_dir_all(&tree).unwrap();
    let v = findings(&fsck(home.path(), repo.path()).failure());
    assert_eq!(v["findings"][0]["check"], "working_tree", "{v}");
    assert_eq!(v["findings"][0]["graft"], "a");
    // Read-only: the stray directory is still there.
    assert!(mounts.join("leftover").is_dir());
}
