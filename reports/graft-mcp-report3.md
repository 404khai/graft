# Graft MCP server — agent-session test report (run 3, 2026-11-15)

**Result: all 11 steps completed.** Every safety check behaved correctly:
- sandbox
- isolation
- confirm on integrate
- force + confirm on prune
- bud/ring refusal on reflink

A–D landed on `main` as 4 linear commits, and Root builds and passes tests. The main weaknesses are result shape and wording. Error codes are too coarse (`failed`), `graft_run` never reports an exit code, several messages still use CLI flag names, and a dry run doesn't catch the checked-out-branch refusal.

Previous runs (1, 2) were blocked because the session loaded `~/Developer/graft-tests/.mcp.json` (Homebrew graft, Root = bench-app, broken Homebrew node). This run was started from bench-graft.

## Preflight (before step 1)

| Check | Result |
|---|---|
| `ps ax \| grep "graft mcp"` | `98232 … /Users/admin/Developer/graft/target/release/graft mcp --root /Users/admin/Developer/graft-tests/bench-graft` ✅ |
| `graft_list` | `[]` |
| `graft_status` (no name) | `[]` |

The binary and Root are as required, so I continued. Note that neither tool result names the Root or the binary. I could only confirm them through `ps`.

## Environment

| | |
|---|---|
| macOS | 27.0 (26A428), Darwin 27.0.0 |
| CPU | Apple M3 |
| Load average, start (00:58) | 6.81 7.52 8.55 |
| Load average, end (01:01) | 24.33 15.91 11.92 (other activity on the machine during the final minutes) |
| Free disk (Data volume) | 39 GiB of 460 GiB, start and end |
| graft binary (MCP server) | `/Users/admin/Developer/graft/target/release/graft`, `graft 0.1.0`, built Nov 15 00:41. This dev build differs from Homebrew 0.1.0: JSON errors, `graft_rewind`, JSON list/status. |
| Claude Code | 2.1.285 |
| node used inside grafts | resolved from the `.mcp.json` `env.PATH` (nvm v24.16.0) |

## Timing method

MCP calls can't be timed from inside the call. I took a `date` stamp in a Bash call issued just before and just after each MCP call, in the same message.
- Each figure includes harness and turn overhead of about 1 s. Treat **≈1.1–1.3 s as the floor**: `graft_new` probably takes well under 1 s of real work.
- For calls batched with other MCP calls in one message, the stamps aren't reliable. Those rows are marked "batch" or "≈".

## Tool calls

