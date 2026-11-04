-- Graft metadata schema, version 4: the delta store and op log.
--
-- Never edit this file once released: add a new migration instead. See
-- docs/on-disk-format.md and ADR 0008.

-- Op log columns for what the delta needs to be replayed exactly.
ALTER TABLE op_log ADD COLUMN mode INTEGER;
ALTER TABLE op_log ADD COLUMN mtime INTEGER;
ALTER TABLE op_log ADD COLUMN target BLOB;

-- Each graft's delta: what overrides its lower tree, one row per path.
CREATE TABLE delta (
    graft_id TEXT    NOT NULL REFERENCES grafts (graft_id) ON DELETE CASCADE,
    path     BLOB    NOT NULL,
    kind     TEXT    NOT NULL CHECK (kind IN ('file', 'symlink', 'dir', 'tombstone')),
    blob     TEXT    CHECK (blob IS NULL OR length(blob) = 64),
    lower    BLOB,
    target   BLOB,
    mode     INTEGER,
    mtime    INTEGER,
    opaque   INTEGER NOT NULL DEFAULT 0 CHECK (opaque IN (0, 1)),
    PRIMARY KEY (graft_id, path),
    CHECK (kind <> 'file' OR (blob IS NULL) <> (lower IS NULL))
) STRICT, WITHOUT ROWID;

CREATE INDEX delta_by_blob ON delta (blob) WHERE blob IS NOT NULL;

-- Files open for writing whose contents are not journaled yet. graftd
-- finalises them after a crash.
CREATE TABLE staged (
    graft_id TEXT    NOT NULL REFERENCES grafts (graft_id) ON DELETE CASCADE,
    id       INTEGER NOT NULL,
    path     BLOB    NOT NULL,
    mode     INTEGER NOT NULL,
    PRIMARY KEY (graft_id, id)
) STRICT, WITHOUT ROWID;
