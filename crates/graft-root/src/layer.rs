//! The layer cache: directory trees built once per input hash and shared
//! by every graft (ADR 0004).
//!
//! This module owns the cache directory (`$GRAFT_HOME/roots/<id>/layers/`)
//! and how keys are derived. Which grafts reference which layers is recorded
//! in the store; builders and `graft gc` coordinate through
//! [`LayerCache::lock`].

use std::fs::File;
use std::path::{Path, PathBuf};

use graft_core::{CommandHash, LayerKey};

use crate::{Result, RootError};

const LAYER_KEY_CONTEXT: &str = "graft layer v1";
const COMMAND_HASH_CONTEXT: &str = "graft trust v1";

fn field(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

/// Everything that determines a layer's contents.
#[derive(Debug, Clone, Copy)]
pub struct KeyInputs<'a> {
    /// The layer's path in a graft.
    pub path: &'a Path,
    /// The build command.
    pub build: &'a str,
    /// The toolchain command, if any.
    pub toolchain: Option<&'a str>,
    /// What the toolchain command printed.
    pub toolchain_output: &'a [u8],
    /// Each input file's path and its contents at the base commit.
    pub files: &'a [(PathBuf, Vec<u8>)],
}

/// Derives a layer's key. Every field is length-prefixed, so no two
/// different inputs can produce the same byte stream. The OS and
/// architecture are included, because build outputs are platform-specific.
#[must_use]
pub fn layer_key(inputs: &KeyInputs<'_>) -> LayerKey {
    let mut h = blake3::Hasher::new_derive_key(LAYER_KEY_CONTEXT);
    field(&mut h, &graft_core::path_to_bytes(inputs.path));
    field(&mut h, inputs.build.as_bytes());
    field(&mut h, inputs.toolchain.unwrap_or_default().as_bytes());
    field(&mut h, inputs.toolchain_output);
    field(&mut h, std::env::consts::OS.as_bytes());
    field(&mut h, std::env::consts::ARCH.as_bytes());
    h.update(&(inputs.files.len() as u64).to_le_bytes());
    for (path, contents) in inputs.files {
        field(&mut h, &graft_core::path_to_bytes(path));
        field(&mut h, contents);
    }
    LayerKey::from_bytes(*h.finalize().as_bytes())
}

/// Identifies a layer's commands for the trust gate: approving a layer
/// approves exactly these commands for this path.
#[must_use]
pub fn command_hash(path: &Path, build: &str, toolchain: Option<&str>) -> CommandHash {
    let mut h = blake3::Hasher::new_derive_key(COMMAND_HASH_CONTEXT);
    field(&mut h, &graft_core::path_to_bytes(path));
    field(&mut h, build.as_bytes());
    field(&mut h, toolchain.unwrap_or_default().as_bytes());
    CommandHash::from_bytes(*h.finalize().as_bytes())
}

/// An entry in the cache directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheEntry {
    /// A complete layer.
    Ready(LayerKey),
    /// A partially installed layer left by an interrupted build.
    Temp {
        /// The layer it was going to become.
        key: LayerKey,
        /// Its path.
        path: PathBuf,
    },
}

/// Held while deciding whether a layer exists and acting on it. Released
/// on drop.
#[derive(Debug)]
pub struct LayerLock {
    _file: File,
}

/// The cache directory for one Root.
#[derive(Debug, Clone)]
pub struct LayerCache {
    dir: PathBuf,
}

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> RootError + '_ {
    move |source| RootError::Io {
        path: path.to_owned(),
        source,
    }
}

impl LayerCache {
    /// The cache in `dir` (`$GRAFT_HOME/roots/<id>/layers`). Nothing is
    /// created until it is needed.
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// The cache directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where the layer `key` lives (whether or not it exists).
    #[must_use]
    pub fn path(&self, key: &LayerKey) -> PathBuf {
        self.dir.join(key.to_string())
    }

    /// Whether the layer `key` is complete in the cache.
    #[must_use]
    pub fn is_ready(&self, key: &LayerKey) -> bool {
        std::fs::symlink_metadata(self.path(key)).is_ok_and(|m| m.is_dir())
    }

    fn ensure_dir(&self) -> Result<()> {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(&self.dir).map_err(io_err(&self.dir))
    }

    fn lock_path(&self, key: &LayerKey) -> PathBuf {
        self.dir.join(format!("{key}.lock"))
    }

    fn open_lock(&self, key: &LayerKey) -> Result<File> {
        self.ensure_dir()?;
        let path = self.lock_path(key);
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(io_err(&path))
    }

