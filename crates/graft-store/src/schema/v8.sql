-- Graft metadata schema, version 8: Radar.
--
-- Never edit this file once released: add a new migration instead. See
-- docs/on-disk-format.md and ADR 0016.

-- Results of Radar's speculative test runs, by a fingerprint of the
-- changes tested and the command, so an unchanged check is not re-run.
CREATE TABLE radar_checks (
    fingerprint TEXT    PRIMARY KEY CHECK (length(fingerprint) = 64),
    outcome     TEXT    NOT NULL
                CHECK (outcome IN ('pass', 'fail', 'conflict', 'timeout', 'error')),
    output      TEXT    NOT NULL,
    checked_at  INTEGER NOT NULL
) STRICT;