| # | Tool | Args | Wall (s) | Result | Note |
|---|---|---|---|---|---|
| 0a | graft_list | — | — | ok | `[]` |
| 0b | graft_status | — | — | ok | `[]` |
| 1 | graft_plan | task: A–D in Markdown | 1.65 | ok | 2 nodes, `types` (A+D) and `m050` (C). B and `test/convert.test.ts` dropped. Not applied. |
| 2 | graft_new | a | 1.15 | ok | `3009 files reflinked, 0 copied…`, `layer node_modules: reused (1b3ea535cb37)` |
| 3 | graft_new | b | 1.25 | ok | same |
| 4 | graft_new | c | 1.14 | ok | same |
| 5 | graft_new | d | 1.30 | ok | same |
| 6 | graft_run | a, `sh -c "npm run build && npm test"` | 4.96 | ok | tsc + vitest output, `Tests 4 passed (4)`. No exit code in the result. |
| 7 | graft_run | b, same | 4.86 | ok | `Tests 3 passed (3)` |
| 8 | graft_run | c, same | 5.07 | ok | `Tests 3 passed (3)` |
| 9 | graft_run | d, same | 5.02 | ok | `Tests 3 passed (3)` (new expectations) |
| 10 | graft_list | — | batch | ok | JSON: backend, base, changed 2/3/1/2, disk_bytes ≈24.9 MB each, path, state live |
| 11–14 | graft_status | a, b, c, d | batch | ok | JSON change lists match the edits exactly |
| 15–18 | graft_diff | a, b, c, d | batch (all 9 calls: 3.95) | ok | Unified diffs match the edits byte for byte. `dist/` not included. |
| 19 | graft_radar | — | 1.46 | ok | `{"level":"path","grafts":["a","d"],"path":"src/core/types.ts","symbols":[],"detail":""}` |
| 20 | graft_radar | check: true | 1.51 | ok | identical, plus note `behaviour checks are off: graft.toml has no [radar] test` |
| 21 | graft_run | a, `touch ../escape` | 1.83 | error ✅ | `{"code":"failed",…,"output":"touch: ../escape: Operation not permitted"}` |
| 22 | graft_run | a, `touch <Root>/escape2` | 1.86 | error ✅ | `Operation not permitted` |
| 23 | graft_run | a, `touch ok` | ≈1.5 | ok ✅ | `(completed with no output)`. `ok` exists in a only. |
| 24 | graft_bud | a → a-alt | 1.90 | error ✅ | `backend_unsupported`, hint `create a virtual graft with graft_new with backend: "nfs"` |
| 25 | graft_ring | a, before-test | 1.67 | error ✅ | `backend_unsupported`, "cannot take rings yet (ADR 0013)" |
| 26 | graft_new | v, backend nfs | 1.81 | ok | `(base 2edd69a, virtual: served by graftd)`, layer reused |
| 27 | graft_run | v, write `v-note.txt` | — | ok | setup |
| 28 | graft_bud | v → v-alt | 1.95 | ok | `Budded graft v-alt … (1 delta entries, blobs shared)`. Status of v-alt shows `A v-note.txt`. |
| 29 | graft_ring | v, before-test | 1.84 | ok | `Ringed graft v as before-test (op 2, 1 delta entries)` |
| 30 | graft_ring | v, list | — | ok | **Plain-text table**, not JSON |
| 31 | graft_run | v, modify + add file | — | ok | setup for rewind |
| 32 | graft_rewind | v, before-test | ≈2.0 | ok | Restored exactly (content `note`, extra file gone). `To undo: graft_rewind with name: "v", ring: "undo-6"` |
| 33 | graft_run | a, `rm ok` | — | ok | Removed the step-6 probe so it wouldn't land on main |
| 34 | graft_integrate | a,b,c,d onto main, dry_run | 2.35 | ok | `checked a → main` ×4, `note: no test gate ran: set [integrate] test in graft.toml…`. Main unchanged. |
| 35 | graft_integrate | same, no confirm | 2.06 | error ✅ | `confirm_required`: "pass confirm: true to do it, or dry_run: true to check first". Main unchanged. |
| 36 | graft_integrate | same, confirm | 2.50 | error | `code:"failed"`, ×4 `main is checked out in Root … pass --update-checkout to fast-forward Root`. Nothing landed. |
| 37 | graft_integrate | same, confirm + update_checkout | 2.94 | ok | `landed a → main a72650b85e6c` … `d → main 0618e8b11571`, plus the no-test-gate note |
| 38 | graft_run | v, write `uncommitted.txt` | — | ok | setup |
| 39 | graft_list | — | — | ok | a–d `integrated`, v and v-alt `live` (nfs, ≈295 KB) |
| 40 | graft_prune | v | batch ≈0.2 | error ✅ | `code:"failed"`: "has work that pruning would lose: changes since base: uncommitted.txt, v-note.txt … rerun with --force" |
| 41 | graft_prune | v, force | batch ≈0.3 | error ✅ | `confirm_required`: "pass confirm: true to do it" |
| 42 | graft_prune | v, force + confirm | batch ≈2.0 | ok | `Pruned graft v` |
| 43–47 | graft_prune | a, b, c, d; v-alt (force+confirm) | batch, 3.39 total | ok | `Pruned graft …` ×5. a-alt never existed. |
| 48 | graft_new | Bad_Name | — | error ✅ | `invalid_name`, clear message |
| 49 | graft_status | nope | — | error ✅ | `not_found`, hint `see graft_list` |
| 50 | graft_new | e (for exit-3 test) | — | ok | base 0618e8b, 3010 files |
| 51 | graft_run | e, `sh -c "echo to-stdout; echo to-stderr >&2; exit 3"` | 2.14 | error | `{"code":"failed","hint":null,"message":"the command failed","output":"to-stdout\nto-stderr"}`. **Exit code 3 not reported.** stdout and stderr merged. |
| 52 | graft_prune | e | — | ok | |
| 53 | graft_list | — | — | ok | `[]` |

## Step results

1. **Plan:**
   ```json
   {"base":"2edd69af…","nodes":[
    {"name":"types","index":0,"level":0,"deps":[],"contract":false,
     "claims":[{"path":"src/core/types.ts","symbol":null},{"path":"test/core.test.ts","symbol":null}]},
    {"name":"m050","index":1,"level":0,"deps":[],"contract":false,
     "claims":[{"path":"src/modules/m050/f10.ts","symbol":null}]}],
    "plan":"3e26eba2bbfa4cb1","width":2}
   ```
   - It **groups A and D** (one node, path-level claims, `symbol: null`).
   - It **drops B** (`src/modules/m002`, a directory) and A's new file `test/convert.test.ts`, with no warning.
   - Nothing is ordered (no deps), so this is a width-2 DAG, not a real DAG. Not applied.
2. **New:**
   - Each graft took ≈1.1–1.3 s including overhead.
   - Path: `/Users/admin/Library/Application Support/graft/roots/9f2c7053633fa0e9d0b927c4070e4b4b/mounts/<name>`.
   - Layer message: `layer node_modules: reused (1b3ea535cb37)`.
