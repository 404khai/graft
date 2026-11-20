# Graft vs git worktrees — benchmark report

Date: 2026-11-14. All timings are `/usr/bin/time -p` wall-clock (`real`) seconds. Each one was taken 3 times or more and is shown as **median (min–max)**. Disk is the drop in free space from `df -k`.

> **Read this caveat first.** The machine was busy and almost out of disk the whole time. The load average went from 5.25 at the start to 28.25 at the end. Other user processes were running, and the disk was 99% full with about 5 GB free. Timings that involve `tsc` therefore vary by up to 5× between runs (for example, Root build 9.22–33.69 s). Read the medians as rough figures. A difference smaller than about 2× in any build/test number is within the noise.

---

## 1. Environment

| | |
|---|---|
| OS | macOS (Darwin 27.0.0, `xnu-13432.1.9~1/RELEASE_ARM64_T8122`), arm64 |
| CPU | Apple M3, 8 cores |
| RAM | 16 GiB (17179869184 bytes); swap 4–5 GiB in use during the run |
| Filesystem | APFS (`/dev/disk3s1` on `/System/Volumes/Data`), 482797652 KB total, **5492176 KB free (99% used)** at start |
| `graft --version` | `graft 0.1.0` |
| `git --version` | `git version 2.54.0` |
| `node --version` | `v24.16.0` (npm 11.13.0) |
| hyperfine | not installed, so `/usr/bin/time -p` was used |
| Load average | start `5.25 8.58 9.89`; at `graft` in-workspace runs `9.08 8.81 13.49`; end `28.25 12.28 7.04` |

`graft doctor` (verbatim):
```
graft 0.1.0

platform      macos aarch64
repository    /Users/admin/Developer/graft-tests/bench-app
  filesystem  apfs
  reflink     supported (clonefile)
graft home    /Users/admin/Library/Application Support/graft (default, exists)
  filesystem  apfs
  same fs     yes
config        /Users/admin/Library/Application Support/graft/config.toml (not present, using defaults)
fuse          unavailable (macFUSE not installed; not needed, Graft uses NFS loopback on macOS)
nfs client    available (/sbin/mount_nfs)
sandbox       available (seatbelt)
```
`graft init` → `Registered Root 72365a19198d4296c93478bb26ea58ce for /Users/admin/Developer/graft-tests/bench-app`
`graft trust` → `Approved node_modules: build \`npm ci\``

## 2. Project (`bench-app`)

- Node + TS library: 150 modules × 20 files (`src/modules/m001/f01.ts` … `m150/f20.ts`), 3 functions per file. Each `fNN` imports `add_…` from `f(NN-1)`, and each `f01` imports `format`/`Money` from `src/core/types.ts`. `src/index.ts` re-exports m001 and core. `test/core.test.ts` has 3 tests. TypeScript 5.x, vitest 2.x, @types/node 22.x.
- Commits: `1156f50 base`, `2edd69a graft.toml: share node_modules layer`. Every run in both methods started from `2edd69a`.
- **Tracked files:** 3008 at `base` (3009 with `graft.toml`)
- **`du -sh node_modules`:** 49M (94 packages in the lockfile). `graft gc` later reported the layer as 58.9 MiB.
- **Root build** (`npm run build`, clean `dist` each time): **10.43 (9.22–33.69)**
- **Root test** (`npm test`): **1.04 (0.96–2.79)** — 3 tests passed

Method 2 used a separate clone, `bench-graft`, made from `bench-app` at `2edd69a` before Part 2 started. It got its own `graft init` (Root `9f2c7053…`) and `graft trust`.

## 3. Lifecycle: worktree vs Graft vs Graft-virtual (NFS)

Method: 3 full rounds per method. Each round created A–D one after another, applied the identical scripted edit, ran build+test, committed, landed in order A→D, verified that the merged `main` builds and passes tests, then removed everything and reset `main` to `2edd69a`. Per-workspace numbers are n = 12 (4 workspaces × 3 rounds). Per-round numbers are n = 3.

