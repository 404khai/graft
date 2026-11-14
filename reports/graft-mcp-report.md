# Graft MCP server — agent-session test report

**Result: BLOCKED at step 2, by the environment and not by Graft.** The MCP connection works and all 11 `graft_*` tools are exposed. However, no graft could be created on either backend. Root's `graft.toml` declares a `node_modules` layer built with `npm ci`, and Homebrew's `node` on this machine crashes on launch with a missing dylib. Every step that needs a live graft (2–9, plus the exit-3 case in step 10) could not be run. I did not patch node, edit `graft.toml` or use the CLI to work around it. Everything that could be run without a graft is recorded below.

## Step 0 — connection

PASS. `ToolSearch "graft"` returned 11 deferred tools. The server process was running: `/opt/homebrew/bin/graft mcp --root /Users/admin/Developer/graft-tests/bench-app` (pid 29933).

| Tool | Required | Optional |
|---|---|---|
| `graft_new` | `name` | `backend` (reflink\|fuse\|nfs), `blueprint`, `from` |
| `graft_run` | `name`, `command[]` (min 1) | `timeout_secs` (1–86400, default 600) |
| `graft_list` | — | — |
| `graft_status` | — | `name` |
| `graft_diff` | `name` | — |
| `graft_radar` | — | `check`, `graft` |
| `graft_plan` | — | `task`, `task_file`, `apply`, `backend` |
| `graft_bud` | `src`, `dst` | — |
| `graft_ring` | `name` | `label`, `list` |
| `graft_integrate` | `names[]` (min 1) | `confirm`, `dry_run`, `message`, `onto`, `prune`, `update_checkout` |
| `graft_prune` | `name` | `force`, `confirm` |

Every `name` field has the same description: "Graft name: lowercase letters, digits and hyphens, at most 40 characters." All objects have `additionalProperties: false`.

## Environment

| | Start | End |
|---|---|---|
| macOS | 27.0 (26A428) | |
| CPU | Apple M3, 8 cores | |
| Load average (1/5/15) | 12.64 11.41 14.43 | 21.28 14.40 15.20 |
| Free disk (/System/Volumes/Data) | 41 GiB (91% used) | 41 GiB |
| graft | `/opt/homebrew/bin/graft`, `graft 0.1.0` | |
| Claude Code | 2.1.285 | |
| node | `/opt/homebrew/bin/node` → Cellar/node/26.5.0: **broken** (see below). `~/.nvm` has v24.16.0, but it is not on PATH. | |
| Root | `bench-app`, `main` at `2edd69a`, clean (only the ignored `.mcp.json` and `node_modules/`) | unchanged |

**The blocker.** Running `node --version` directly in a shell gives:
```
dyld: Library not loaded: /opt/homebrew/opt/simdjson/lib/libsimdjson.33.dylib
  Referenced from: /opt/homebrew/Cellar/node/26.5.0/bin/node
```
Only `libsimdjson.34.dylib` is installed (simdjson 5.0.4). `brew` reports node as outdated (26.5.0 installed, 26.13.1 stable). The likely fix is `brew upgrade node` (or `brew reinstall node`). That is a system change, so I did not make it; it needs your approval.

**Timing method.** I ran a Bash `date` timestamp (`python3 time.time()`) immediately before and after each MCP call, in the same tool-call block. The calls executed in order. Each figure therefore includes Claude Code's harness overhead. I measured the floor with two back-to-back Bash timestamp calls: **0.93 s**. Graft's own time is roughly wall time minus 0.5–0.9 s. Graft reports no timing of its own.

## Tool-call table

