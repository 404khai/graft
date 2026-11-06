# On-disk format

**Format version: 7** (metadata schema v6)

This file describes everything Graft persists. Any change to a structure
below bumps the format version, adds a migration, and updates this file in
the same PR (AGENTS.md §5).

## `$GRAFT_HOME`

Resolved in this order:

1. `$GRAFT_HOME`, if set and non-empty. It must be an absolute path.
2. Linux and other Unix: `$XDG_DATA_HOME/graft` if `$XDG_DATA_HOME` is an
   absolute path, else `$HOME/.local/share/graft`.
3. macOS: `$HOME/Library/Application Support/graft`.

Graft creates directories under `$GRAFT_HOME` with mode `0700`. Resolving the
path never creates anything; only commands that write state do.

```
$GRAFT_HOME/
├── config.toml                 optional user config
├── run/                        mode 0700, graftd's runtime files (ADR 0007)
│   ├── graftd.sock             JSON-RPC socket, mode 0600 (see `run/` below)
│   ├── graftd.lock             OS file lock held by the running graftd
│   └── graftd.log              graftd's stderr when started by the CLI
└── roots/
    └── <root-id>/              one directory per Root, mode 0700
        ├── graft.db            SQLite metadata (+ graft.db-wal, graft.db-shm)
        ├── blobs/              mode 0700, delta blobs (ADR 0008)
        │   ├── <aa>/<rest>     a blob, named by its blake3 hash, mode 0444
        │   └── tmp/            blobs being written; renamed into place
        ├── grafts/             per virtual graft, mode 0700
        │   └── <graft-id>/
        │       ├── work/<n>    staging files of files open for writing
        │       └── git/        the graft's .git, created on first use
        ├── layers/             mode 0700, the layer cache (ADR 0004)
        │   ├── <layer-key>/    a complete layer (directory tree)
        │   ├── <layer-key>.lock  OS file lock held by builders and gc
        │   └── .tmp-<layer-key>/ a layer being installed
        └── mounts/             mode 0700
            ├── <name>/         a reflink graft's working tree, or a FUSE
            │                   graft's mount point (see "Grafts" below)
            └── .tmp-<graft-id>/  a graft being built; renamed to <name>/
```

Entries in `roots/` whose name is not a valid Root ID, or that contain no
`graft.db`, are ignored.

## Format history

| Version | Change | Migration |
| --- | --- | --- |
| 1 | `$GRAFT_HOME` layout, IDs, `graft.db` schema v1 | — |
| 2 | Adds `mounts/`, graft repositories and `refs/graft/<graft-id>` pins in Root | None: additive; v1 never created these |
| 3 | Adds `layers/` and schema v2 (`layers`, `layer_refs`, `trusted_commands`) | Schema v1 → v2 runs automatically on open; tables only added |
| 4 | Adds `run/` (graftd) and schema v3 (`grafts.backend`) | Schema v2 → v3 runs automatically on open; existing grafts become `reflink` |
| 5 | Adds `blobs/`, `grafts/` and schema v4 (`delta`, `staged`, `op_log.mode/mtime/target`) | Schema v3 → v4 runs automatically on open; tables and nullable columns only added |
| 6 | Schema v5: `grafts.backend` accepts `nfs`; the socket moves to `/tmp/graft-<uid>/` when `run/graftd.sock` would be too long | Schema v4 → v5 rebuilds `grafts` (foreign keys off, checked before commit); every row and child row is kept |
| 7 | Schema v6: Bud and Ring (`grafts.budded_from`, `op_log.to_seq` and the `rewind` op, tables `rings` and `ring_delta`; ADR 0013) | Schema v5 → v6 runs automatically on open; tables and nullable columns only added |

## `config.toml`

TOML. Optional; a missing file means defaults. Unknown keys are an error.

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `log.filter` | string | `"warn"` | `tracing` filter directive. `$GRAFT_LOG` overrides it. |
| `fs.backend` | string | `"reflink"` | Backend for new grafts: `reflink`, `fuse` (Linux) or `nfs` (macOS). `$GRAFT_FS` and `graft new --backend` override it. |

