# On-disk format

**Format version: 1** (metadata schema v1)

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
        ├── layers/             (layer cache phase) shared layers
        └── mounts/<name>/      (reflink grafts phase) graft mount points
```

Entries in `roots/` whose name is not a valid Root ID, or that contain no
`graft.db`, are ignored.

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
