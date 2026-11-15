# MCP reference

`graft mcp` serves Graft to MCP clients over stdio ([ADR 0019](decisions/0019-mcp-server.md)).
It runs for one Root: the git repository it starts in, or `--root PATH`.
If that repository is not registered yet, `graft_init` registers it. Each
tool call runs the `graft` command with the same name and the arguments
below (never through a shell), so it behaves exactly as the CLI does:
trust approvals, claims, prune safety and the sandbox all apply. Commands
run with `GRAFT_MCP=1`, so hints name these tools rather than CLI commands.

## Errors

A failed call is a tool error (`isError: true`) whose text is one JSON
object ([ADR 0025](decisions/0025-mcp-errors-for-agents.md)):

```json
{"code": "not_found", "message": "no graft named a", "hint": "see graft_list", "output": ""}
```

| `code` | Meaning |
| --- | --- |
| `invalid_arguments` | The arguments do not match the tool's schema. |
| `confirm_required` | A destructive call without `confirm: true`. |
| `not_registered` | The repository is not a Root yet; call `graft_init`. |
| `invalid_name` | Not a valid graft name. |
| `not_found` | No such graft (or ring). |
| `layer_build_failed` | A `graft.toml` layer build failed; nothing was created. `message` ends with the build's output. |
| `backend_unsupported` | The graft's backend cannot do this (bud, ring and rewind need a virtual graft). |
| `work_would_be_lost` | `graft_prune` without `force` on a graft with work Root does not have. |
| `checkout_would_fall_behind` | `graft_integrate` onto the branch Root has checked out, without `update_checkout: true` (a dry run reports it too). |
| `root_not_clean` | `update_checkout`, but Root has changes to tracked files. |
| `conflict`, `gate_failed`, `not_approved`, `nothing_to_integrate`, `claimed` | Why a `graft_integrate` request stopped; each request's code is in the results in `output`, and the call's code is the first one's. |
| `command_failed` | A `graft_run` command exited non-zero or was killed; the error also carries `exit_code`, `signal`, `stdout`, `stderr`, `duration_ms`. |
| `timed_out` | The call ran past its time limit and was killed. |
| `failed` | Anything else; see `message` and `output`. |

`hint`, when not null, is the next step (it is not repeated in
`message`). `output` is what the command printed besides the error.

Every successful call returns JSON too (except `graft_diff`, a unified diff), described with each tool below;
`graft_list` and `graft_status` start with the `root` and `version` they
describe, and the server's instructions name them as well. Warnings a command prints (for example a copy fallback on a
filesystem without reflinks) are a `warnings` list in a JSON result.

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

### `graft_init`

Register the repository this server runs in as a Root, so the other tools can use it. Safe to repeat. Returns JSON {root, path, registered}; registered is false when it already was a Root. (idempotent)

No parameters.

### `graft_new`