| Metric | git worktree | Graft (reflink) | Graft virtual (`--backend nfs`) |
|---|---|---|---|
| Create, per workspace | `worktree add`: **1.02 (0.31–4.25)** | `graft new` (warm layer): **0.95 (0.62–1.63)** | `graft new --backend nfs`: **0.07 (0.05–0.57)** |
| Dependency install, per workspace | `npm ci`: **2.83 (1.30–5.75)** | 0 (shared layer, reused) | 0 (shared layer, reused) |
| Cold first create (layer built once) | n/a | 3.84 (n=1; runs `npm ci` to build the layer) | n/a (layer reused) |
| Build, per workspace | — | `graft run … npm run build`: 7.00 (4.52–32.65) | 53.06 (19.08–138.53) |
| Test, per workspace | — | `graft run … npm test`: 3.65 (2.94–7.87) | 4.88 (1.40–11.74) |
| Build + test, per workspace | **12.86 (5.39–42.70)** | **10.68 (7.48–36.71)** | **59.49 (20.48–145.20)** |
| **Ready** (create → first passing test), per workspace | add + ci + build+test: **16.82 (7.76–48.15)** | new + build + test: **11.52 (8.89–37.96)** | **59.56 (20.53–145.77)** |
| **Disk added, all 4 live** (clean pass, see note) | **257.5 MB (257.2–258.1)** | **9.1 MB (3.8–10.4)** | **0.75 MB (0.13–3.22)** |
| Landing, all 4 | `git merge` ×4: **0.18 (0.18–0.31)** (each 0.05, 0.03–0.10) | `graft integrate a b c d`: **8.09 (7.39–9.81)** | **6.52 (6.38–9.51)** |
| Removal, all 4 | `git worktree remove --force` ×4: **2.97 (1.57–5.17)** (each 0.77, 0.36–1.58) | `graft prune` ×4: **2.52 (1.46–2.86)** (each 0.58, 0.25–1.17) | **2.02 (1.86–3.64)** (each 0.24, 0.12–2.70) |
| Commit inside workspace | not timed separately | 0.10 (0.06–0.30) | 0.88 (0.26–4.34) |

**Disk methodology note.** Inside the full rounds, the `df` deltas were unusable:
- Worktree: 301968 / 267876 / 269340 KB
- Graft: 7228 / 21028 / 97028 KB
- NFS: **2058792 / −1142064 / −1102356 KB**

Swap grew and shrank on the same APFS container (it went from 5120 M to 4096 M total), and APFS frees deleted blocks lazily. A first disk-only pass, with a 3 s settle, still swung widely: graft 161300 / 78660 / −1048 KB. The table uses a second disk-only pass:
- Create the 4 workspaces (worktrees include `npm ci`), with no build.
- Wait **30 s after the previous cleanup** before reading `df`.
- Swap total was 4096 M before and after every rep.

Idle drift over 12 s was 1036 / −11284 / 940 KB.

All three methods passed every build and test (rc=0, all 36 workspace runs). Every merged `main` built and passed `Tests 6 passed (6)`.

## 4. In-workspace operations

One workspace of each kind, with node_modules present and a warm `dist`. `git status` and `git diff` were run after a one-line edit. Incremental build means a one-line edit followed by `npm run build`. There is no `tsc --incremental`, so this is a full re-typecheck with a warm cache. n = 3. The medians were taken under load averages of 9–13, which is noisy.

| Operation | worktree | Graft reflink | Graft NFS |
|---|---|---|---|
| `git status` | **0.02** (0.02–0.02) | 0.18 (0.14–0.27) | 0.08 (0.06–0.21) |
| `git diff` | **0.01** (0.01–0.01) | 0.22 (0.21–0.24) | 0.04 (0.04–0.05) |
| `rg format .` | **0.04** (0.03–0.07) | 0.28 (0.24–0.64) | 1.59 (0.51–3.55) |
| `find . -type f \| wc -l` | **0.05** (0.05–0.05) | 0.25 (0.21–0.46) | 0.52 (0.12–2.51) |
| file count from `find` | 9988 | 9994 | 9994 |
| incremental build, run directly | **5.46** (4.57–6.34) | 22.08 (20.60–22.39) | 139.24 (116.43–157.91) |
| incremental build, `graft run` | — | 18.33 (15.96–23.96) | 39.60 (31.61–63.81) |
| `npm test`, run directly | 5.58 (5.33–6.00) | 2.82 (1.84–3.58) | **0.74** (0.69–0.77) |
| `npm test`, `graft run` | — | 7.25 (2.40–8.06) | 0.75 (0.71–1.66) |

