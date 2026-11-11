//! Claims admission for virtual grafts' writes (ADR 0018).

use std::path::{Path, PathBuf};

use graft_core::GraftId;
use graft_fs::Admission;
use graft_root::Repo;
use graft_store::Store;

/// Admits each journaled write of a Root's grafts against other live
/// grafts' claims, with graft-sched. Claims are soft: if the store cannot
/// be read, writes are admitted (with a warning) rather than lost.
#[derive(Debug)]
pub struct ClaimsAdmission {
    root_dir: PathBuf,
}

impl ClaimsAdmission {
    /// Admission for the Root at `root_dir` (`$GRAFT_HOME/roots/<id>`).
    #[must_use]
    pub fn new(root_dir: PathBuf) -> Self {
        Self { root_dir }
    }

    fn check(
        &self,
        graft: GraftId,
        path: &[u8],
        contents: &Path,
    ) -> Result<Result<(), String>, String> {
        let shown = String::from_utf8_lossy(path).into_owned();
        let mut store = Store::open(&self.root_dir).map_err(|e| e.to_string())?;
        let on_path = store.live_claims(Some(&shown)).map_err(|e| e.to_string())?;
        let record = store
            .graft_by_id(graft)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("no graft {graft}"))?;
        if on_path.iter().all(|c| c.graft_id == graft) {
            return Ok(Ok(()));
        }
        let root = store.root().map_err(|e| e.to_string())?;
        let repo = Repo::discover(&root.path).map_err(|e| e.to_string())?;
        let rel = graft_core::path_from_bytes(path.to_vec());
        let old = repo
            .read_file_at(&record.base_commit, &rel)
            .map_err(|e| e.to_string())?;
        let new = std::fs::read(contents).map_err(|e| e.to_string())?;
        if new.is_empty() {
            // `open(O_TRUNC)` then `write`: the emptied file can be journaled
            // before its new contents arrive (NFS). Those are checked when
            // they are journaled.
            return Ok(Ok(()));
        }
        let changed = graft_radar::symbols::changed_symbols(&rel, old.as_deref(), Some(&new));
        let all: Vec<graft_sched::admission::Claim> = store
            .live_claims(None)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|c| graft_sched::admission::Claim {
                graft: c.graft,
                path: PathBuf::from(c.path),
                symbol: c.symbol,
                plan: c.plan,
            })
            .collect();
        let mine = all
            .iter()
            .filter(|c| c.graft == record.name.as_str())
            .find_map(|c| c.plan.clone());
        let verdict = graft_sched::admission::admit(
            record.name.as_str(),
            mine.as_ref(),
            &rel,
            changed.as_ref(),
            &all,
        );
        if verdict == graft_sched::admission::Verdict::Allowed {
            return Ok(Ok(()));
        }
        let text = verdict.describe();
        store
            .record_admission(record.name.as_str(), &shown, &text)
            .map_err(|e| e.to_string())?;
        Ok(Err(text))
    }
}

impl Admission for ClaimsAdmission {
    fn admit(&self, graft: GraftId, path: &[u8], contents: &Path) -> Result<(), String> {
        match self.check(graft, path, contents) {
            Ok(verdict) => verdict,
            Err(e) => {
                tracing::warn!(error = %e, "claims admission could not run; write admitted");
                Ok(())
            }
        }
    }
}