## Identifiers

All IDs are stored and printed as lowercase hex. Parsers reject uppercase
and any other length.

| Type | Bytes | Text | How it is made |
| --- | --- | --- | --- |
| `RootId` | 16 | 32 hex chars | Derived; see below |
| `GraftId` | 16 | 32 hex chars | 16 random bytes from the OS (`getrandom`) |
| `BlobHash` | 32 | 64 hex chars | `blake3` of the blob contents |
| `CommitId` | 20 or 32 | 40 or 64 hex chars | git object ID (SHA-1 or SHA-256) |

### Root ID derivation

```
root_id = blake3::derive_key("graft root-id v1",
                             canonical_path_bytes || 0x00 || first_commit_hex)[0..16]
```

- `canonical_path_bytes`: the canonicalised path of the working tree's top
  level, as raw OS bytes (non-UTF-8 paths are used as-is).
- `first_commit_hex`: the commit reached by following first parents from
  HEAD until a commit has none (in a shallow clone, the shallow boundary).

The Root ID is computed once, at `graft init`. Afterwards a repository's
Root is found by path, not by recomputing the ID; see
[ADR 0002](decisions/0002-root-identity-and-lookup.md).

### Graft names

1 to 40 characters of `[a-z0-9-]`, starting with a letter or digit and not
ending with a hyphen. Unique among a Root's grafts that are not pruned.

## Timestamps

`INTEGER` milliseconds since the Unix epoch, UTC. Printed as RFC 3339
(`2026-10-31T10:32:58.123Z`).

## Paths

Paths are stored as `BLOB`s of raw OS bytes, never as text, so every path
round-trips exactly.

## `graft.db`

SQLite 3, one database per Root.

- Journal mode `WAL`; `synchronous = NORMAL` (a crash can lose the most
  recent commits but never corrupts the database); `foreign_keys = ON`.
- Writers wait up to 30 s for a lock (`busy_timeout`).
- All tables are `STRICT`.
- The schema version is `PRAGMA user_version`.

### Schema versions and migrations

Migration `n` takes the schema from version `n - 1` to `n`. Migrations are
SQL files in `crates/graft-store/src/schema/`, applied in order inside one
`BEGIN IMMEDIATE` transaction when a database is opened, so concurrent
openers serialise and a failed migration leaves the database untouched. A
database with a version newer than the running Graft supports is refused,
never downgraded. Released migration files are hash-pinned by a test and
must never be edited.

| Version | Migration | Changes |
| --- | --- | --- |
| 1 | `v1.sql` | `roots`, `grafts`, `op_log` |
| 2 | `v2.sql` | `layers`, `layer_refs`, `trusted_commands` |

### `roots`

The Root this database belongs to. Exactly one row.

| Column | Type | Notes |
| --- | --- | --- |
| `singleton` | INTEGER PK | Always `1` (`CHECK`) |
| `root_id` | TEXT | `RootId`; equals the directory name |
| `path` | BLOB | Canonical working-tree path at registration |
| `first_commit` | TEXT | `CommitId` used to derive `root_id` |
| `registered_at` | INTEGER | Timestamp |

### `grafts`

| Column | Type | Notes |
| --- | --- | --- |
| `graft_id` | TEXT PK | `GraftId` |
| `name` | TEXT | Graft name |
| `base_commit` | TEXT | `CommitId` the graft was created from |
| `state` | TEXT | `created`, `live`, `integrated` or `pruned` |
| `created_at` | INTEGER | Timestamp |
| `updated_at` | INTEGER | Timestamp of the last change to the row |
| `backend` | TEXT | (v3) `reflink` or `fuse`, (v5) or `nfs`; default `reflink` (ADRs 0006, 0009) |
| `budded_from` | TEXT | (v6) `GraftId` of the graft this one was budded from, or `NULL`. Not a foreign key: the source may be pruned first |