These were run one after another (worktree, then reflink, then NFS), so load changed between columns. The `npm test` column moves in the opposite direction from the build column, which shows how much load shifted during the run. The reflink and worktree numbers come from the same kind of on-disk APFS tree, so treat the 4× gap in incremental build as unconfirmed. The NFS build gap (≈ 25× worse direct, ≈ 7× worse via `graft run`) showed up in every measurement, including all 12 lifecycle builds. I consider it real.

## 5. Landing

**Conflicts.** None in either method, in any round. A (adds `convert` after `format`) and D (changes the body of `format`, and the expectations in `test/core.test.ts`) both touch `src/core/types.ts`. Their hunks are separated by an unchanged line (`}`), and A put its test in a new file, `test/convert.test.ts`. So:
- git printed `Auto-merging src/core/types.ts` on `git merge feat-d`, with exit code 0.
- `graft integrate` printed `landed d → main …`, with exit code 0.

Nothing had to be resolved. I did not change the edits to force a conflict. The merged `types.ts` was identical under both methods:
```ts
export function format(m: Money): string {
  return `${m.currency} ${m.amount.toFixed(2)}`;
}

export function convert(m: Money, rate: number): Money {
  return { amount: m.amount * rate, currency: m.currency };
}
```
After landing, `npm run build` passed and `npm test` printed `Test Files 4 passed (4)`, `Tests 6 passed (6)` for every method and round.

**What `graft integrate` printed** (round 1, verbatim):
```
landed   a → main f2cfbeb17cad
landed   b → main 0b3a565bf79a
landed   c → main 6a632b67c1f2
landed   d → main 0570b01141ea
```
`graft queue` afterwards:
```
  ID  STATE     GRAFT                TARGET               COMMIT       WHEN
   1  landed    a                    main                 f2cfbeb17cad 2026-11-14T00:14:53.419Z
   2  landed    b                    main                 0b3a565bf79a 2026-11-14T00:14:55.230Z
   3  landed    c                    main                 6a632b67c1f2 2026-11-14T00:14:57.676Z
   4  landed    d                    main                 0570b01141ea 2026-11-14T00:15:00.311Z
```
`graft ls` then showed all four as `integrated`. `--update-checkout` fast-forwarded Root's checked-out `main`, and Root's `git status` was clean.

**Final `git log --oneline -6` of main** (round 3 of each):