3. **Build and test:**
   - All 4 passed, ≈5 s each. The CLI run's reflink build + test median was 10.68 s, under different load.
   - The output (tsc and vitest) shows what happened. On success, though, there is no explicit exit code or duration.
4. **List, status and diff:** all JSON or diff output, and all exactly matching the edits.
5. **Radar:** `path` level on `src/core/types.ts` for a + d, with `symbols: []` and `detail: ""`. Same as the CLI.
6. **Sandbox and isolation:** PASS.
   - Both escapes were denied with `Operation not permitted`.
   - `find` over the graft mounts and `~/Developer/graft-tests` found no `escape*` files.
   - `ok` was present only in a.
7. **Bud and ring:**
   - Reflink: refused with a clear structured `backend_unsupported` and a hint naming nfs.
   - NFS: graft_new 1.81 s, bud 1.95 s, ring 1.84 s, rewind ≈2.0 s. All worked and were verified on disk.
8. **Integrate:**
   - Dry run: ok. No confirm: refused. Confirm without update_checkout: refused (see bugs). Confirm + update_checkout: landed all 4 in 2.94 s.
   - The **test gate did not run**, and the result says so explicitly.
   - Main now:
     ```
     0618e8b chore: integrate graft d
     66036dd chore: integrate graft c
     a66b285 chore: integrate graft b
     a72650b chore: integrate graft a
     2edd69a graft.toml: share node_modules layer
     1156f50 base
     ```
   - Root `npm run build && npm test`: rc=0, `Test Files 2 passed (2)`, `Tests 4 passed (4)`, 3.31 s.
   - The merged `types.ts` is identical to the CLI result.
9. **Prune safety:** all three checks correct (refuse, refuse, succeed). Then a, b, c, d and v-alt were pruned.
10. **Errors:**

    | Case | Code | Clear? | Machine-readable? |
    |---|---|---|---|
    | invalid name | `invalid_name` | yes | yes |
    | missing graft | `not_found` | yes | yes |
    | exit 3 | `failed` | message is generic | **no exit code** |

11. **Cleanup:**
    - `graft_list` → `[]`.
    - `mount | grep graft` → nothing.
    - The mounts directory is empty.
    - Root `git status` shows only `?? .claude/` and `?? .mcp.json`. Both were there before the run (Claude Code session files), so I didn't touch them.
    - **Main is at 0618e8b, not reset. Waiting for your approval to reset to 2edd69a.**

## Confusing, missing, or forced to guess (exact text)

1. **No Root or binary identity in any result.** The server instructions say "workspace (a graft) of this repository", and `graft_list`/`graft_status` return `[]`. I had to use `ps` to know which Root was being served. In runs 1–2, that gap hid a wrong-Root session.
2. **`graft_run` has no exit code**: `{"code":"failed","hint":null,"message":"the command failed","output":"…"}`. The `exit 3` case can't be told apart from `exit 1` or a signal. On success the result is the bare output, or `(… completed with no output)`, with no `exit_code: 0` and no duration.
3. **stdout and stderr are merged** in `output`.
4. **`code:"failed"` is overloaded.** It covers a command exiting non-zero, the integrate checked-out-branch refusal, and the prune would-lose-work refusal. The last two deserve their own codes, for example `checkout_would_fall_behind` and `work_would_be_lost`.
5. **CLI flags in MCP messages:**
   - integrate: `land elsewhere with --onto, or pass --update-checkout to fast-forward Root`
   - prune: `rerun with --force to discard it`

   The confirm messages use MCP wording (`pass confirm: true`), so the two styles are mixed.
6. **`graft_integrate` dry_run said `checked a → main`** for all four. The real run then failed on the checked-out-branch rule. The `dry_run` description says "Run every step except landing", but that check is evidently not one of the steps it runs.
7. **`update_checkout` description**: "Allow landing on the branch Root has checked out (fast-forwards Root's clean working tree)". It says "clean", yet Root had untracked `.claude/` and `.mcp.json` and the landing worked. Does "clean" ignore untracked files? I had to guess.
8. **`graft_ring` description** still says "restore it later with `graft rewind`" (CLI), even though `graft_rewind` now exists.
9. **`graft_ring list`** returns a plain-text table (`before-test  op      2       1 entries  2026-11-15T00:00:21.923Z`), while list and status return JSON.
10. **`graft_new`, `graft_bud`, `graft_ring`, `graft_rewind`, `graft_prune` and `graft_integrate` return prose**, for example `Created graft a at /Users/…/mounts/a (base 2edd69a)`. An agent has to regex the path out of it. `graft_new`'s description says only "Returns where it is."
11. **`graft_plan`**: "The task names files and symbols in `code spans`." It doesn't say that directories and not-yet-existing files are ignored. There's also no `unresolved` field, so B silently disappeared.
12. **`graft_radar`**: `"detail":""` on a path overlap is an empty field that gives no information. It could list the hunk ranges each graft touched.
13. **`graft_bud`/`graft_ring` descriptions** say "virtual graft" without saying that this means `nfs` (and `fuse`?). The refusal hint is clear, so I only learned this after the call failed.
14. **`disk_bytes`** is ≈24.9 MB per reflink graft after a build. It's unclear whether this counts `dist/`, the shared layer or reflinked blocks. The CLI report showed ≈300 KiB before building.