Index `grafts_name_unique`: unique `name` where `state <> 'pruned'`.

State changes: any state may move to any other state except itself, and
`pruned` is terminal.

### `op_log`

Created in v1 and written from the delta store phase on. `WITHOUT ROWID`.

| Column | Type | Notes |
| --- | --- | --- |
| `graft_id` | TEXT | References `grafts`, `ON DELETE CASCADE` |
| `seq` | INTEGER | Per-graft sequence number, from 1 |
| `at` | INTEGER | Timestamp |
| `kind` | TEXT | `create`, `write`, `truncate`, `mkdir`, `symlink`, `unlink`, `rmdir`, `rename`, `chmod`, `touch` or (v6) `rewind` |
| `path` | BLOB | Repo-relative path |
| `path_to` | BLOB | Destination for renames, else `NULL` |
| `blob` | TEXT | Resulting `BlobHash`, if any |
| `actor` | TEXT | Optional caller-supplied actor or tool-call ID |
| `mode` | INTEGER | (v4) Permission bits, for `create`/`write`/`truncate`/`mkdir`/`chmod` |
| `mtime` | INTEGER | (v4) Explicit modification time in ms, for `touch` |
| `target` | BLOB | (v4) Symlink target, for `symlink` |
| `to_seq` | INTEGER | (v6) For `rewind`: the sequence number whose state the graft returned to (0: the empty delta). `path` is empty |

`path` is the source of a `rename` and `path_to` its destination; `blob`
is the file's new contents for `create`, `write` and `truncate`. A
graft's op log replayed from empty through `Delta::replay` reproduces its
`delta` rows exactly (ADR 0008); a `rewind` restores the state replay had
right after op `to_seq` (ADR 0013). A bud starts with a copy of its
source's op log, sequence numbers included.

### `delta` (v4)

`WITHOUT ROWID`. One row per path that the graft overrides; paths are
graft-relative bytes, `/`-separated.

| Column | Type | Notes |
| --- | --- | --- |
| `graft_id` | TEXT | References `grafts`, `ON DELETE CASCADE` |
| `path` | BLOB | Graft-relative path |
| `kind` | TEXT | `file`, `symlink`, `dir` or `tombstone` |
| `blob` | TEXT | A file's contents as a `BlobHash`, or `NULL` |
| `lower` | BLOB | A file whose contents are the lower tree's at this path (after a rename or mode change), or `NULL`. A file has exactly one of `blob` and `lower` |
| `target` | BLOB | A symlink's target |
| `mode` | INTEGER | Permission bits of a file or directory |
| `mtime` | INTEGER | Modification time in ms, or `NULL` for the base commit's time |
| `opaque` | INTEGER | 1 if a directory hides everything the lower tree has below it |

Primary key `(graft_id, path)`. Index `delta_by_blob` on `blob`. A
`tombstone` hides the path and everything the lower tree has below it. The
lower tree is the base commit's tree with the graft's layers overlaid.

### `staged` (v4)

`WITHOUT ROWID`. Files open for writing whose contents are in
`grafts/<graft-id>/work/<id>` and not yet journaled. graftd journals them
on the next mount after a crash.

| Column | Type | Notes |
| --- | --- | --- |
| `graft_id` | TEXT | References `grafts`, `ON DELETE CASCADE` |
| `id` | INTEGER | Staging file number |
| `path` | BLOB | The file's current path in the graft |
| `mode` | INTEGER | Its permission bits |

Primary key: `(graft_id, seq)`.

### `rings` (v6)

`WITHOUT ROWID`. Named checkpoints of a graft's delta (ADR 0013).

| Column | Type | Notes |
| --- | --- | --- |
| `graft_id` | TEXT | References `grafts`, `ON DELETE CASCADE` |
| `label` | TEXT | `RingLabel` (graft name rules); `undo-<seq>` is taken by `graft rewind` |
| `seq` | INTEGER | Sequence number of the graft's last op when the ring was taken, 0 if none |
| `created_at` | INTEGER | Timestamp |