| # | Tool | Arguments | Wall (s) | Result | Note |
|---|---|---|---|---|---|
| 1 | `graft_plan` | 4-edit task (A–D, Markdown, code spans) | 2.01 | ok | DAG with 2 nodes (`types`, `m050`). A+D grouped. B dropped. Not applied. |
| 2 | `graft_new` | `a` (default backend) | 2.09 | **error** | Layer `npm ci` → SIGABRT (node dyld crash). No graft left behind. |
| 3 | `graft_list` | — | not timed | ok | `No grafts. Create one with \`graft new <name>\`.` |
| 4 | `graft_new` | `v`, backend `nfs` | 3.64 | **error** | Same layer failure. The NFS backend also needs the layer. This call started `graftd` (pid 32041). |
| 5 | `graft_new` | `Bad_Name` | 1.53 | error (expected) | `graft: invalid graft name "Bad_Name": may only contain lowercase letters, digits and hyphens`. Clear. |
| 6 | `graft_status` | `nosuch` | 1.15 | error (expected) | `graft: no graft named nosuch; see \`graft ls\``. |
| 7 | `graft_run` | `nosuch`, `sh -c "exit 3"` | 1.63 | error | Missing graft, so the exit-3 case could **not** be tested. |
| 8 | `graft_radar` | — | 1.70 | ok | `{"overlaps":[],"notes":[]}` |
| 9 | `graft_status` | — (all) | 1.01 | ok | `No grafts.` (plain text, not JSON). |
| 10 | `graft_integrate` | `[a]`, `dry_run` | 1.45 | error (expected) | `graft: no graft named a; see \`graft ls\``. |
| 11 | `graft_prune` | `a` | 1.90 | error (expected) | Same "no graft named" message. |
| 12 | `graft_bud` | `a` → `a-alt` | 1.87 | error (expected) | Same "no graft named" message. The reflink refusal could not be tested. |
| 13 | `graft_integrate` | `[a]`, no confirm, no dry_run | not timed | error (expected) | `graft_integrate lands commits in Root: pass confirm: true to do it, or dry_run: true to check first`. The confirm check runs before the existence check. |
| 14 | `graft_radar` | `check: true` | not timed | ok | `{"overlaps":[],"notes":[]}`. See confusion #6. |
| 15 | `graft_ring` | `a`, `list` | not timed | error (expected) | Same "no graft named" message. |

Calls 13–15 were sent in one block bracketed by a single pair of timestamps, so they have no individual times.

### Step-by-step status

| Step | Status |
|---|---|
| 0 Connection | PASS |
| 1 Plan | Done; recorded below |
| 2 graft_new a–d | **BLOCKED** (layer build fails) |
| 3 Edits + build/test | BLOCKED (needs grafts; node is broken anyway) |
| 4 list/status/diff on grafts | BLOCKED (ran only with no grafts) |
| 5 Radar with 4 live | BLOCKED (ran only with no grafts: empty) |
| 6 Isolation/sandbox | BLOCKED |
| 7 Bud/ring (reflink refusal, nfs) | BLOCKED (only the "missing graft" path was exercised) |
| 8 Integrate | Partially done: the no-confirm refusal works. Dry run and land are BLOCKED. Root `npm run build && npm test` was not run, because node is broken. |
| 9 Prune safety | BLOCKED (prune on a missing graft only) |
| 10 Error handling | 2 of 3 done (bad name, missing graft). Exit-3 is BLOCKED. |
| 11 Cleanup | Done (see the end of this report) |

### Step 1 — `graft_plan` output (verbatim)
```json
{"base":"2edd69af5ff0263f500a2fee19e927de60035346","nodes":[
 {"claims":[{"path":"src/core/types.ts","symbol":null},{"path":"test/core.test.ts","symbol":null}],
  "contract":false,"deps":[],"index":0,"level":0,"name":"types"},
 {"claims":[{"path":"src/modules/m050/f10.ts","symbol":null}],
  "contract":false,"deps":[],"index":1,"level":0,"name":"m050"}],
 "plan":"d6f11f4dc7b5124d","width":2}
```
- **A and D are grouped** into one node, `types`. Its claims are `src/core/types.ts` and `test/core.test.ts`, at path level (`symbol: null`), even though the task named `` `convert` `` and `` `format` `` in code spans.
- **B is missing.** The task named `` `src/modules/m002` `` (a directory) and the planner produced no node and no note for it.
- **`test/convert.test.ts`** (a new file A creates) is not claimed by any node.
- The nodes have no edges (`deps: []`, both at level 0, `width: 2`).
- The node names are generated (`types`, `m050`), not a–d, so I did not apply the plan, as instructed.

