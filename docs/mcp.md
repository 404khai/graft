# MCP reference

`graft mcp` serves Graft to MCP clients over stdio ([ADR 0019](decisions/0019-mcp-server.md)).
It runs for one Root: the directory it starts in, or `--root PATH`. Each
tool call runs the `graft` command with the same name and the arguments
below (never through a shell), so it behaves exactly as the CLI does:
trust approvals, claims, prune safety and the sandbox all apply. A failed
command is a tool error carrying its output.

Tools that change Root's history or discard work need `confirm: true`:
`graft_integrate` (unless `dry_run: true`) and `graft_prune` with
`force: true`. `graft_run` only runs commands inside the graft's sandbox.

## Resources

| URI | What |
| --- | --- |
| `graft://radar` | Radar's report as JSON: every overlap between live grafts. |
| `graft://radar/{graft}` | The overlaps involving one graft. |

Both can be subscribed to (`resources/subscribe`): the server then runs
`graft radar --watch` and sends `notifications/resources/updated` for each
subscribed URI whenever an overlap it covers appears or goes. Read the
resource again to see what changed.

## Tools

The schemas below are generated from the server and pinned by a snapshot
test (`crates/graft-mcp/src/snapshots/`).

### `graft_new`

Create a graft: a private copy-on-write workspace checked out from Root. Returns where it is.

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `backend` | string (reflink \| fuse \| nfs) | no | Filesystem backend (default: the configured one). |
| `blueprint` | string | no | Blueprint of graft.toml to provision: ports, services, env files. |
| `from` | string | no | Revision to start from (default: Root's HEAD). |
| `name` | string | yes | Graft name: lowercase letters, digits and hyphens, at most 40 characters. |

### `graft_list`

List grafts with their base, state, changed files and disk use. (read-only)

No parameters.

### `graft_status`

Show what changed in a graft since its base (or summarise every graft). (read-only)

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `name` | string | no | Graft name: lowercase letters, digits and hyphens, at most 40 characters. |

### `graft_diff`

Show a graft's changes as a unified diff. (read-only)

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `name` | string | yes | Graft name: lowercase letters, digits and hyphens, at most 40 characters. |

### `graft_run`

Run a command confined to a graft: it can only write inside the graft (and its private TMPDIR), with the graft's blueprint environment. The command is an argument list, not a shell line.

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `command` | array of string | yes | Program and arguments, e.g. ["cargo", "test"]. |
| `name` | string | yes | Graft name: lowercase letters, digits and hyphens, at most 40 characters. |
| `timeout_secs` | integer | no | Kill it after this many seconds (default 600). |

### `graft_bud`

Fork a virtual graft in constant time: a new graft with the same base and a copy of its changes.

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `dst` | string | yes | The new graft's name. |
| `src` | string | yes | Graft name: lowercase letters, digits and hyphens, at most 40 characters. |

### `graft_ring`

Checkpoint a virtual graft's changes as a named ring (restore it later with `graft rewind`), or list its rings.

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `label` | string | no | The ring's label (default ring-<seq>). |
| `list` | boolean | no | List the graft's rings instead. |
| `name` | string | yes | Graft name: lowercase letters, digits and hyphens, at most 40 characters. |

### `graft_radar`

Overlaps between live grafts, as JSON: same file (path), same symbol, merge conflicts, or tests that fail only when merged (behaviour). To be told when overlaps change, subscribe to the resource graft://radar/<graft> (or graft://radar for all). (read-only)

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `check` | boolean | no | Run the behaviour checks (speculative merges with the [radar] test) first. |
| `graft` | string | no | Only overlaps involving this graft. |

### `graft_integrate`

Merge grafts onto a branch of Root through the integrate queue: structural merge, test gate, one commit each. Changes Root's history, so it needs confirm: true unless dry_run. (destructive)

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `confirm` | boolean | no | Required (true) to land anything. |
| `dry_run` | boolean | no | Run every step except landing. |
| `message` | string | no | Commit message. |
| `names` | array of string | yes | Grafts, in order. |
| `onto` | string | no | Branch to land on. |
| `prune` | boolean | no | Prune each graft once it has landed. |
| `update_checkout` | boolean | no | Allow landing on the branch Root has checked out (fast-forwards Root's clean working tree). |

### `graft_plan`

Split a task into an ordered DAG of grafts along the code's dependency graph, as JSON. The task names files and symbols in `code spans`. With apply, creates the grafts that are ready and records their claims.

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `apply` | boolean | no | Create the ready grafts. |
| `backend` | string (reflink \| fuse \| nfs) | no |  |
| `task` | string | no | The task, as Markdown. |
| `task_file` | string | no | Or a path to it. |

### `graft_prune`

Remove a graft. Refuses if work would be lost; force discards it and needs confirm: true. (destructive)

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `confirm` | boolean | no | Required (true) with force. |
| `force` | boolean | no | Discard uncommitted or unintegrated work. |
| `name` | string | yes | Graft name: lowercase letters, digits and hyphens, at most 40 characters. |