    /// Takes the layer's lock, waiting for any other holder.
    ///
    /// # Errors
    ///
    /// If the lock file cannot be opened or locked.
    pub fn lock(&self, key: &LayerKey) -> Result<LayerLock> {
        let file = self.open_lock(key)?;
        file.lock().map_err(io_err(&self.lock_path(key)))?;
        Ok(LayerLock { _file: file })
    }

    /// Takes the layer's lock if nobody holds it.
    ///
    /// # Errors
    ///
    /// If the lock file cannot be opened.
    pub fn try_lock(&self, key: &LayerKey) -> Result<Option<LayerLock>> {
        let file = self.open_lock(key)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(LayerLock { _file: file })),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(e)) => Err(io_err(&self.lock_path(key))(e)),
        }
    }

    /// Moves the freshly built directory `built` into the cache as `key`.
    /// The move goes through a temporary name, so a layer directory is
    /// always complete. Call with the layer's lock held.
    ///
    /// # Errors
    ///
    /// If `built` is not a directory, a layer `key` already exists, or the
    /// move fails (for example across filesystems).
    pub fn install(&self, key: &LayerKey, built: &Path, _lock: &LayerLock) -> Result<PathBuf> {
        self.ensure_dir()?;
        let meta = std::fs::symlink_metadata(built).map_err(io_err(built))?;
        if !meta.is_dir() {
            return Err(RootError::Io {
                path: built.to_owned(),
                source: std::io::Error::other("the build did not produce a directory"),
            });
        }
        let target = self.path(key);
        if std::fs::symlink_metadata(&target).is_ok() {
            return Err(RootError::Io {
                path: target,
                source: std::io::ErrorKind::AlreadyExists.into(),
            });
        }
        let temp = self.dir.join(format!(".tmp-{key}"));
        std::fs::rename(built, &temp).map_err(io_err(&temp))?;
        std::fs::rename(&temp, &target).map_err(io_err(&target))?;
        Ok(target)
    }

    /// Every layer and leftover temporary in the cache.
    ///
    /// # Errors
    ///
    /// If the cache directory exists but cannot be read.
    pub fn entries(&self) -> Result<Vec<CacheEntry>> {
        let read = match std::fs::read_dir(&self.dir) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_err(&self.dir)(e)),
        };
        let mut out = Vec::new();
        for entry in read.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if let Some(key) = name.strip_prefix(".tmp-").and_then(|k| k.parse().ok()) {
                out.push(CacheEntry::Temp {
                    key,
                    path: entry.path(),
                });
            } else if let Ok(key) = name.parse() {
                out.push(CacheEntry::Ready(key));
            }
        }
        out.sort_by_key(|e| match e {
            CacheEntry::Ready(k) | CacheEntry::Temp { key: k, .. } => *k,
        });
        Ok(out)
    }

    /// Removes a cache entry's directory (and, for a ready layer, its lock
    /// file). Call with the layer's lock held.
    ///
    /// # Errors
    ///
    /// If the directory exists but cannot be removed.
    pub fn remove(&self, entry: &CacheEntry, _lock: &LayerLock) -> Result<()> {
        let path = match entry {
            CacheEntry::Ready(key) => self.path(key),
            CacheEntry::Temp { path, .. } => path.clone(),
        };
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.is_dir() => remove_tree(&path)?,
            Ok(_) => std::fs::remove_file(&path).map_err(io_err(&path))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_err(&path)(e)),
        }
        Ok(())
    }
}

/// `remove_dir_all`, retrying after making directories writable (some
/// package managers mark them read-only). Never follows symlinks.
fn remove_tree(path: &Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => return Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {}
        Err(e) => return Err(io_err(path)(e)),
    }
    make_writable(path).map_err(io_err(path))?;
    std::fs::remove_dir_all(path).map_err(io_err(path))
}