### Step 10 — are the errors machine-readable?
Each error came back as an MCP tool error (`isError`) whose content is a single human-readable string prefixed `graft:`. There is no error code, kind or structured field. An agent can tell that the call failed, but it can only tell *why* by matching on the text. For example, "not found", "invalid name" and "layer build failed" all look the same apart from their wording. The messages themselves are clear and short.

## Confusing or missing items in the schema, descriptions and results

1. **Errors and empty results point to CLI commands the agent is not supposed to use.** Examples: `see \`graft ls\`` (should be `graft_list`) and `Create one with \`graft new <name>\`` (should be `graft_new`). The `graft_ring` description says "restore it later with `graft rewind`", but **there is no rewind tool over MCP**. An MCP-only agent can checkpoint a ring but never restore it.
2. **`graft_new`**: "Returns where it is". I could not see the success format. `backend`: "default: the configured one". The tool never says which backend is configured, and nothing in the schema mentions that a `graft.toml` layer build runs on every create and can fail. Its failure took down both backends.
3. **The layer-build failure message** was clear about the cause: `graft: creating graft a: layer node_modules: build \`npm ci\` failed (signal: 6 (SIGABRT))`, preceded by the raw dyld stderr. However, it gave no next step (retry, skip the layer, or fix the toolchain), and no tool argument lets you create a graft without the layer.
4. **`graft_plan`**: "The task names files and symbols in `code spans`." Two things are unexplained. First, a directory span (`src/modules/m002`) is silently ignored. Second, symbol spans did not produce symbol claims. The `plan` id (`d6f11f4dc7b5124d`) is returned, but no tool accepts it, so there is no way to apply a reviewed plan later or to choose node names.
5. **`graft_bud`**: "Fork a virtual graft in constant time." The description does not say which backends count as "virtual" (is it nfs only, or fuse too?). `src` reuses the generic name description.
6. **`graft_radar`** returned `"notes":[]` even with `check: true`. In the CLI run, `graft radar` printed `note: behaviour checks are off: graft.toml has no [radar] test` for the same `graft.toml`. With no live grafts that may be intended, but a caller who explicitly asks for `check: true` should be told that it did nothing.
7. **Mixed output formats.** `graft_plan` and `graft_radar` return JSON, while `graft_list` and `graft_status` return prose (`No grafts.`). An agent has to parse both.
8. **`graft_integrate`**: "structural merge, test gate". Nothing says whether a test gate is configured for this Root, and `onto` has no stated default. I could not see whether the result reports the gate, because the call never got past the existence check.

## Comparison with the CLI report (2026-11-14)

- **Worked in the CLI run but not over MCP:** everything that needs a graft. This is **not an MCP regression**. In the CLI run, node worked and the layer built in 3.84 s. The layer was later removed by `graft gc`, so this run needed a cold rebuild, and node has broken since then. Both interfaces would fail the same way today. I did not verify this with the CLI, because the CLI was off-limits.
- **Same behaviour:** name validation, the "no graft named" errors and empty radar.
- **MCP-only:** the `confirm: true` gate on `graft_integrate` and on forced `graft_prune` (the integrate refusal is verified). This guard does not exist in the CLI and works as intended.
- **CLI-only:** `graft rewind`, `graft queue`, `graft gc`, `graft daemon stop` and `graft path` have no MCP equivalents. In particular, an MCP agent cannot stop the `graftd` that `graft_new --backend nfs` started, or garbage-collect layers.

## Bugs

