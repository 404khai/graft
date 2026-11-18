# 0030: Diagnostics bundle and crash files
Status: accepted

## Context

A bug report about Graft usually needs the same facts: which version, which
platform and filesystem, which backend and whether reflinks, FUSE, NFS and
the sandbox work, the state of each Root's database, whether graftd is up
and what it last logged, and any panic. Today a maintainer has to ask for
each one, and a panic in graftd goes to `run/graftd.log` among everything
else, where the next start may bury it. Agents hit the same problems over
MCP and had no tool that tells them whether the machine or their call is at
fault.

## Options considered

1. **Remote telemetry and crash reporting** (a hosted service). Gives
   aggregate data, but sends data off the machine. AGENTS.md §3.8 makes that
   the owner's call, and it does not help with the one machine a report is
   about.
2. **A tarball of `$GRAFT_HOME/run/`, `crashes/` and the databases.** Complete,
   but needs an archive crate, ships whole databases (graft names, paths,
   op log rows) and is hard for a user to read before sharing.
3. **One JSON file** written by `graft doctor --report`: doctor's findings
   plus summaries (schema version, `integrity_check`, graft counts by state
   and backend), the last 200 graftd log lines and the newest 5 crash files,
   with secret-looking values redacted.

## Decision

Option 3, plus a panic hook in `graft` and `graftd` that writes each panic
(version, platform, message, location, backtrace) to
`$GRAFT_HOME/crashes/`, keeping the newest 20. The hook is best effort and
then runs the previous hook, so panics print as before.

- `graft doctor --json` prints doctor's findings as JSON;
  `graft doctor --report` also writes the bundle to
  `$GRAFT_HOME/reports/report-<ms>.json` (mode 0600), `--report-to FILE`
  elsewhere. Writing the bundle never starts graftd and never reads a
  graft's files.
- Redaction: `GRAFT_*` variables only, with `*KEY*`, `*TOKEN*`, `*SECRET*`
  and `*PASSWORD*` names redacted (AGENTS.md §11); in `config.toml`, the log
  tail and crash files, the value of any `NAME=value` or `NAME = value` with
  such a name is replaced. Paths are kept, since a report needs them, and
  doctor says to read the file before sharing it.
- MCP: `graft_doctor` runs `graft doctor --json [--report]`. Its description
  tells the agent to give the user the report's path, not its contents.
- No new dependencies.

## Consequences

- One command answers most of a maintainer's first questions; the issue
  template can ask for it.
- Redaction is pattern-based: a secret logged without a `NAME=` form (for
  example a bare token in an error message) is not caught. Graft does not
  log secrets (AGENTS.md §11), and the user reviews the file.
- Not covered here and planned as follow-ups: a request ID carried from the
  CLI through JSON-RPC into graftd's log and the op log; size-capped
  rotation of `run/graftd.log`; `graft fsck` (op log replay, blob and
  refcount checks); `graft stats` (graftd counters and latency histograms).
  Opt-in remote telemetry needs its own ADR and the owner's decision.
