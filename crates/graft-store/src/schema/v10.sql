-- Graft metadata schema, version 10: Scheduler claims.
--
-- Never edit this file once released: add a new migration instead. See
-- docs/on-disk-format.md and ADR 0018.

-- Soft claims: a graft's right to change a file, or one symbol in it.
CREATE TABLE claims (
    graft_id   TEXT    NOT NULL REFERENCES grafts (graft_id) ON DELETE CASCADE,
    path       TEXT    NOT NULL CHECK (length(path) BETWEEN 1 AND 4096),
    -- '' claims the whole file.
    symbol     TEXT    NOT NULL DEFAULT '',
    -- The plan (a hash of Root's HEAD and the task) and position that
    -- created the claim, if a plan did.
    plan       TEXT,
    plan_order INTEGER CHECK (plan_order IS NULL OR plan_order >= 0),
    PRIMARY KEY (graft_id, path, symbol)
) STRICT, WITHOUT ROWID;

CREATE INDEX claims_by_path ON claims (path);

-- Refused writes, so the writer can find out why.
CREATE TABLE admissions (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    graft_name TEXT    NOT NULL,
    path       TEXT    NOT NULL,
    verdict    TEXT    NOT NULL,
    at         INTEGER NOT NULL
) STRICT;
