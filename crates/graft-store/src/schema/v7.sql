-- Graft metadata schema, version 7: Blueprints.
--
-- Never edit this file once released: add a new migration instead. See
-- docs/on-disk-format.md and ADR 0015.

-- The blueprint a graft was created with, if any.
ALTER TABLE grafts ADD COLUMN blueprint TEXT CHECK (blueprint IS NULL OR length(blueprint) BETWEEN 1 AND 40);

-- What a graft's blueprint provisioned, so prune can release it. A row is
-- written before the resource is created.
CREATE TABLE graft_resources (
    graft_id TEXT NOT NULL REFERENCES grafts (graft_id) ON DELETE CASCADE,
    kind     TEXT NOT NULL
             CHECK (kind IN ('port', 'postgres-database', 'docker-volume', 'docker-network')),
    name     TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 40),
    value    TEXT NOT NULL CHECK (length(value) BETWEEN 1 AND 128),
    PRIMARY KEY (graft_id, kind, name)
) STRICT, WITHOUT ROWID;

-- A port, database or Docker name belongs to one graft of the Root.
CREATE UNIQUE INDEX graft_resources_value_unique ON graft_resources (kind, value);