Primary key `(graft_id, label)`.

### `ring_delta` (v6)

`WITHOUT ROWID`. Each ring's copy of the graft's `delta` rows, with the
same columns as `delta` plus `label`. References `rings (graft_id, label)`,
`ON DELETE CASCADE`. Primary key `(graft_id, label, path)`; index
`ring_delta_by_blob` on `blob`.

### Blob references

A blob in `blobs/` is kept while any `delta` or `ring_delta` row names it
(mark and sweep, no counters). Bud and ring add rows, so they add
references; rewind and prune remove rows. The sweep runs on prune and
`graft gc`, under graftd's write locks or `graftd.lock`.

### `layers` (v2)

| Column | Type | Notes |
| --- | --- | --- |
| `layer_key` | TEXT PK | `LayerKey`; equals the directory name in `layers/` |
| `path` | BLOB | The layer's path in a graft |
| `build` | TEXT | The command that built it |
| `built_at` | INTEGER | Timestamp |

### `layer_refs` (v2)

`WITHOUT ROWID`. Primary key `(graft_id, layer_key)`; index on `layer_key`.

| Column | Type | Notes |
| --- | --- | --- |
| `graft_id` | TEXT | References `grafts`, `ON DELETE CASCADE` (pruning releases) |
| `layer_key` | TEXT | References `layers`, no cascade (a referenced layer cannot be deleted) |

### `trusted_commands` (v2)

| Column | Type | Notes |
| --- | --- | --- |
| `command_hash` | TEXT PK | `CommandHash` of an approved layer's commands |
| `description` | TEXT | What was approved, e.g. ``node_modules: build `npm ci` `` |
| `trusted_at` | INTEGER | Timestamp |

## Layer keys and command hashes

Both are 32-byte `blake3::derive_key` outputs, printed as 64 hex characters.
Every field is prefixed with its length as a little-endian `u64`.

```
layer_key    = derive_key("graft layer v1",
                 path, build, toolchain-or-"", toolchain-output,
                 OS, ARCH, count, (input-path, input-contents)*)
command_hash = derive_key("graft trust v1", path, build, toolchain-or-"")
```

Inputs are read from the graft's base commit, not from any working tree.



Each graft is a standalone git repository at `mounts/<name>/`
([ADR 0003](decisions/0003-reflink-graft-layout.md)):

| Path | Contents |
| --- | --- |
| `.git/HEAD` | `ref: refs/heads/graft/<name>` |
| `.git/refs/heads/graft/<name>` | The base commit, until the graft commits |
| `.git/objects/info/alternates` | Canonical path of Root's `objects/` directory |
| `.git/config` | `[include] path = <Root's .git/config>`, then `core.bare = false`, `core.logallrefupdates = true`, and a `root` remote whose URL is Root's working tree |
| `.git/shallow` | Copy of Root's, if Root is shallow |
| `.git/info/exclude` | Copy of Root's, if present, followed by an anchored, escaped line per layer path (`/node_modules/`) |
| `.git/index` | Index of the base tree, with real stat data |

Objects the graft creates (its own commits) live in the graft's own
`.git/objects/`. Roots that set `core.worktree` are refused.

A graft is built in `mounts/.tmp-<graft-id>/` and renamed into place when
complete, so `mounts/<name>/` is never half-built. A leftover `.tmp-*`
directory is a failed or interrupted `graft new`, and prune removes it.

### Virtual grafts: FUSE (v4, writable from v5) and NFS (v6)

