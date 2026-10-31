-- Graft metadata schema, version 2: the layer cache and build trust.
--
-- Never edit this file once released: add a new migration instead. See
-- docs/on-disk-format.md and ADR 0004.

-- Layers present in the layer cache ($GRAFT_HOME/roots/<id>/layers/<key>/).
CREATE TABLE layers (
    layer_key TEXT    PRIMARY KEY CHECK (length(layer_key) = 64),
    path      BLOB    NOT NULL,
    build     TEXT    NOT NULL,
    built_at  INTEGER NOT NULL
) STRICT;

-- Which grafts use which layers. Pruning a graft deletes its rows; a layer
-- with rows here cannot be deleted (no cascade on layer_key).
CREATE TABLE layer_refs (
    graft_id  TEXT NOT NULL REFERENCES grafts (graft_id) ON DELETE CASCADE,
    layer_key TEXT NOT NULL REFERENCES layers (layer_key),
    PRIMARY KEY (graft_id, layer_key)
) STRICT, WITHOUT ROWID;

CREATE INDEX layer_refs_by_layer ON layer_refs (layer_key);

-- Layer commands the user approved with `graft trust`.
CREATE TABLE trusted_commands (
    command_hash TEXT    PRIMARY KEY CHECK (length(command_hash) = 64),
    description  TEXT    NOT NULL,
    trusted_at   INTEGER NOT NULL
) STRICT;
