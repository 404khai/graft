# 0026: Structured results for every MCP tool
Status: accepted

## Context

The first complete agent session over `graft mcp`
(`reports/graft-mcp-report3.md`) ran every step with every safety check
holding. What it lacked was shape. `graft_run` reported no exit code, and
merged stdout with stderr. Policy refusals (prune would lose work,
integrate onto the checked-out branch) came back as the generic `failed`,
with CLI flag names in the text. A dry run said `checked` for a request
the real run then refused. `graft_new`, bud, ring, rewind, prune and
integrate answered in prose, so the agent had to regex paths out of them.
No result named the Root, which is how runs 1 and 2 used the wrong Root
without noticing. A layer build that failed in the MCP client's
environment did not say which `PATH` it ran with. `graft_plan` dropped
directories and files that do not exist yet without a word. This builds
on ADR 0025.

## Options considered

- **Exit codes for `graft_run`**
  - A. Decode `graft run`'s own exit status (it exits with the command's
    code, or 128 + signal). Ambiguous: a command that exits 130 looks the
    same as one killed by SIGINT.
  - B. Under `GRAFT_MCP=1`, `graft run` writes one more stderr line,
    `{"run":{"exit_code","signal","duration_ms"}}`. The server reads it and
    returns it with stdout and stderr kept apart. **Chosen.**
- **JSON results**
  - A. One global `--json` flag.
  - B. A `--json` flag per command that has a result. The server passes it
    for the tools that need it, as `ls`, `status`, `radar`, `plan` and
    `integrate` already do. **Chosen:** each command's JSON is described
    and tested with that command.
- **Directories in `graft_plan`**
  - A. Report them as unresolved.
  - B. Expand them into whole-file claims for every file under them, and
    report spans that name nothing yet (new files, unknown symbols) as
    `unresolved`. **Chosen:** a task that names a directory means its
    files, and claims stay file-level.

## Decision

- `graft_run` returns `{exit_code, signal, stdout, stderr, duration_ms,
  timed_out}`. A non-zero exit or a signal is a tool error with code
  `command_failed` and the same fields. The server's time limit gives
  `timed_out`.
- Refusals get their own codes:
  - `work_would_be_lost` (prune);
  - `checkout_would_fall_behind` and `root_not_clean` (integrate onto the
    checked-out branch);
  - per-request integrate codes: `conflict`, `gate_failed`,
    `not_approved`, `nothing_to_integrate`, `claimed`.

  Under MCP, hints name parameters (`force: true`, `update_checkout:
  true`, `onto`), not flags. The hint is reported once, not also inside
  `message`.
- The checked-out-branch rule runs before any work, for dry runs too, and
  again when landing (the target could change in between).
- `--json` on `new`, `bud`, `ring` (and `ring --list`), `rewind`, `prune`
  and `integrate`. `ls --json` and `status --json` become `{root, version,
  grafts}`. The server's instructions name the Root and the version.
- A failed layer build ends with `build environment: \`npm\` is
  /path/to/npm; PATH=…`.
- `graft plan` expands directory spans and bare paths into the files under
  them. Its JSON gains `unresolved`, and its text lists them before the
  plan.

## Consequences

- The JSON shapes and codes are part of the MCP interface
  (`docs/mcp.md`), pinned by the schema snapshot and the e2e tests.
  `ls --json`/`status --json` changed shape before any release shipped
  them.
- Human CLI output is unchanged except that `graft plan` prints a "Not
  placed" line when there is something to report.
- Tests: `refusals_have_codes_and_dry_runs_predict_them` and the extended
  `failures_have_codes_and_mcp_hints` and `rings_rewind_over_mcp`
  (tests/e2e/mcp.rs), `directories_expand_and_unknown_spans_are_reported`
  (graft-sched), `codes_come_from_the_chain` (hint not in the message), and
  the tool schema snapshot. The init check in every MCP test asserts that
  the instructions name the Root and the version.
