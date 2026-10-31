# On-disk format

**Format version: 3** (metadata schema v2)

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
└── roots/
    └── <root-id>/              one directory per Root, mode 0700
        ├── graft.db            SQLite metadata (+ graft.db-wal, graft.db-shm)
        ├── blobs/              (delta store phase) delta blobs
        ├── layers/             mode 0700, the layer cache (ADR 0004)
        │   ├── <layer-key>/    a complete layer (directory tree)
        │   ├── <layer-key>.lock  OS file lock held by builders and gc
        │   └── .tmp-<layer-key>/ a layer being installed
        └── mounts/             mode 0700
            ├── <name>/         a graft's working tree (see "Grafts" below)
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

## `config.toml`

TOML. Optional; a missing file means defaults. Unknown keys are an error.

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `log.filter` | string | `"warn"` | `tracing` filter directive. `$GRAFT_LOG` overrides it. |

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
| `kind` | TEXT | Operation kind (defined in the delta store phase) |
| `path` | BLOB | Repo-relative path |
| `path_to` | BLOB | Destination for renames, else `NULL` |
| `blob` | TEXT | Resulting `BlobHash`, if any |
| `actor` | TEXT | Optional caller-supplied actor or tool-call ID |

Primary key: `(graft_id, seq)`.

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

`graft prune` marks the row `pruned`, removes `mounts/<name>/` and any
`mounts/.tmp-<id>/`, deletes `refs/graft/<id>`, and finally deletes the row
(which deletes its `layer_refs`). Layers stay in the cache until `graft gc`,
which, holding each layer's lock, removes layers with no references and
leftover `.tmp-*` installs. Lock files are left in place.
A prune interrupted part-way is resumed by running `graft prune <name>`
again.
