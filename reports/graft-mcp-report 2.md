# Graft MCP server — agent-session test report (run 2, 2026-11-15)

**Result: BLOCKED at step 2, by session configuration and a broken host `node`, not by a Graft defect.** Steps 2–9 and the exit-3 case in step 10 were not run. Nothing was worked around: I did not use the CLI, edit `graft.toml` or `.mcp.json`, or patch node.

## Root cause

- The test was meant to run against `bench-graft`, and `bench-graft/.mcp.json` is correctly set up for that: dev build `/Users/admin/Developer/graft/target/release/graft`, `--root …/bench-graft`, with `PATH` putting nvm node v24.16.0 first.
- The Claude Code session was started in `~/Developer/graft-tests`, so it loaded **`~/Developer/graft-tests/.mcp.json`** instead:
  ```json
  "command": "/opt/homebrew/bin/graft", "args": ["mcp","--root","/Users/admin/Developer/graft-tests/bench-app"], "env": {}
  ```
  Running process: `83098 /opt/homebrew/bin/graft mcp --root /Users/admin/Developer/graft-tests/bench-app`.
- So the server ran the Homebrew 0.1.0 binary, used **Root = bench-app** (the worktree-benchmark repo, also at `2edd69a`), and inherited a PATH where `node` = `/opt/homebrew/bin/node` (26.5.0). That node aborts on launch:
  ```
  dyld[84267]: Library not loaded: /opt/homebrew/opt/simdjson/lib/libsimdjson.33.dylib
    Referenced from: … /opt/homebrew/Cellar/node/26.5.0/bin/node
  graft: creating graft a: layer node_modules: build `npm ci` failed (signal: 6 (SIGABRT))
  ```
- Every `graft_new` has to build the `node_modules` layer (`build = "npm ci"`), so no graft can be created on any backend.

**To unblock:** start Claude Code from `~/Developer/graft-tests/bench-graft`, so its `.mcp.json` is the one loaded. Alternatively, run `brew reinstall simdjson node`, or remove or fix the parent `.mcp.json`. Then rerun.

## Environment

| | |
|---|---|
| macOS | 27.0 (26A428), Darwin 27.0.0 |
| CPU | Apple M3 |
| Load average, start | 7.22 7.68 9.37 |
| Load average, end | 7.72 7.73 9.28 |
| Free disk (Data volume) | 39 GiB of 460 GiB (92 % used) |
| graft binary actually used by MCP | `/opt/homebrew/bin/graft` → `Cellar/graft/0.1.0`, `graft 0.1.0` |
| graft binary intended (bench-graft/.mcp.json) | `/Users/admin/Developer/graft/target/release/graft`, `graft 0.1.0` |
| Claude Code | 2.1.285 |

## Tool calls

Times are wall-clock between `date` stamps I took just before and just after each call. They include roughly 0.5–1 s of harness overhead. The calls without a time ran as one parallel batch, so their individual times can't be separated.

| # | Tool | Args | Wall | Result | Note |
|---|---|---|---|---|---|
| 1 | graft_plan | task = A–D in Markdown | 1.7 s | ok | 2 nodes, `types` and `m050` (see below) |
| 2 | graft_new | a (default backend) | 0.75 s | **error** | `npm ci` SIGABRT, broken Homebrew node |
| 3 | graft_new | v, backend nfs | — | **error** | same layer failure. The NFS backend still builds the layer. |
| 4 | graft_new | Bad_Name | — | error (expected) | `invalid graft name "Bad_Name": may only contain lowercase letters, digits and hyphens`. Clear. |
| 5 | graft_status | nope | — | error (expected) | `no graft named nope; see `graft ls``. Clear, but points to the CLI. |
| 6 | graft_list | — | — | ok | `No grafts. Create one with `graft new <name>`.` Plain text, not JSON. |
| 7 | graft_radar | — | — | ok | `{"overlaps":[],"notes":[]}` |
| 8 | graft_run | nope, `sh -c "exit 3"` | — | error | Failed on the missing graft, so the exit-3 path was **not tested** |
| 9 | graft_integrate | [a], dry_run | — | error (expected) | `no graft named a; see `graft ls`` |

**graft_plan output (step 1), verbatim:**
```json
{"base":"2edd69af…","nodes":[
 {"claims":[{"path":"src/core/types.ts","symbol":null},{"path":"test/core.test.ts","symbol":null}],"contract":false,"deps":[],"index":0,"level":0,"name":"types"},
 {"claims":[{"path":"src/modules/m050/f10.ts","symbol":null}],"contract":false,"deps":[],"index":1,"level":0,"name":"m050"}],
 "plan":"3e26eba2bbfa4cb1","width":2}
```
- **A and D are grouped** into the node `types`, at path level (`symbol: null`).
- **B is missing.** `src/modules/m002` names a directory, and the plan seems to accept only file paths.
- **A's new file `test/convert.test.ts` is missing**, probably because it doesn't exist yet.
- The names are not a–d, so I did not apply it.

