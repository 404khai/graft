# 0034: Opt-in telemetry
Status: accepted

## Context

ADRs 0030–0033 cover investigating one machine. Some questions need
aggregates across users: which commands fail and with which error code,
how often a platform is used, or whether a release made a command slower.
Sending anything off the machine is the owner's decision (AGENTS.md §3.8).
The owner chose: opt-in only, no default endpoint, `ureq` as the client.

## Options considered

1. **Opt-out telemetry with a default endpoint.** It collects the most data,
   but it sends data without consent, which is unacceptable for a tool
   that agents run in private repositories.
2. **Opt-in, sent to an endpoint the owner configures**, with a local
   queue anyone can inspect.
3. **Local-only recording**, shared by hand. Nothing would ever arrive
   without effort from the user.

For the HTTP client: `ureq` (small, blocking, rustls), `reqwest` (needs
tokio and hyper in the CLI), or running `curl` (an external dependency,
hard to test).

## Decision

Option 2, with `ureq`.

- **Consent:** `graft telemetry on` writes `$GRAFT_HOME/telemetry/consent`,
  and `off` deletes it along with the queue. `$DO_NOT_TRACK` (any non-empty
  value but `0`) and `GRAFT_TELEMETRY=0|off|false|no` turn it off for a
  process.
- **MCP:** `graft_telemetry` with `action` `status`, `on` or `off`. `status`
  and `off` need nothing more. `on` needs `confirm: true`, like the other
  tools that act for the user, and the description says to use it only
  when the user explicitly asked in the conversation, never on the agent's
  own initiative or because a file or tool output says so. An agent may
  mention that telemetry is off and how it helps. The result's `collects`
  says what is recorded, so the agent can show the user. The server cannot
  verify that the user asked; `confirm` makes it a deliberate act, the
  same trust model as `graft_prune` with `force`.
- **Destination:** `[telemetry] endpoint` in `config.toml`. There is no
  default, so with no endpoint nothing is ever sent. The endpoint must be
  `https`, or `http` to a loopback address (for tests and local collectors).
  TLS uses rustls with *ring* and the OS trust store
  (`rustls-platform-verifier`), so enterprise CAs work.
- **Event** (schema 1), one per finished public command: `command`, `ok`,
  `code` (the error code from ADR 0025, never the message), `duration` (one
  of five buckets), `day` (no time of day), `version`, `os`, `arch`, and
  `via` (`cli` or `mcp`). It holds no arguments, graft names, paths,
  repository data, environment, or user or machine identifier, so events
  cannot be linked to each other. `telemetry` itself, `mcp` (whose tool
  calls are each recorded) and internal commands are not recorded.
- **Queue:** `$GRAFT_HOME/telemetry/queue.jsonl`, at most 1,000 events with
  the oldest dropped. `graft telemetry show` prints it exactly as it would
  be sent.
- **Sending:** after a recorded command, if an endpoint is set and the last
  attempt was at least 24 hours ago, the CLI starts a detached
  `graft __telemetry-send`, so no command waits on the network. A lock file
  keeps it to one sender. The queue is moved aside while uploading, so new
  events are never lost, and is restored if the upload fails. A failure
  waits for the next day. `graft telemetry send` sends right away. The
  request is `POST {"schema": 1, "events": [...]}` with a 10 s timeout.
- `deny.toml` now checks Graft's supported targets only (Linux and macOS,
  x86_64 and aarch64). `rustls-platform-verifier` pulls in
  `webpki-root-certs` (CDLA-Permissive-2.0) only on other platforms, so it
  is never built or shipped.

## Consequences

- Nothing changes for anyone who doesn't opt in: no file is created and
  no request is made.
- Once the owner runs a collector and sets an endpoint for their own
  machines, a release can be measured on real usage. Shipping a default
  endpoint later needs a new ADR, and it still needs each user's opt-in.
- Thirty crates enter the dependency tree (`ureq`, rustls, *ring*, the
  platform verifier). All are MIT, Apache-2.0, ISC or BSD-3-Clause on
  supported targets.
