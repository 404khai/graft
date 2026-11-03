-- Graft metadata schema, version 3: per-graft filesystem backend.
--
-- Never edit this file once released: add a new migration instead. See
-- docs/on-disk-format.md and ADR 0006.

-- Which backend materialises the graft. Grafts created before this
-- migration are reflink grafts.
ALTER TABLE grafts ADD COLUMN backend TEXT NOT NULL DEFAULT 'reflink'
    CHECK (backend IN ('reflink', 'fuse'));
