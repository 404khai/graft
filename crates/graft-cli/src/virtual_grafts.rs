//! Status, diff, disk use and prune safety for virtual (FUSE) grafts, from
//! their delta (ADR 0008).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use graft_root::graft::{ChangeKind, FileDiff, GraftRepo};
use graft_root::tree::ObjectStore;
use graft_store::GraftRecord;
use graft_store::blobs::BlobStore;
use graft_store::registry::Located;

/// What graft `record` changed against its base, with contents, hiding
/// added paths its ignore rules exclude, as git status does. The ignore
/// rules are read through the mount at `mount`; if it cannot be read, the
/// unfiltered changes are returned with a warning.
/// Asks graftd to journal the graft's pending writes (NFS grafts journal a
/// file only once it goes idle), so the delta read next is current.
pub(crate) fn sync(
    home: &graft_core::GraftHome,
    located: &Located,
    record: &GraftRecord,
    warnings: &mut Vec<String>,
) {
    let Ok(mut client) = graft_daemon::Client::connect(home) else {
        return;
    };
    if let Err(e) = client.sync(&located.record.id, &record.name) {
        warnings.push(format!(
            "graft {}: recent writes may be missing ({e})",
            record.name
        ));
    }
}

pub(crate) fn diffs(
    home: &graft_core::GraftHome,
    located: &Located,
    root_dir: &Path,
    mount: &Path,
    record: &GraftRecord,
    warnings: &mut Vec<String>,
) -> anyhow::Result<Vec<FileDiff>> {
    sync(home, located, record, warnings);
    let objects = ObjectStore::open(&located.repo);
    let layers = graft_fs::layer_mounts(&located.store, root_dir, record.id)?;
    let delta = located.store.delta(record.id)?;
    let blobs = BlobStore::new(root_dir.join("blobs"));
    let diffs = graft_fs::virtual_diffs(&objects, &record.base_commit, layers, &delta, &blobs)?;
    let added: Vec<PathBuf> = diffs
        .iter()
        .filter(|d| d.change.kind == ChangeKind::Added)
        .map(|d| d.change.path.clone())
        .collect();
    if added.is_empty() {
        return Ok(diffs);
    }
    match GraftRepo::open(mount).and_then(|g| g.ignored(&added)) {
        Ok(ignored) => Ok(graft_fs::without_ignored(diffs, &ignored)),
        Err(e) => {
            warnings.push(format!(
                "cannot read graft {}'s ignore rules ({e}); ignored files are listed too",
                record.name
            ));
            Ok(diffs)
        }
    }
}

/// Bytes the graft stores of its own.
pub(crate) fn disk(
    located: &Located,
    root_dir: &Path,
    record: &GraftRecord,
) -> anyhow::Result<u64> {
    Ok(graft_fs::virtual_disk_usage(
        &located.store,
        root_dir,
        record.id,
    )?)
}

/// Why pruning would lose work, if it would: changes against the base, or
/// commits in the graft's `.git` that Root does not have.
pub(crate) fn work_at_risk(
    home: &graft_core::GraftHome,
    located: &Located,
    root_dir: &Path,
    mount: &Path,
    record: &GraftRecord,
) -> anyhow::Result<Option<String>> {
    let mut why = String::new();
    let mut warnings = Vec::new();
    let changes = diffs(home, located, root_dir, mount, record, &mut warnings)?;
    if !changes.is_empty() {
        let shown: Vec<_> = changes
            .iter()
            .take(5)
            .map(|d| d.change.path.display().to_string())
            .collect();
        let _ = write!(why, "  changes since base: {}", shown.join(", "));
        if changes.len() > shown.len() {
            let _ = write!(why, " (and {} more)", changes.len() - shown.len());
        }
        why.push('\n');
    }
    let git_dir = root_dir
        .join("grafts")
        .join(record.id.to_string())
        .join("git");
    if git_dir.join("HEAD").is_file() {
        match GraftRepo::open(mount).and_then(|g| g.unintegrated(&located.repo)) {
            Ok(lost) => {
                for l in lost {
                    let _ = writeln!(
                        why,
                        "  commit {} on {} is not in Root",
                        l.commit.short(7),
                        l.reference
                    );
                }
            }
            Err(e) => {
                let _ = writeln!(why, "  cannot inspect the graft's commits: {e}");
            }
        }
    }
    Ok((!why.is_empty()).then_some(why))
}