A graft with `backend = 'fuse'` (Linux) or `'nfs'` (macOS, served over
NFSv3 loopback, ADR 0009) stores no working tree. `mounts/<name>/`
is an empty directory (mode `0700`) used as the mount point; `graftd`
mounts the graft's delta over its lower tree there (ADRs 0006 and 0008).
Changed file contents are blobs in `blobs/`; `.git` inside the mount is
served from `grafts/<graft-id>/git/`, which has the same layout as a
reflink graft's `.git` plus `core.checkStat = minimal` and
`core.trustctime = false`, and is created on first use. A graft that
should be mounted is one with a virtual backend in state `live`; `graftd`
remounts every such graft of its platform's kind when it starts.

Blobs are shared by content across a Root's grafts. A blob no `delta` row
references is deleted by `graft gc` or `graft prune`, under graftd's write
locks (or holding `run/graftd.lock` when no daemon runs).

## `run/` (v4)

Created by `graftd` with mode `0700`; the daemon refuses to start if it is
owned by another user or accessible by group or others. `graftd.sock` is
created with mode `0600`. `graftd.lock` is held with an exclusive OS file
lock for the daemon's lifetime; a socket whose lock is free is stale and is
replaced. The protocol is in ADR 0007.

Unix socket paths are limited to about 100 bytes. When
`run/graftd.sock` would be longer (a deep `$GRAFT_HOME`), the socket is
`/tmp/graft-<uid>/<first 16 hex chars of blake3($GRAFT_HOME)>.sock`
instead (format v6). `graftd` creates `/tmp/graft-<uid>` with mode `0700`
and refuses it if it is a symlink, is owned by anyone other than
`$GRAFT_HOME`'s owner, or is accessible by group or others.

## Refs in Root

| Ref | Points at | Lifetime |
| --- | --- | --- |
| `refs/graft/<graft-id>` | The graft's base commit | `graft new` to `graft prune`; keeps the base reachable so `git gc` cannot prune it |

## Graft lifecycle on disk

`graft new` writes in this order, undoing completed steps on failure:

1. `grafts` row in state `created` (reserves the name)
2. `mounts/.tmp-<id>/`, renamed to `mounts/<name>/`
3. `refs/graft/<id>` in Root
4. for each layer, holding `layers/<key>.lock`: build it into the cache if
   missing, insert `layers` and `layer_refs` rows, reflink it into the graft
5. state `live`

For a FUSE graft, step 2 creates the empty `mounts/<name>/`; step 4 builds
missing layers in a temporary reflink graft `mounts/.tmp-<id>/` that is
removed afterwards, and records the references without copying anything;
then `graftd` mounts the graft before step 5. Prune refuses if the delta has
changes against the base or `.git` has commits Root lacks, unless
`--force`. It asks `graftd` to unmount the graft (lazily if busy), which
then removes the empty mount point with `rmdir` (never recursively),
`grafts/<graft-id>/`, the graft's `delta` and `staged` rows, and
unreferenced blobs; the CLI then deletes the pin ref and the record.

`graft prune` marks the row `pruned`, removes `mounts/<name>/` and any
`mounts/.tmp-<id>/`, deletes `refs/graft/<id>`, and finally deletes the row
(which deletes its `layer_refs`). Layers stay in the cache until `graft gc`,
which, holding each layer's lock, removes layers with no references and
leftover `.tmp-*` installs. Lock files are left in place.
A prune interrupted part-way is resumed by running `graft prune <name>`
again.

`graft bud <src> <dst>` (virtual grafts only) asks graftd to journal the
source's pending writes, then in one transaction inserts the bud's `grafts`
row (state `created`, the source's base and backend) with copies of the
source's `delta`, `op_log` and `layer_refs` rows; then it pins
`refs/graft/<id>`, graftd mounts the bud, and its state becomes `live`. A
failure undoes every step. The bud's `.git` is created fresh on first use.

`graft ring` inserts a `rings` row and its `ring_delta` rows in one
transaction. `graft rewind` unmounts the graft (it refuses if the unmount
had to be lazy, or files are still staged), then in one transaction rings
the current state as `undo-<seq>`, appends a `rewind` op and replaces the
graft's `delta` rows with the ring's; graftd then remounts it.
