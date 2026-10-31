-- Graft metadata schema, version 1.
--
-- Never edit this file once released: add a new migration instead. See
-- docs/on-disk-format.md for the meaning of every column.

-- The Root this database belongs to. Exactly one row (one database per Root).
CREATE TABLE roots (
    singleton     INTEGER PRIMARY KEY CHECK (singleton = 1),
    root_id       TEXT    NOT NULL CHECK (length(root_id) = 32),
    path          BLOB    NOT NULL,
    first_commit  TEXT    NOT NULL CHECK (length(first_commit) IN (40, 64)),
    registered_at INTEGER NOT NULL
) STRICT;

CREATE TABLE grafts (
    graft_id    TEXT    PRIMARY KEY CHECK (length(graft_id) = 32),
    name        TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 40),
    base_commit TEXT    NOT NULL CHECK (length(base_commit) IN (40, 64)),
    state       TEXT    NOT NULL
                CHECK (state IN ('created', 'live', 'integrated', 'pruned')),
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
) STRICT;

-- Names are unique among grafts that have not been pruned.
CREATE UNIQUE INDEX grafts_name_unique ON grafts (name) WHERE state <> 'pruned';

-- Append-only journal of every write in a graft. Created now, written from
-- the delta store phase on.
CREATE TABLE op_log (
    graft_id TEXT    NOT NULL REFERENCES grafts (graft_id) ON DELETE CASCADE,
    seq      INTEGER NOT NULL CHECK (seq >= 1),
    at       INTEGER NOT NULL,
    kind     TEXT    NOT NULL,
    path     BLOB    NOT NULL,
    path_to  BLOB,
    blob     TEXT    CHECK (blob IS NULL OR length(blob) = 64),
    actor    TEXT,
    PRIMARY KEY (graft_id, seq)
) STRICT, WITHOUT ROWID;
