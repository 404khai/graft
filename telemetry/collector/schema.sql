-- Daily counts only: one row per distinct combination of fields per day.
-- No raw events, no IP addresses, no identifiers.
CREATE TABLE IF NOT EXISTS daily_counts (
  day      TEXT    NOT NULL,  -- YYYY-MM-DD, as the client reported it
  command  TEXT    NOT NULL,
  ok       INTEGER NOT NULL,  -- 1 or 0
  code     TEXT    NOT NULL,  -- error code, '' when ok
  duration TEXT    NOT NULL,  -- one of five buckets
  version  TEXT    NOT NULL,
  os       TEXT    NOT NULL,
  arch     TEXT    NOT NULL,
  via      TEXT    NOT NULL,  -- cli or mcp
  count    INTEGER NOT NULL,
  PRIMARY KEY (day, command, ok, code, duration, version, os, arch, via)
);
