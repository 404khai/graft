# 0031: Request IDs and graftd log rotation
Status: accepted

## Context

ADR 0030 left two gaps in investigating a bug. A failing `graft` command
and what graftd did for it could only be matched by timestamp, which is
not enough when several agents run commands at once, and an MCP tool error
gave the agent nothing to quote. `run/graftd.log` also grew without bound:
the CLI appends graftd's stderr to it at every start. And log lines written
to a pipe or a file contained ANSI colour codes, so they did not grep.

The op log already records which `graft run` session made each write
(the actor, Phase 10). Request IDs cover everything else.

## Options considered

1. **OpenTelemetry trace context** (`traceparent`) and an exporter. That is
   the standard, but it needs several new crates and a collector, which
   is far more than a local tool needs.
2. **A request ID per `graft` invocation**, carried in a new optional
   JSON-RPC member and recorded as a `tracing` span field on both sides.
3. For rotation: **graftd rotates its own log**, which needs it to reopen
   fd 2 (`dup2`, so `unsafe` in a crate that forbids it), or a log
   crate such as `tracing-appender`. Or **the CLI rotates before starting
   graftd**, when no daemon holds the file open.

## Decision

- `RequestId` (graft-core): 1 to 64 of `[A-Za-z0-9._-]`. The CLI uses
  `$GRAFT_REQUEST_ID` when it is set and valid. When it is invalid, the CLI
  warns and ignores it. When it is unset, the CLI makes a random 16-hex ID.
  Every log line of the command carries it as the span field
  `graft{request=<id>}`. `graft __empty-trash` inherits it. It is never
  persisted.
- JSON-RPC requests carry it as an optional `request_id` member. Older
  clients omit it, and graftd logs `-`. graftd parses it before logging and
  logs `invalid` for anything else, so a client cannot inject log lines.
  Each request is logged in an `rpc{request, method}` span: at debug when it
  is handled (with `ms`), and at warn when it fails (with `code` and `error`).
- `graft mcp` gives each tool call the ID `mcp-<pid>-<n>`, logs the call at
  info with that ID, outcome and duration, and adds `request_id` to every
  error result (an additive field to ADR 0025's format).
- Before starting graftd, the CLI moves a `graftd.log` of 8 MiB or more to
  `graftd.log.1`, replacing any older one. A graftd that runs for a long
  time still writes to one file until its next start. At the default
  `info` level it logs mounts and failures, so this file stays small.
- Both binaries colour log output only when stderr is a terminal.
- No new dependencies.

## Consequences

- `grep 'request=<id>'` over the CLI's stderr and `run/graftd.log` shows one
  command end to end. An agent's error carries the ID to quote.
- The ID covers only the CLI and graftd, not the FUSE or NFS callbacks a
  command's file accesses cause. Those come from the kernel with no caller
  context. The op log's actor covers writes made through `graft run`.
- With at most two files of about 8 MiB each, `run/` stays bounded across
  restarts but not within one very long, very noisy daemon lifetime (for
  example with `GRAFT_LOG=trace`).
