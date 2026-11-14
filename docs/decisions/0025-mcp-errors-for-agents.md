# 0025: Structured errors and agent-facing hints over MCP
Status: accepted

## Context

The first real agent session over `graft mcp` (`reports/graft-mcp-report.md`)
found that an agent could tell a call failed but not why without matching
on English text. Hints named CLI commands it must not use (`see \`graft
ls\``, `graft new <name>`, "restore it later with `graft rewind`", for
which there was no tool). `graft_list` and `graft_status` answered in
prose while `graft_plan` and `graft_radar` answered in JSON. A virtual
create whose layer build failed left the `graftd` it had started running,
and nothing over MCP can stop it.

The MCP server runs the `graft` binary for every call (ADR 0019), so the
CLI produces every message.

## Options considered

- A. **Classify in the server** by matching the CLI's stderr. No CLI change,
  but every wording change silently breaks a code.
- B. **The CLI reports codes itself when asked.** `graft mcp` sets
  `GRAFT_MCP=1`; errors that matter carry a typed code in their
  `anyhow` chain, and on failure the binary prints one more stderr line,
  `{"error":{"code","message","hint"}}`. Hints pick MCP tool names under
  `GRAFT_MCP=1`. The server returns that object (plus `output`, what else
  the command printed) as the tool error's text.
- C. Link the CLI's command functions into the server and return typed
  results. Removes the process boundary ADR 0019 chose for identical
  behaviour and safety checks; too large for a patch release.

## Decision

B. Codes: `invalid_arguments` and `confirm_required` (refused by the
server before anything runs), `invalid_name`, `not_found` (graft or
ring), `layer_build_failed`, `backend_unsupported`, and `failed` for the
rest (including a `graft_run` command that exits non-zero). The CLI's
human output without `GRAFT_MCP` is unchanged except that each coded
error's hint is appended to its message, as before.

Alongside:

- `graft ls --json` and `graft status --json`, used by `graft_list` and
  `graft_status` (empty lists are `[]`).
- A `graft_rewind` tool, so `graft_ring`'s checkpoints can be restored
  over MCP; its output names the tool to undo it with.
- Tool descriptions say which backends are virtual, that layers are built
  on create, and where `graft_integrate` lands by default.
- A virtual `graft new` that fails stops `graftd` again if it started it
  and nothing is mounted. A `graftd` that was already running, or serves
  other grafts, keeps running.

## Consequences

- Codes are part of the MCP interface: documented in `docs/mcp.md`, and
  changing or removing one is a breaking change. New codes can be added.
- Errors not yet given a code report `failed` with their message; more
  can be coded as agents need to tell them apart.
- Tests: `failures_have_codes_and_mcp_hints`, `rings_rewind_over_mcp`
  (tests/e2e/mcp.rs), `a_failed_create_stops_the_graftd_it_started`
  (tests/e2e/virtual_grafts.rs), `codes_come_from_the_chain` (unit), and
  the tool schema snapshot.