| # | Label | Bug | Steps to reproduce |
|---|---|---|---|
| 1 | **should-fix** | When the layer build fails, no graft can be created at all, on any backend. A broken toolchain therefore blocks agents that would never run node, and there is no opt-out argument. | Make `npm ci` fail (here, a broken node). Call `graft_new {name:"a"}` and `graft_new {name:"v", backend:"nfs"}`. Both fail. |
| 2 | **should-fix** | MCP error and hint text names CLI commands (`graft ls`, `graft new`, `graft rewind`) instead of MCP tools, and `graft_ring` promises a restore path that MCP does not have. | `graft_status {name:"nosuch"}` → `see \`graft ls\``. `graft_list` with no grafts → `graft new <name>`. Read the `graft_ring` description. |
| 3 | **should-fix** | Errors are not machine-readable: plain text only, with no code or kind. | Any failing call, e.g. `graft_new {name:"Bad_Name"}`. |
| 4 | **should-fix** | `graft_plan` silently drops a task item that names a directory. It also ignores new files and symbol spans (claims come back with `symbol: null`). | Run `graft_plan` with the task in Step 1. B (`src/modules/m002`) is absent with no note, and `test/convert.test.ts` is unclaimed. |
| 5 | nit | A failed create leaves an empty `roots/<id>/mounts/.layer-build/` directory behind. The `grafts` table stays clean. | Run `graft_new` with a failing layer, then `ls "~/Library/Application Support/graft/roots/72365a…/mounts"`. |
| 6 | nit | A failed NFS create starts `graftd` and leaves it running, and no MCP tool can stop it. | Run `graft_new {backend:"nfs"}` with a failing layer, then `ps aux \| grep graftd` (pid 32041, still running). |
| 7 | nit (unverified) | `graft_radar {check:true}` returns empty `notes` when no `[radar] test` is configured, whereas the CLI prints a note. This was observed only with zero live grafts. | `graft_radar {check:true}` on this Root. |
| 8 | nit | The layer-build error embeds unbounded raw child stderr (the dyld search paths here) ahead of the summary line. | Same as bug 1. |

There are no blocker-level Graft bugs. The only blocker in this run is the environment (the broken Homebrew node).

## Top 5 recommended changes before 0.1.1

1. **Structured MCP errors**: return `{code, message, hint}` (for example `not_found`, `invalid_name`, `layer_build_failed`, `confirm_required`, `backend_unsupported`). Also make `graft_list` and `graft_status` return JSON like `graft_plan` and `graft_radar` do.
2. **Use MCP vocabulary in MCP mode.** Hints should name `graft_list` and `graft_new`, not `graft ls` and `graft new`. Add a `graft_rewind` tool, or remove the rewind promise from the `graft_ring` description.
3. **Make layer failure survivable.** Add a `graft_new` option to skip or defer layers, or create the graft and report the layer as failed. Include a next step in the error. Also say in the `graft_new` description that `graft.toml` layers are built on create.
4. **`graft_plan` should report what it could not place** (directories, new files and unmatched spans) in a `notes` field. It should also emit symbol-level claims when symbols are named, and let callers set node names or apply a plan by its `plan` id.
5. **Clean up after a failed create, and give MCP lifecycle control.** Remove `.layer-build` on failure. Either don't start `graftd` until a create succeeds, or expose daemon stop and `gc` over MCP. Also say which backends count as "virtual" in the `graft_bud` and `graft_ring` descriptions.

## Cleanup (step 11)

- `graft_list` → `No grafts.`
- Root `git status --short` → empty. Only the ignored `.mcp.json` and `node_modules/` are present, both untouched.
- `mount | grep graft` → nothing.
- Still present: the `graftd` daemon (pid 32041) that `graft_new --backend nfs` started at 00:11. No MCP tool stops it, so I left it running. The empty `mounts/.layer-build` directory is also still there.
- **`main` is still at `2edd69a`**, because nothing was integrated. No reset is needed, and I did not touch `main`. If you want the full run, fix node first (for example `brew upgrade node`, which needs your approval) and re-run this test. After that run, resetting `main` to `2edd69a` would need your approval.
- The Graft source repo (`/Users/admin/Developer/graft`) was not modified.
