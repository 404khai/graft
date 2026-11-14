# Profile: commands inside a reflink graft on macOS

Question: the owner's benchmark report (`graft-vs-worktree-report.md`, §4)
measured commands inside a reflink graft at 5–10× the time of the same
commands in a git worktree on macOS: `git status` 0.18 s vs 0.02 s, `git
diff` 0.22 vs 0.01, `rg` 0.28 vs 0.04, `find` 0.25 vs 0.05, incremental
`tsc` build 22 s vs 5.5 s. On Linux the gap was 2–8%. Is it real?

**Answer: it does not reproduce.** Measured interleaved on the same
machine at the same moment, a reflink graft is within noise of a worktree
for every command, in a scratch `GRAFT_HOME` and in one under
`~/Library/Application Support`. Nothing was changed.

## Machine and method

- Apple M3 (8 cores), 16 GiB, macOS 27.0 (26A428), APFS, 47 GiB free (90%
  used). Load average 12–28 throughout, from other projects on the machine.
- Project: a fresh clone of the report's `bench-app` (3,009 tracked files,
  49 MB `node_modules`), Node v24.16.0, git 2.54.0, ripgrep from Homebrew.
- Workspaces: `git worktree add --detach` + `npm ci`; reflink grafts from
  `graft new` (debug build of this branch; the commands timed are git, rg,
  find and tsc, not graft) with `GRAFT_HOME` in a scratch directory under
  `/private/tmp`, and with `GRAFT_HOME=~/Library/Application
  Support/graft-probe` (same volume, same Spotlight and Time Machine scope
  as the default home). The same one-line edit in `src/core/types.ts` in
  every workspace.
- `scripts/compare-in-workspace.sh`: each round runs a command once in
  every workspace, in a rotating order, after one untimed warm-up; medians
  (min–max) in ms.

## Results

9 rounds, load 17.9:

| command | worktree | reflink graft (scratch home) | Root |
| --- | --- | --- | --- |
| `git status` | 71 (55–324) | 80 (56–108) | 87 (62–106) |
| `git diff` | 53 (45–67) | 60 (51–78) | 57 (48–68) |
| `rg -q format .` | 33 (28–37) | 31 (29–41) | 33 (29–39) |
| `find . -type f \| wc -l` | 69 (55–272) | 67 (58–91) | 77 (62–150) |

9 rounds, load 14.1, with a graft under `~/Library/Application Support`:

| command | worktree | scratch home | Application Support |
| --- | --- | --- | --- |
| `git status` | 78 (61–275) | 74 (61–383) | 75 (59–259) |
| `git diff` | 60 (54–105) | 68 (55–92) | 71 (57–77) |
| `rg -q format .` | 38 (30–70) | 38 (30–63) | 37 (32–48) |
| `find . -type f \| wc -l` | 73 (60–78) | 75 (57–83) | 74 (56–82) |

Straight after creation (Spotlight still importing new files), 5 rounds,
load 14.1:

| command | new worktree | new graft |
| --- | --- | --- |
| `git status` | 70 (66–71) | 70 (62–76) |
| `git diff` | 118 (59–901) | 117 (59–328) |
| `rg -q format .` | 33 (33–40) | 38 (33–43) |
| `find . -type f \| wc -l` | 46 (45–51) | 47 (46–52) |

`npx tsc -p .` (the report's `npm run build`, warm, after the edit), 7
rounds, load 28.6:

| | worktree | graft (Application Support) | graft (scratch home) |
| --- | --- | --- | --- |
| `npx tsc -p .` | 8355 (3149–31691) | 8896 (4356–18647) | 7764 (4905–30120) |

An earlier 3-round run at load 16 gave 3077 (2889–11718) for the worktree
and 3849 (3242–17503) for the graft: the spread of either column covers
the other's median. On this machine `tsc` timings move 3–10× with load, so
only interleaved rounds are comparable.

## Candidates checked

| Candidate | Finding |
| --- | --- |
| Spotlight (`mds`) | Indexing is on for `/`; files in the worktree and in grafts under `~/Library/Application Support` both have Spotlight metadata (`mdls`), so both are indexed alike. No difference measured, including straight after creation. |
| Time Machine | `tmutil isexcluded`: grafts are included, like the worktree. No difference measured. |
| Extended attributes | Both trees carry `com.apple.provenance` on every file (`xattr`); grafts carry no other xattr. |
| APFS compression / clones | `ls -lO`: no `compressed` flag on either. Clones share blocks but read like any file. |
| The graft's `.git` | A plain `.git` directory with `[include]` of Root's config, alternates to Root's objects and an index with a cache tree (ADR 0012). `core.fsmonitor` and `core.untrackedCache` are unset in both. `git status`/`git diff` times match the worktree's. |
| The space in the path | The `Application Support` column matches the scratch-home column. |
| `node_modules` layer clone | `rg` and `find` walk it in both; times match. |

## Why the report saw a gap

The report ran its columns one after another (worktree, then reflink,
then NFS) while load rose from 9 to 28; its `npm test` column moved in the
opposite direction from its build column (graft *faster* than the
worktree), which it flagged itself. With four NFS grafts and their
`tsc` runs active earlier in the session, and swap at 3.5–5 GiB, the
reflink column most likely ran under heavier memory and CPU pressure. The
controlled numbers above show no reflink-specific cost; this is treated as
a measurement artefact.

To rerun: create a worktree and a graft of the same project, make the
same edit in each, then

```
scripts/compare-in-workspace.sh 9 worktree=WT reflink="$(graft path NAME)"
COMPARE_COMMANDS='npx tsc -p .' scripts/compare-in-workspace.sh 7 worktree=WT reflink="$(graft path NAME)"
```