Create a graft: a private copy-on-write workspace checked out from Root. Returns JSON {name, path, base, backend, layers: [{path, status: built|reused, key}]}. Layers declared in Root's graft.toml (e.g. node_modules) are built on the first create and shared after; if a build fails, nothing is created and the error (code layer_build_failed) shows the build's output.

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `backend` | string (reflink \| fuse \| nfs) | no | Filesystem backend: reflink (a real working tree; the default unless config.toml or $GRAFT_FS says otherwise), or a virtual graft served by graftd: fuse on Linux, nfs on macOS. Only virtual grafts support graft_bud, graft_ring and graft_rewind. |
| `blueprint` | string | no | Blueprint of graft.toml to provision: ports, services, env files. |
| `from` | string | no | Revision to start from (default: Root's HEAD). |
| `name` | string | yes | Graft name: lowercase letters, digits and hyphens, at most 40 characters. |

### `graft_list`

List grafts as JSON {root, version, grafts: [...]}: each graft's name, base, state, backend, path, changed (file count) and disk_bytes (blocks not shared with Root or the layer cache, so build outputs count; null when unknown, with the reason in note). (read-only)

No parameters.

### `graft_path`

The absolute path of a graft's working tree, as JSON {name, path}. Run shell commands there (it may contain spaces, so quote it). (read-only)

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `name` | string | yes | Graft name: lowercase letters, digits and hyphens, at most 40 characters. |

### `graft_status`

Show what changed in a graft since its base, as JSON {root, version, grafts: [...]}, each with its changes ({kind: A|M|D|..., path}). Without name, every graft. (read-only)

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `name` | string | no | Graft name: lowercase letters, digits and hyphens, at most 40 characters. |

### `graft_diff`

Show a graft's changes as a unified diff. (read-only)

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `name` | string | yes | Graft name: lowercase letters, digits and hyphens, at most 40 characters. |

### `graft_run`

Run a command confined to a graft: it can only write inside the graft (and its private TMPDIR), with the graft's blueprint environment. The command is an argument list, not a shell line. Returns JSON {exit_code, signal, stdout, stderr, duration_ms, timed_out}; a non-zero exit, a signal or a timeout is a tool error with code command_failed or timed_out and the same fields.

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `command` | array of string | yes | Program and arguments, e.g. ["cargo", "test"]. |
| `name` | string | yes | Graft name: lowercase letters, digits and hyphens, at most 40 characters. |
| `timeout_secs` | integer | no | Kill it after this many seconds (default 600). |

### `graft_bud`

Fork a virtual graft (backend fuse or nfs) in constant time: a new graft with the same base and a copy of its changes. Reflink grafts are refused (backend_unsupported). Returns JSON {name, src, path, base, entries}.

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `dst` | string | yes | The new graft's name. |
| `src` | string | yes | Graft name: lowercase letters, digits and hyphens, at most 40 characters. |

### `graft_ring`

Checkpoint a virtual graft's (fuse or nfs) changes as a named ring, to restore later with graft_rewind, or list its rings. Returns JSON {graft, label, seq, entries, at}, or a list of them with list.

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `label` | string | no | The ring's label (default ring-<seq>). |
| `list` | boolean | no | List the graft's rings instead. |
| `name` | string | yes | Graft name: lowercase letters, digits and hyphens, at most 40 characters. |

### `graft_rewind`

Restore a virtual graft's changes to a ring made with graft_ring. The state it leaves is saved first as a ring named undo-<seq>, so a rewind can itself be undone. Returns JSON {name, ring, seq, undo}.

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `name` | string | yes | Graft name: lowercase letters, digits and hyphens, at most 40 characters. |
| `ring` | string | yes | The ring's label, as graft_ring lists it. |

### `graft_radar`

Overlaps between live grafts, as JSON: same file (path), same symbol, merge conflicts, or tests that fail only when merged (behaviour). To be told when overlaps change, subscribe to the resource graft://radar/<graft> (or graft://radar for all). (read-only)

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `check` | boolean | no | Run the behaviour checks (speculative merges with the [radar] test) first. |
| `graft` | string | no | Only overlaps involving this graft. |

### `graft_integrate`

Merge grafts onto a branch of Root through the integrate queue: structural merge, the [integrate] test gate if graft.toml sets one (the result says when none ran), one commit each. Changes Root's history, so it needs confirm: true unless dry_run. Returns JSON {results: [{graft, target, state, code, commit, output}], gated}; if any request did not land, it is a tool error whose code is the first failed request's (conflict, gate_failed, checkout_would_fall_behind, root_not_clean, not_approved, nothing_to_integrate, claimed, failed) and whose output holds the results. (destructive)

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `confirm` | boolean | no | Required (true) to land anything. |
| `create_branch` | boolean | no | Create onto at Root's HEAD if it does not exist (else a missing branch is an error). |
| `dry_run` | boolean | no | Run every step and check except landing, including the checked-out-branch rule, so a passing dry run predicts the real one. |
| `message` | string | no | Commit message. |
| `names` | array of string | yes | Grafts, in order. |
| `onto` | string | no | Branch to land on (default: [integrate] branch in graft.toml, else the branch Root has checked out, which also needs update_checkout). |
| `prune` | boolean | no | Prune each graft once it has landed. |
| `update_checkout` | boolean | no | Allow landing on the branch Root has checked out: Root's working tree is fast-forwarded, which needs no changes to tracked files (untracked files are left alone). |
| `test` | string | no | Shell command to use as this call's test gate instead of graft.toml's [integrate] test, e.g. "cargo test". Runs confined to the scratch checkout; needs no graft trust. |

### `graft_plan`

Split a task into an ordered DAG of grafts along the code's dependency graph, as JSON {plan, base, nodes, width, unresolved}. The task names files, directories (every file under them) and symbols in `code spans`; spans that name nothing yet (a new file, an unknown symbol) are listed in unresolved, not placed. With apply, creates the grafts that are ready and records their claims.

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `apply` | boolean | no | Create the ready grafts. |
| `backend` | string (reflink \| fuse \| nfs) | no |  |
| `task` | string | no | The task, as Markdown. |
| `task_file` | string | no | Or a path to it. |

### `graft_prune`

Remove a graft. Refuses if work would be lost (code work_would_be_lost); force discards it and needs confirm: true. Returns JSON {name, pruned}. (destructive)

| Parameter | Type | Required | Meaning |
| --- | --- | --- | --- |
| `confirm` | boolean | no | Required (true) with force. |
| `force` | boolean | no | Discard uncommitted or unintegrated work. |
| `name` | string | yes | Graft name: lowercase letters, digits and hyphens, at most 40 characters. |
