-- Graft metadata schema, version 9: the Integrate queue.
--
-- Never edit this file once released: add a new migration instead. See
-- docs/on-disk-format.md and ADR 0017.

-- One row per `graft integrate` request, in queue order. Rows outlive
-- their graft (no foreign key), so `graft queue` keeps the history.
CREATE TABLE integrations (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    graft_id    TEXT    NOT NULL CHECK (length(graft_id) = 32),
    graft_name  TEXT    NOT NULL CHECK (length(graft_name) BETWEEN 1 AND 40),
    target      TEXT    NOT NULL CHECK (length(target) BETWEEN 1 AND 255),
    message     TEXT,
    dry_run     INTEGER NOT NULL DEFAULT 0 CHECK (dry_run IN (0, 1)),
    state       TEXT    NOT NULL
                CHECK (state IN ('queued', 'running', 'conflict', 'failed', 'landed', 'checked')),
    commit_id   TEXT    CHECK (commit_id IS NULL OR length(commit_id) IN (40, 64)),
    output      TEXT    NOT NULL DEFAULT '',
    enqueued_at INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
) STRICT;

CREATE INDEX integrations_by_state ON integrations (state, id);