fn make_writable(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::symlink_metadata(dir)?;
        if !meta.is_dir() {
            return Ok(());
        }
        let mode = meta.permissions().mode();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode | 0o700))?;
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                make_writable(&entry.path())?;
            }
        }
    }
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs<'a>(files: &'a [(PathBuf, Vec<u8>)], build: &'a str) -> KeyInputs<'a> {
        KeyInputs {
            path: Path::new("node_modules"),
            build,
            toolchain: Some("node --version"),
            toolchain_output: b"v24.0.0\n",
            files,
        }
    }

    #[test]
    fn keys_change_with_every_input() {
        let files = vec![(PathBuf::from("package-lock.json"), b"{}".to_vec())];
        let base = layer_key(&inputs(&files, "npm ci"));
        assert_eq!(base, layer_key(&inputs(&files, "npm ci")));
        assert_ne!(base, layer_key(&inputs(&files, "npm install")));
        let changed = vec![(PathBuf::from("package-lock.json"), b"{ }".to_vec())];
        assert_ne!(base, layer_key(&inputs(&changed, "npm ci")));
        let renamed = vec![(PathBuf::from("other.json"), b"{}".to_vec())];
        assert_ne!(base, layer_key(&inputs(&renamed, "npm ci")));
        let mut other_node = inputs(&files, "npm ci");
        other_node.toolchain_output = b"v22.0.0\n";
        assert_ne!(base, layer_key(&other_node));
    }

    #[test]
    fn length_prefixing_prevents_ambiguity() {
        // "ab" + "c" must not hash like "a" + "bc".
        let a = vec![(PathBuf::from("ab"), b"c".to_vec())];
        let b = vec![(PathBuf::from("a"), b"bc".to_vec())];
        assert_ne!(layer_key(&inputs(&a, "x")), layer_key(&inputs(&b, "x")));
    }

    #[test]
    fn command_hashes_cover_path_and_commands() {
        let p = Path::new("node_modules");
        let h = command_hash(p, "npm ci", None);
        assert_eq!(h, command_hash(p, "npm ci", None));
        assert_ne!(h, command_hash(p, "npm ci && curl evil", None));
        assert_ne!(h, command_hash(p, "npm ci", Some("node -v")));
        assert_ne!(h, command_hash(Path::new("vendor"), "npm ci", None));
    }

    #[test]
    fn command_hashes_are_stable_across_releases() {
        // A change here revokes every recorded approval: bump the context
        // string and the on-disk format together. (Layer keys include the
        // OS and architecture, so they are not pinned here.)
        let hash = command_hash(Path::new("target"), "cargo build", None);
        assert_eq!(hash.to_string(), EXPECTED_COMMAND_HASH);
    }

    const EXPECTED_COMMAND_HASH: &str =
        "b9de224c2517591e0c36443e1f2c7ba23df833c37854aef44eb5675f73bda3a4";

    #[test]
    fn install_lock_and_remove() {
        let dir = tempfile::tempdir().unwrap();
        let cache = LayerCache::new(dir.path().join("layers"));
        let key = LayerKey::from_bytes([1; 32]);
        assert!(!cache.is_ready(&key));
        assert_eq!(cache.entries().unwrap(), []);

        let lock = cache.lock(&key).unwrap();
        assert!(
            cache.try_lock(&key).unwrap().is_none(),
            "lock must be exclusive"
        );
        let built = dir.path().join("node_modules");
        std::fs::create_dir_all(built.join("pkg")).unwrap();
        std::fs::write(built.join("pkg/index.js"), "x").unwrap();
        let installed = cache.install(&key, &built, &lock).unwrap();
        assert!(cache.is_ready(&key));
        assert!(!built.exists());
        assert_eq!(
            std::fs::read_to_string(installed.join("pkg/index.js")).unwrap(),
            "x"
        );
        assert_eq!(cache.entries().unwrap(), [CacheEntry::Ready(key)]);

        std::fs::create_dir(dir.path().join("again")).unwrap();
        assert!(
            cache
                .install(&key, &dir.path().join("again"), &lock)
                .is_err()
        );
        cache.remove(&CacheEntry::Ready(key), &lock).unwrap();
        assert!(!cache.is_ready(&key));
        drop(lock);
        assert!(cache.try_lock(&key).unwrap().is_some());
    }

    #[test]
    fn lists_interrupted_installs() {
        let dir = tempfile::tempdir().unwrap();
        let cache = LayerCache::new(dir.path().to_owned());
        let key = LayerKey::from_bytes([2; 32]);
        std::fs::create_dir(dir.path().join(format!(".tmp-{key}"))).unwrap();
        std::fs::write(dir.path().join("unrelated"), "").unwrap();
        let entries = cache.entries().unwrap();
        assert!(
            matches!(&entries[..], [CacheEntry::Temp { key: k, .. }] if *k == key),
            "{entries:?}"
        );
    }

    #[test]
    fn install_refuses_a_non_directory() {
        let dir = tempfile::tempdir().unwrap();
        let cache = LayerCache::new(dir.path().join("layers"));
        let key = LayerKey::from_bytes([3; 32]);
        let lock = cache.lock(&key).unwrap();
        std::fs::write(dir.path().join("file"), "x").unwrap();
        assert!(
            cache
                .install(&key, &dir.path().join("file"), &lock)
                .is_err()
        );
        assert!(
            cache
                .install(&key, &dir.path().join("missing"), &lock)
                .is_err()
        );
    }
}