## Not run (blocked)

Steps 2 (b, c, d), 3–7, 8 (integrate dry_run / no-confirm / confirm), 9 (prune safety) and the exit-3 case of 10. There are no results for these.

## Cleanup (step 11)

- `graft_list`: no grafts.
- Root (bench-app) `git status`: clean, HEAD `2edd69a`. bench-graft is untouched apart from its pre-existing untracked `.claude/` and `.mcp.json`.
- `mount | grep graft`: nothing.
- Graft home `mounts/` directories: empty.
- No reset was needed, since main is already at `2edd69a`.

## Confusing / missing information in schema or results

1. **The server doesn't say which Root it serves.** The instructions say "of this repository", and no tool result names the Root path. I only found out it was bench-app from `ps`. `graft_list` and `graft_plan` should include `root`, and so should the server instructions.
2. **Layer-build failures look like `graft_new` failures.** The error is dozens of dyld lines followed by `layer node_modules: build `npm ci` failed (signal: 6 (SIGABRT))`. Nothing says which `node` or PATH was used, or that the cause is the environment the MCP server was launched with.
3. **Errors point to CLI commands**: `see `graft ls``, `Create one with `graft new <name>``. Over MCP they should name `graft_list` / `graft_new`.
4. **Output formats are mixed.** `graft_plan` and `graft_radar` return JSON, `graft_list` returns prose, and errors are free text with no code. That makes them hard for an agent to branch on.
5. **`graft_plan`'s `task` description**, "The task names files and symbols in `code spans`", doesn't say that directories and not-yet-existing files are dropped. B disappeared without any warning.
6. **`graft_bud` / `graft_ring` descriptions** say "virtual graft" but don't say which backends are virtual (`fuse`/`nfs`?) or what happens on reflink. I couldn't test the refusal.
7. **`graft_ring` mentions `graft rewind`**, which has no MCP tool. The rewind path is missing over MCP.
8. **`graft_new` description** says "Returns where it is" but not the shape (JSON or text) or whether it includes the layer message. I couldn't check this.

## CLI vs MCP

- The CLI run (`graft-vs-worktree-report.md` §3–§7) completed every step in bench-graft. It ran through a shell whose PATH resolved a working node.
- Over MCP, the server inherits the environment of the client that launched it (`env: {}` here). So the same `graft.toml` fails at layer build. The CLI didn't have this failure mode.
- Name validation, missing-graft errors and an empty radar behave the same in both.
- `rewind` exists in the CLI but has no MCP tool.

## Bugs

1. **should-fix — layer builds depend on the MCP client's environment without saying so.** Repro: start `graft mcp` with `env: {}` from a client whose PATH resolves a broken `node`, then call `graft_new a`. Result: the error doesn't say which binary or PATH it used. Expected: the error names the resolved `npm`/`node` and PATH, or the blueprint/`graft.toml` can pin the toolchain environment.
2. **should-fix — `graft_plan` drops directory claims and new files without telling you.** Repro: `graft_plan` with task "edit three files in `src/modules/m002`" plus "new test in `test/convert.test.ts`". Result: neither appears in the plan, and there's no note. Expected: expand the directory, or report "unresolved claims".
3. **nit — MCP error and empty-state text refers to CLI commands** (`graft ls`, `graft new <name>`).
4. **nit — `graft_list` returns prose while `graft_radar` and `graft_plan` return JSON.**
5. **nit — no tool result identifies Root**, which is how a wrong-Root session went unnoticed until `ps`.

(Not a Graft bug: Homebrew `node` 26.5.0 is linked against a missing `libsimdjson.33.dylib`.)

## Top 5 changes before 0.1.1

1. **Put Root and the binary version in the MCP server instructions and in `graft_list` / `graft_status` results.** That would have caught this session's misconfiguration at the first call.
2. **Make layer-build failures diagnosable.** Return a structured error `{code:"layer_build_failed", layer, command, exit/signal, path_used, stderr_tail}` and cap the stderr dump.
3. **Give every MCP error a machine-readable code** (`invalid_name`, `not_found`, `needs_confirm`, `layer_build_failed`, …) and return JSON for `graft_list`, `graft_status` and `graft_run` (with `exit_code`).
4. **`graft_plan`: resolve directory claims and new-file paths, or list unresolved claims.** Today, missing work items fail silently.
5. **Word MCP results for MCP:** refer to `graft_*` tool names rather than CLI commands, add a `graft_rewind` tool (or drop the mention), and say in the bud/ring descriptions which backends are virtual.
