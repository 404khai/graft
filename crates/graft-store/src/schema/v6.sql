-- Graft metadata schema, version 6: Bud and Ring.
--
-- Never edit this file once released: add a new migration instead. See
-- docs/on-disk-format.md and ADR 0013.

-- The graft a bud was forked from. Not a foreign key: the source may be
-- pruned while the bud lives.
ALTER TABLE grafts ADD COLUMN budded_from TEXT CHECK (budded_from IS NULL OR length(budded_from) = 32);

-- For `rewind` ops: the sequence number whose state the graft returned to.
ALTER TABLE op_log ADD COLUMN to_seq INTEGER CHECK (to_seq IS NULL OR to_seq >= 0);

-- Named checkpoints of a graft's delta.
CREATE TABLE rings (
    graft_id   TEXT    NOT NULL REFERENCES grafts (graft_id) ON DELETE CASCADE,
    label      TEXT    NOT NULL CHECK (length(label) BETWEEN 1 AND 40),
    seq        INTEGER NOT NULL CHECK (seq >= 0),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (graft_id, label)
) STRICT, WITHOUT ROWID;

-- Each ring's copy of the delta, with the same columns as `delta`.
CREATE TABLE ring_delta (
    graft_id TEXT    NOT NULL,
    label    TEXT    NOT NULL,
    path     BLOB    NOT NULL,
    kind     TEXT    NOT NULL CHECK (kind IN ('file', 'symlink', 'dir', 'tombstone')),
    blob     TEXT    CHECK (blob IS NULL OR length(blob) = 64),
    lower    BLOB,
    target   BLOB,
    mode     INTEGER,
    mtime    INTEGER,
    opaque   INTEGER NOT NULL DEFAULT 0 CHECK (opaque IN (0, 1)),
    PRIMARY KEY (graft_id, label, path),
    FOREIGN KEY (graft_id, label) REFERENCES rings (graft_id, label) ON DELETE CASCADE,
    CHECK (kind <> 'file' OR (blob IS NULL) <> (lower IS NULL))
) STRICT, WITHOUT ROWID;

CREATE INDEX ring_delta_by_blob ON ring_delta (blob) WHERE blob IS NOT NULL;
