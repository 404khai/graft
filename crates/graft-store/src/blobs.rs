//! The content-addressed delta blob store, `blobs/` (ADR 0008).
//!
//! A blob lives at `blobs/<first two hex chars>/<rest>`, named by the
//! `blake3` hash of its contents. Blobs are written to `blobs/tmp/` and
//! renamed into place, so a blob path either does not exist or is complete.
//! Blobs are read-only once stored.

use std::collections::HashSet;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use graft_core::BlobHash;

use crate::error::{Result, StoreError};

fn io_err<'a>(action: &'static str, path: &'a Path) -> impl FnOnce(io::Error) -> StoreError + 'a {
    move |source| StoreError::Io {
        action,
        path: path.to_owned(),
        source,
    }
}

/// The blob store of one Root.
#[derive(Debug, Clone)]
pub struct BlobStore {
    dir: PathBuf,
}

/// The hash of `data`.
#[must_use]
pub fn hash_bytes(data: &[u8]) -> BlobHash {
    BlobHash::from_bytes(*blake3::hash(data).as_bytes())
}

/// The hash of the file at `path`.
///
/// # Errors
///
/// If it cannot be read.
pub fn hash_file(path: &Path) -> io::Result<BlobHash> {
    let mut hasher = blake3::Hasher::new();
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0; 1 << 16];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                hasher.update(&buf[..n]);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(BlobHash::from_bytes(*hasher.finalize().as_bytes()))
}

fn tmp_name() -> String {
    let mut bytes = [0u8; 8];
    // A collision only makes a rename replace an identical temporary.
    let _ = getrandom::fill(&mut bytes);
    format!("{}-{:016x}", std::process::id(), u64::from_le_bytes(bytes))
}

impl BlobStore {
    /// The store at `dir` (`$GRAFT_HOME/roots/<id>/blobs`).
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// The store's directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where the blob `hash` lives.
    #[must_use]
    pub fn path(&self, hash: &BlobHash) -> PathBuf {
        let (shard, rest) = hash.shard();
        self.dir.join(shard).join(rest)
    }

    /// Whether the blob is stored.
    #[must_use]
    pub fn contains(&self, hash: &BlobHash) -> bool {
        self.path(hash).is_file()
    }

    fn private_dir(dir: &Path) -> Result<()> {
        let mut b = std::fs::DirBuilder::new();
        b.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        b.create(dir).map_err(io_err("creating", dir))
    }

    fn tmp_dir(&self) -> Result<PathBuf> {
        let tmp = self.dir.join("tmp");
        Self::private_dir(&tmp)?;
        Ok(tmp)
    }

    /// Moves the complete file `tmp` into place as its blob.
    fn install(&self, tmp: &Path, sync: bool) -> Result<BlobHash> {
        let hash = hash_file(tmp).map_err(io_err("hashing", tmp))?;
        let target = self.path(&hash);
        if target.is_file() {
            let _ = std::fs::remove_file(tmp);
            return Ok(hash);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(tmp, std::fs::Permissions::from_mode(0o444))
                .map_err(io_err("protecting", tmp))?;
        }
        if sync {
            std::fs::File::open(tmp)
                .and_then(|f| f.sync_all())
                .map_err(io_err("syncing", tmp))?;
        }
        if let Some(shard) = target.parent() {
            Self::private_dir(shard)?;
        }
        std::fs::rename(tmp, &target).map_err(io_err("storing", &target))?;
        Ok(hash)
    }

    /// Stores a copy of the file at `src` (sharing blocks where the
    /// filesystem can) and returns its hash. With `sync`, the blob is on
    /// disk before this returns.
    ///
    /// # Errors
    ///
    /// I/O errors.
    pub fn put_file(&self, src: &Path, sync: bool) -> Result<BlobHash> {
        let tmp = self.tmp_dir()?.join(tmp_name());
        // std::fs::copy clones on APFS and uses copy_file_range on Linux.
        std::fs::copy(src, &tmp).map_err(io_err("copying", src))?;
        let stored = self.install(&tmp, sync);
        if stored.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        stored
    }

    /// Stores `data` and returns its hash.
    ///
    /// # Errors
    ///
    /// I/O errors.
    pub fn put_bytes(&self, data: &[u8]) -> Result<BlobHash> {
        let hash = hash_bytes(data);
        if self.contains(&hash) {
            return Ok(hash);
        }
        let tmp = self.tmp_dir()?.join(tmp_name());
        std::fs::write(&tmp, data).map_err(io_err("writing", &tmp))?;
        let stored = self.install(&tmp, false);
        if stored.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        stored
    }

    /// Deletes every blob not in `keep`, and leftover temporaries. Returns
    /// the number of blobs and bytes removed. The caller must ensure no
    /// blob is being stored or referenced concurrently.
    ///
    /// # Errors
    ///
    /// If the store cannot be listed.
    pub fn sweep(&self, keep: &HashSet<BlobHash>) -> Result<(u64, u64)> {
        let (mut count, mut bytes) = (0, 0);
        let shards = match std::fs::read_dir(&self.dir) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((0, 0)),
            Err(e) => return Err(io_err("listing", &self.dir)(e)),
        };
        for shard in shards.flatten() {
            let name = shard.file_name();
            let name = name.to_string_lossy();
            if name == "tmp" {
                for t in std::fs::read_dir(shard.path())
                    .into_iter()
                    .flatten()
                    .flatten()
                {
                    let _ = std::fs::remove_file(t.path());
                }
                continue;
            }
            if name.len() != 2 || !shard.path().is_dir() {
                continue;
            }
            for blob in std::fs::read_dir(shard.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                let full = format!("{name}{}", blob.file_name().to_string_lossy());
                let Ok(hash) = full.parse::<BlobHash>() else {
                    continue;
                };
                if keep.contains(&hash) {
                    continue;
                }
                let len = blob.metadata().map_or(0, |m| m.len());
                if std::fs::remove_file(blob.path()).is_ok() {
                    count += 1;
                    bytes += len;
                }
            }
        }
        Ok((count, bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_by_content_and_sweeps_unreferenced_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::new(dir.path().join("blobs"));
        let a = store.put_bytes(b"alpha").unwrap();
        assert_eq!(a, hash_bytes(b"alpha"));
        assert_eq!(std::fs::read(store.path(&a)).unwrap(), b"alpha");
        let src = dir.path().join("src");
        std::fs::write(&src, b"beta").unwrap();
        let b = store.put_file(&src, true).unwrap();
        assert_eq!(b, hash_file(&src).unwrap());
        // Storing the same content again is a no-op.
        assert_eq!(store.put_file(&src, false).unwrap(), b);
        assert!(
            std::fs::read_dir(dir.path().join("blobs/tmp"))
                .unwrap()
                .next()
                .is_none()
        );

        let keep: HashSet<_> = [a].into_iter().collect();
        assert_eq!(store.sweep(&keep).unwrap(), (1, 4));
        assert!(store.contains(&a));
        assert!(!store.contains(&b));
    }
}
