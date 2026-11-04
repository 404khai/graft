-- Graft metadata schema, version 5: NFS-backed virtual grafts (macOS).
--
-- Never edit this file once released: add a new migration instead. See
-- docs/on-disk-format.md and ADR 0009.
--
-- SQLite cannot change a CHECK constraint in place, so grafts is rebuilt
-- (SQLite's documented procedure; migrations run with foreign keys off and
-- are checked before commit, so child rows are kept).

CREATE TABLE grafts_v5 (
    graft_id    TEXT    PRIMARY KEY CHECK (length(graft_id) = 32),
    name        TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 40),
    base_commit TEXT    NOT NULL CHECK (length(base_commit) IN (40, 64)),
    state       TEXT    NOT NULL
                CHECK (state IN ('created', 'live', 'integrated', 'pruned')),
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    backend     TEXT    NOT NULL DEFAULT 'reflink'
                CHECK (backend IN ('reflink', 'fuse', 'nfs'))
) STRICT;

INSERT INTO grafts_v5 (graft_id, name, base_commit, state, created_at, updated_at, backend)
    SELECT graft_id, name, base_commit, state, created_at, updated_at, backend FROM grafts;

DROP TABLE grafts;
ALTER TABLE grafts_v5 RENAME TO grafts;

CREATE UNIQUE INDEX grafts_name_unique ON grafts (name) WHERE state <> 'pruned';
