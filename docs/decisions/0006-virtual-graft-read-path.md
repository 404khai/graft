# 0006: Virtual graft read path (FUSE on Linux)
Status: accepted

## Context

Reflink grafts (ADR 0003) cost one stat and one clone per tracked file, so
creating one grows with the number of files. The virtual filesystem phase
asks that `graft new` take under a second regardless of repo size, by
serving files on demand from Root's object store. This phase is read-only:
writes return `EROFS`; the delta store comes next.

Decisions needed:

1. Which FUSE library.
2. How the tree is modelled and served (inodes, caches, content).
3. How git filters (CRLF, `.gitattributes`) are honoured, since a checkout
   applies them and "identical results to Root" depends on them.
4. How layers appear in a virtual graft.
5. Whether a virtual graft has a `.git`.
6. How grafts choose a backend, and how that is recorded.

## Options considered

**FUSE library**

- A. `fuser` (pure Rust, MIT, actively maintained, the crate AGENTS.md
  names). Default features mount through `fusermount3` without linking
  libfuse, so no C build dependency.
- B. `fuse3` (async, tokio). Smaller user base; async buys nothing for a
  CPU-and-object-store-bound read path.
- C. libfuse via bindgen. Adds a C toolchain dependency for no benefit.

**Tree model**

- D. Materialise the whole tree into an inode table at mount. O(files) at
  create time: exactly what this phase removes.
- E. Lazy inode table: the root inode is the base commit's tree; a
  directory's entries are read from its git tree object the first time it
  is looked up or listed, and child inodes are allocated then. Blob sizes
  come from object headers; contents are read on first `read` and kept in a
  byte-bounded cache.

**Filters**

- F. Serve raw blobs. Wrong for repos using `eol`, `text`, or LFS-style
  smudge filters; `cat` would differ from Root.
- G. Run every file through the filter pipeline. Correct, but sizes then
  need the converted content, so `ls -l` reads every blob.
- H. Run the pipeline only where attributes can apply: if Root's config sets
  `core.autocrlf`, `core.eol` or `core.attributesFile`, if
  `info/attributes` exists, or if the file or one of its ancestor
  directories in the base tree contains a `.gitattributes`. Elsewhere,
  serve the raw blob with its header size. Attributes are taken from the
  base tree, not Root's working tree, as a checkout of the base would.

**Layers**

- I. Reflink layer contents into a backing directory per graft. O(layer
  size) per graft again.
- J. Pass through: a layer's path in the graft is served directly from the
  layer cache directory, read-only. Lookups and reads go to the real files
  with `symlink_metadata` and `pread`; symlinks inside a layer are returned,
  never followed.

**`.git`**

- K. Synthesise a git directory with an index. Building an index is
  O(files), and without a write path `git status` has nothing to report.
- L. No `.git` in this phase. `graft status/diff` for a virtual graft are
  defined by its (empty) delta. `git` inside a virtual graft arrives with
  the delta store, which owns `git status` parity.

**Backend selection**

- M. Global config only. Cannot mix backends within a Root.
- N. Per graft: `graft new --backend`, else `$GRAFT_FS`, else
  `fs.backend` in `config.toml`, else `reflink`. Recorded in a new
  `grafts.backend` column so every later command (and `graftd` on restart)
  knows how to treat the graft.

## Decision

A + E + H + J + L + N.

- `fuser` 0.18 with default features off, as a Linux-only dependency of
  `graft-fs`. The FUSE adapter (`fuse` module) is a thin translation layer;
  the tree model (`view` module, `GraftView`) is platform-independent and
  unit-tested without mounting, on macOS too.
- Inodes are allocated on first lookup and never reused while the graft is
  mounted (FUSE `forget` is accepted but does not free them). Memory grows
  with the number of distinct paths visited, not the repo size. Inode 1 is
  the root. Attributes and entries are immutable for a read-only graft, so
  the kernel gets long TTLs (60 s).
- Git objects are read through `gix` from Root's object store, opened once
  per mount as a thread-safe handle. Filter conversion runs on one worker
  thread that owns the pipeline; it is only started for grafts where H says
  attributes can apply.
- Ownership is the daemon's uid and gid. Files are `0644` or `0755`
  (executable bit from git), directories `0755`, symlinks `0777`.
  Timestamps are the base commit's committer time. Submodules appear as
  empty directories, as `git clone` leaves them.
- Mounted with `ro`, `nosuid`, `nodev`, `default_permissions`, and
  `fsname=graft`, `subtype=graft`. Every mutating callback returns `EROFS`.
  No callback can panic across the FUSE boundary: errors map to `errno`
  (`ENOENT`, `ENOTDIR`, `EIO` with a logged cause).
- Prune unmounts. If the mount is busy (`EBUSY`), it falls back to a lazy
  unmount (`fusermount3 -u -z`) and reports a warning: processes holding
  files keep reading until they close them, and the mount point is removed.
  A stale mount left by a crashed daemon (`ENOTCONN`) is lazily unmounted
  before remounting.
- Schema v3 adds `grafts.backend` (`reflink` | `fuse`, default `reflink`
  for existing rows). Format version 4.

## Consequences

- `graft new --backend fuse` is O(1) in repo size: a row, a pin ref, and a
  mount. The first `ls -R` or `rg` pays for reading trees and blobs.
- Virtual grafts are Linux-only until the macOS backend phase. On macOS,
  `--backend fuse` fails with a clear error.
- Without a `.git`, tools that need a repository (`git log`, some build
  scripts) do not work in a virtual graft yet. `cargo check` and `tsc`
  work when their output goes outside the read-only tree
  (`CARGO_TARGET_DIR`, `--noEmit`).
- Layer builds still need a real working tree. If a virtual graft needs a
  layer that is not cached, it is built in a temporary reflink graft of the
  same base, installed into the cache, and the temporary graft is removed.
- Repos that use attributes pay one O(files) index build per mount, when
  the first filtered file is read.