## CLI vs MCP

| | CLI report | MCP (this run) |
|---|---|---|
| Create (reflink, warm layer) | 0.95 s median | ≈1.1–1.3 s incl. ~1 s overhead |
| Build + test per graft | 10.68 s median | ≈5.0 s (different load) |
| Integrate 4 | 8.09 s | 2.94 s (commits made by integrate, `chore: integrate graft x`) |
| Commit messages | `feat a` … (user commits) | `chore: integrate graft a` … (no per-graft message; `message` is a single string for the whole call) |
| Test gate visibility | nothing printed (§7.4) | **explicit note** "no test gate ran" ✅ improvement |
| Radar | path-level a+d | identical, as JSON |
| Sandbox / isolation | pass | pass |
| Prune refusal | text, rc=1 | structured but `code:"failed"`; force now also needs `confirm` ✅ |
| `--update-checkout` | passed up front | first attempt refused. Dry run didn't predict it. |
| Exit codes | `rc=1` visible from the shell | **not reported** ❌ |
| Rewind | CLI | `graft_rewind` works, and the undo hint uses MCP wording ✅ |
| Environment | inherits the user's shell | inherits `.mcp.json` `env`. A wrong config silently changes Root and toolchain (runs 1–2). |

## Bugs

1. **should-fix — `graft_run` drops the exit code.**
   - Repro: `graft_run {name:"e", command:["sh","-c","exit 3"]}`.
   - Result: `{"code":"failed","message":"the command failed",…}`.
   - Expected: `exit_code: 3` (or `signal`), plus separate `stdout`/`stderr`. On success: `exit_code: 0`.
2. **should-fix — integrate dry run doesn't detect the checked-out-branch refusal.**
   - Repro: with Root on `main`, `graft_integrate {names:[a], onto:"main", dry_run:true}` → `checked a → main`.
   - Then `{…, confirm:true}` → `failed … pass --update-checkout`.
   - Expected: the dry run reports the same refusal.
3. **should-fix — `graft_plan` silently drops directory claims and new-file claims.**
   - Repro: a task containing `` `src/modules/m002` `` and `` `test/convert.test.ts` `` (not yet existing).
   - Result: neither appears among the nodes, and there's no warning.
4. **should-fix — the generic `code:"failed"` is used for policy refusals** (integrate checkout rule, prune would-lose-work), so agents can't branch on the code.
5. **nit — CLI flag names in MCP messages** (`--onto`, `--update-checkout`, `--force`), and `graft rewind` in the `graft_ring` description.
6. **nit — `graft_ring list` and the mutation tools return prose or tables instead of JSON.**
7. **nit — no Root/binary identity in the server instructions or in list/status results.**
8. **nit — radar `detail` is an empty string for path overlaps.**

No blockers found.

## Top 5 changes before 0.1.1 (ranked)

1. **`graft_run` result:** `{exit_code, signal, stdout, stderr, duration_ms, timed_out}` on success and failure alike.
2. **Specific error codes for every refusal** (`checkout_would_fall_behind`, `work_would_be_lost`, `command_failed`, …), with `hint` filled in using MCP parameter names (`update_checkout: true`, `force: true`). Remove the CLI flag names from MCP text.
3. **Make `dry_run` run the checkout/landing-target policy check**, so a successful dry run predicts a successful confirm.
4. **JSON results for every tool**, especially `graft_new` (`{name, path, base, backend, layers:[{path, status, id}]}`), bud, ring list, rewind, prune and integrate (`[{graft, status, commit, reason}]`). Include `root` in `graft_list`/`graft_status` and in the server instructions.
5. **`graft_plan`: expand directory claims and accept new-file claims**, or return an `unresolved` array, so no work item disappears silently. Add per-graft commit messages for integrate while you're there.

## Notes on my own actions

- I removed the step-6 probe file `ok` from graft a (`graft_run a rm ok`) before integrating, so it wouldn't land on main. That is a change inside the graft, not a workaround.
- I created a temporary graft `e` for the exit-3 test after a–d had been pruned, then pruned it.
- I wrote the edits through the graft paths that `graft_new` returned, using shell redirection/`sed` from my own Bash tool. I didn't touch Root.
- The Root build/test check wrote `dist/` (gitignored). Root `git status` is unchanged.