git worktree (merge commits, non-linear):
```
be87a72 Merge branch 'feat-d'
4bd7267 Merge branch 'feat-c'
a6f378e feat d
22b0a97 Merge branch 'feat-b'
0e53984 feat c
2809bd1 feat b
```
Graft reflink (linear, one commit per graft):
```
e214344 feat d
80277b8 feat c
efab730 feat b
791eb12 feat a
2edd69a graft.toml: share node_modules layer
1156f50 base
```
Graft NFS:
```
18b2ecf feat d
e3a920d feat c
2dd886a feat b
f5a4cd4 feat a
2edd69a graft.toml: share node_modules layer
1156f50 base
```
(After each round, both repos' `main` was reset to `2edd69a`, so the final repo state is the base.)

## 6. Radar, sandbox and isolation (verbatim)

**`graft radar`** with A–D all live. The output was identical in all 6 rounds (3 reflink, 3 NFS):
```
LEVEL      GRAFTS                         PATH                           DETAIL
path       a + d                          src/core/types.ts
note: behaviour checks are off: graft.toml has no [radar] test
```
`graft radar --check` gave the same output, because no `[radar] test` is configured. Radar flagged A + D, but **at the file (`path`) level, not on the `format` symbol**. That is accurate for these edits, since A never touched `format`. It did not flag B or C, and they really were independent. With only a, b and v live (non-overlapping), it printed `No overlaps between live grafts.`

**Isolation — PASS.** I wrote `iso-probe.txt` in graft `a`:
```
.../mounts/a/iso-probe.txt                                   (exists)
ls: /Users/admin/Developer/graft-tests/bench-graft/iso-probe.txt: No such file or directory
ls: .../mounts/b/iso-probe.txt: No such file or directory
```

**Sandbox — PASS.**
```
$ graft run a -- sh -c 'touch ../escape'
touch: ../escape: Operation not permitted
rc=1          (../escape does not exist afterwards)
$ graft run a -- sh -c 'touch ok'
rc=0          (.../mounts/a/ok exists)
$ graft run a -- sh -c 'touch /Users/admin/Developer/graft-tests/bench-graft/escape2'   # extra: write into Root
touch: /Users/admin/Developer/graft-tests/bench-graft/escape2: Operation not permitted
rc=1
```

**`graft status a`** — PASS:
```
graft a (live) at /Users/admin/Library/Application Support/graft/roots/9f2c7053633fa0e9d0b927c4070e4b4b/mounts/a, base 2edd69a
  A iso-probe.txt
  A ok
```
**`graft diff a`** — PASS (the empty file `ok` has no hunk):
```
diff --git a/iso-probe.txt b/iso-probe.txt
new file
--- /dev/null
+++ b/iso-probe.txt
@@ -1,0 +1,1 @@
+hi
diff --git a/ok b/ok
new file
--- /dev/null
+++ b/ok
```
**`graft ls`** — PASS:
```
NAME  BASE     STATE  CHANGED       DISK
a     2edd69a  live         2  308.0 KiB
v     2edd69a  live         0  288.9 KiB
b     2edd69a  live         0  288.0 KiB
```
During the lifecycle rounds, `graft ls` showed CHANGED = 2/3/2/2 for a/b/c/d. That matches the edits: B touched 3 files.

**Prune safety (extra)** — `graft prune a` with uncommitted work:
```
graft: graft a has work that pruning would lose:
  uncommitted changes: iso-probe.txt, ok
Commit it and integrate it into Root, or rerun with --force to discard it.
rc=1
```

## 7. Errors, confusing, slow or broken

**Graft:**
1. **NFS virtual backend is much slower for `tsc`.** Lifecycle builds took 53.06 s median (19.08–138.53), against 7.00 s on reflink. A direct incremental build in the NFS mount took 139.24 s, against 22.08 s on reflink and 5.46 s in a worktree. Commits inside NFS grafts were also slower: 0.88 s against 0.10 s. All of these runs exited 0.
2. **Disk use isn't shown during NFS runs.** With 4 NFS grafts building at once, free space dropped by 2058792 KB (≈ 2 GB) in round 1, while `du` of the graft home was only 135 MB. My reading is that swap and page-cache pressure from graftd plus 4 tsc runs caused it (swap: 3478 MB used of 5120 MB). This is an inference; I did not prove it.
3. `graft radar` reports overlap only at the `path` level here. The task said to expect a flag on `format`, but radar said `src/core/types.ts`. That is not wrong (A and D edit different symbols), but it isn't what the task described. `--check` needs a `[radar] test` in `graft.toml`, which this setup doesn't have.
4. `graft integrate` printed nothing about tests ("merge, test, commit" in `--help`). I saw no evidence that it ran any test without an `[integrate]` config.
5. On a cold layer, `graft new` prints the full `npm ci` output, including `6 vulnerabilities (3 moderate, 1 high, 2 critical)`, before `Created graft a …`.
6. Graft working trees live at `~/Library/Application Support/graft/roots/<id>/mounts/<name>`. The path contains a space, so scripts must quote `$(graft path x)`.
7. Running `npm test` through `graft run` took 7.25 s against 2.82 s direct in the same reflink graft. That's possible sandbox overhead, but it's within the load noise.

**git worktree:**
8. Each worktree needs its own `npm ci` (2.83 s median, plus ≈ 64 MB each).
9. The very first `git worktree remove --force` took 11.36 s, followed by 4.58, 3.57 and 2.85 s. That was a trial round, R0. Later rounds took 0.36–1.58 s.

**My own harness errors (not tool faults, but recorded):**
10. My first two timing attempts recorded empty values. `2>/dev/null` / `2>&1` also swallowed `/usr/bin/time`'s own output. I fixed this with `/usr/bin/time -p -o file`. The trial worktree round R0 (rc=0 for everything, disk 274588 KB) was discarded as a timing sample, and its merges (0.04/0.07/0.11/0.05 s) and removals (above) are not in the medians.
11. The first in-workspace run hung on `rg format`. With no path and a non-TTY stdin, ripgrep reads stdin. I killed it and re-ran with `rg format .` and `</dev/null`.
12. The workspaces were created one after another, A then B then C then D, not truly in parallel. That is the same for both methods.

## 8. Observations (numbers only)

- **Disk for 4 workspaces:** worktree 257.5 MB, Graft reflink 9.1 MB (**28× less**), Graft NFS 0.75 MB (**343× less**).
- **Create:** worktree add 1.02 s against graft new 0.95 s (about equal). NFS took 0.07 s (**15× faster**).
- **Dependency install:** worktree needs 2.83 s of `npm ci` per workspace (11.3 s for 4). Graft needs 0 s per workspace after a one-time 3.84 s layer build.
- **Ready (create → first passing test):** worktree 16.82 s, Graft reflink **11.52 s (−5.30 s, −32%)**, Graft NFS 59.56 s (**+42.74 s, 3.5× slower**).
- **Landing 4:** `git merge` 0.18 s against `graft integrate` 8.09 s (**+7.91 s, 45× slower**) and NFS 6.52 s. In exchange, integrate produced a linear history, a queue log, and updated graft state. git produced 3–4 merge commits.
- **Removal 4:** worktree 2.97 s, Graft 2.52 s (−0.45 s), NFS 2.02 s (−0.95 s).
- **In-workspace git/search ops:** worktree was 5–22× faster than reflink grafts. `git status` took 0.02 against 0.18, `git diff` 0.01 against 0.22, `rg` 0.04 against 0.28, and `find` 0.05 against 0.25. All of these are under 0.3 s in absolute terms.
- **Incremental build:** worktree 5.46 s, reflink graft 22.08 s direct / 18.33 s via `graft run`, NFS 139.24 s / 39.60 s. The reflink gap is unconfirmed because of load, see §4.
- **Conflict detection:** both methods hit 0 textual conflicts. Graft radar flagged the A/D file overlap before landing. git gave no warning before merge-time `Auto-merging`.
- **Safety:** Graft's sandbox blocked writes outside the graft (2/2), its isolation held (2/2), and `prune` refused to lose uncommitted work. Worktrees have no equivalent.

## 9. Cleanup

All workspaces were removed. Steps: `graft prune a --force` (after the refusal above), `graft prune b`, `graft prune v`, `graft gc` (`Removed 1 layer(s), freeing 58.9 MiB. Kept 0 in use.`) and `graft daemon stop` (`Stopped graftd`). Then `git worktree remove --force ../wt/a`, `git branch -D feat-a` (`Deleted branch feat-a (was 2edd69a).`) and `rmdir ../wt`. No graft NFS mounts remain (`mount | grep -c graft` → `0`).

`bench-app`:
```
$ git worktree list
/Users/admin/Developer/graft-tests/bench-app 2edd69a [main]
$ git status
On branch main
nothing to commit, working tree clean
$ git branch
* main
```
`bench-graft`:
```
$ graft ls
No grafts. Create one with `graft new <name>`.
$ git status
On branch main
Your branch is up to date with 'origin/main'.

nothing to commit, working tree clean
$ git worktree list
/Users/admin/Developer/graft-tests/bench-graft 2edd69a [main]
```
Free space at the end: 4613412 KB.
