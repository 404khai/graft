# 0007: graftd and its IPC protocol
Status: accepted

## Context

A FUSE mount lives as long as the process serving it, so virtual grafts
need a long-running owner: `graftd`. AGENTS.md fixes JSON-RPC over a Unix
domain socket with `0600` permissions, started on demand by the CLI, and
asks that a restarted daemon remount existing grafts. Open questions:

1. Framing on the socket.
2. How exactly one daemon runs per `$GRAFT_HOME`.
3. How the CLI starts it.
4. Where the source of truth for "what should be mounted" lives.

## Options considered

**Framing**

- A. JSON-RPC 2.0, one JSON object per line (newline-delimited). JSON
  cannot contain a raw newline outside strings, and `serde_json` escapes
  newlines in strings, so a line is always one message. Trivial to debug
  with `socat`.
- B. LSP-style `Content-Length` headers. More robust for huge payloads, but
  no payload here is large.
- C. A binary protocol. Not JSON-RPC.

**Single instance**

- D. Check whether the socket answers. Racy: two CLIs can both see no
  daemon and both start one.
- E. An OS file lock on `run/graftd.lock`, held for the daemon's lifetime.
  The loser of a start race exits quietly; the lock is released by the
  kernel if the daemon dies, so a stale socket is safe to replace.

**Starting**

- F. Require users to start it (systemd unit, launchd agent). Touches
  system config; not on demand.
- G. The CLI connects; on `ENOENT` or `ECONNREFUSED` it spawns `graftd`
  (next to its own executable, or `$GRAFTD`) in a new process group with
  stdio detached and stderr appended to `run/graftd.log`, then retries the
  connection for up to 5 s.

**Source of truth**

- H. The daemon's memory. Lost on crash.
- I. The per-Root `graft.db`: a graft with `backend = 'fuse'` in state
  `live` should be mounted. The daemon's table of mounts is a cache of
  that, rebuilt at start-up by scanning every Root under `$GRAFT_HOME`.

## Decision

A + E + G + I.

- Socket `$GRAFT_HOME/run/graftd.sock`, created with mode `0600` inside
  `run/` (mode `0700`). The daemon refuses to start if `run/` is not owned
  by the current user or is accessible by others.
- Requests: `{"jsonrpc":"2.0","id":N,"method":M,"params":{...}}`. Methods:

  | Method | Params | Result |
  | --- | --- | --- |
  | `ping` | — | `{ "version", "pid" }` |
  | `mount` | `{ "root", "graft" }` | `{ "path" }` |
  | `unmount` | `{ "root", "graft" }` | `{ "lazy": bool }` |
  | `mounts` | — | `[{ "root", "graft", "path" }]` |
  | `shutdown` | — | `null`; unmounts everything and exits |

  `root` is a Root ID and `graft` a graft name. Errors use JSON-RPC error
  objects with a human-readable `message`; codes `-32600`..`-32603` for
  protocol errors and `1` for Graft errors.
- The CLI uses a small blocking client (std `UnixStream`); only the daemon
  depends on `tokio`. Mounting and unmounting run on tokio's blocking pool.
- `ping` reports the daemon's version. If it differs from the CLI's, the
  CLI warns; it does not restart a daemon that is serving mounts.
- At start-up, and on `mount`, the daemon reads `graft.db`. `mount` of a
  graft whose row is not `fuse` is refused, so the protocol cannot be used
  to mount arbitrary paths: the mount point is always
  `$GRAFT_HOME/roots/<root>/mounts/<name>`.
- `SIGTERM` and `SIGINT` unmount every graft (lazily if busy) and exit.

## Consequences

- `graft new --backend fuse` starts `graftd` the first time. Nothing is
  installed system-wide; `graftd` keeps running until `shutdown`, a signal,
  or reboot. Auto-exit when idle is deferred.
- Any process running as the user can talk to the socket. That is the same
  trust boundary as the user's files, and matches AGENTS.md §11.
- After a crash, mounts become stale (`ENOTCONN`) until the next CLI command
  restarts the daemon, which unmounts and remounts them.
- The MCP server will reuse the same client.
