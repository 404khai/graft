# 0019: MCP server
Status: accepted

## Context

AGENTS.md Phase 15: any MCP-capable agent can create, use and integrate
grafts without a person running commands. `graft-mcp`, using the official
Rust MCP SDK (`rmcp`), "talking to graftd over IPC", exposes `graft_new`,
`graft_list`, `graft_status`, `graft_diff`, `graft_run`, `graft_bud`,
`graft_ring`, `graft_radar`, `graft_integrate`, `graft_plan` and
`graft_prune`. Destructive calls need an explicit confirmation argument;
Radar events reach agents; tool schemas are documented and snapshot-tested;
an MCP client harness runs plan → new → run → radar → integrate → prune.
AGENTS.md §11: the server never exposes a tool that runs arbitrary
commands outside a graft.

## Options considered

**What the tools call**

- A. graftd over IPC, as the scope says. graftd only owns virtual mounts
  (ADR 0007). Reflink grafts (the default backend), layers, blueprints,
  Radar, Integrate and the Scheduler all live in the CLI; moving them into
  graftd is a rewrite of every phase since 2, for no gain to agents.
- B. The `graft` binary itself, run as a child process with arguments
  built from validated parameters (never a shell), in the Root's working
  tree. One implementation of every command, so the MCP tools behave
  exactly as the CLI does, including graftd use where the CLI uses it.
  graftd is still reached over IPC, by the CLI.

**Tool schemas**

- C. `#[tool]` macros with `schemars`-derived schemas. Less code, but the
  schemas change with the derive's version and are harder to review.
- D. Hand-written JSON Schemas, one function per tool, pinned by snapshot
  tests (`insta`).

**Radar events**

- E. A polling tool only. Agents must remember to poll.
- F. Logging notifications (`notifications/message`) for each change.
  The SDK marks MCP logging deprecated (SEP-2577), so it would not last.
- G. Resources: `graft://radar` and `graft://radar/<graft>` are Radar's
  JSON report. A client that subscribes (`resources/subscribe`) starts one
  `graft radar --watch --json` for the session, and is sent
  `notifications/resources/updated` for each subscribed URI whenever an
  overlap it covers appears or goes; it reads the resource to see what.

**Confirmation**

`graft_prune` with `force: true`, and `graft_integrate` unless
`dry_run: true`, refuse unless `confirm: true` is passed, with a tool
error saying what would happen. `graft_run` only runs commands through
`graft run`, so inside the graft's sandbox (ADR 0014).

## Decision

B + D + G, with the confirmations above.

- `graft mcp` serves the tools over stdio. It runs in the directory it was
  started in, which must be inside a registered Root.
- Every tool result is the command's output as text; a non-zero exit is a
  tool error with the command's stderr. `graft_radar` and `graft_plan`
  return JSON (they use `--json`).
- `graft_run` takes the command as an array (no shell), a working
  directory inside the graft is not configurable, and a timeout (default
  600 s) kills it.

## Consequences

- Each tool call starts a `graft` process: tens of milliseconds, small
  next to what agents do with it.
- The MCP surface can never drift from the CLI's behaviour or safety
  checks (trust, claims, prune safety, sandbox).
- New dependencies: `rmcp` (the official SDK) and `insta` (tests).
