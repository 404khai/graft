# Profile: the first graft's create time

Source: Profile workflow run 42180527426 (`test/profile-graft-new`),
`scripts/profile-graft-new.sh`, medium fixture, 3 fresh `GRAFT_HOME`s per
runner. Raw tables: `graft-new-<label>.md`. Linux numbers are runs 2 and 3
(run 1 is a warm-up of the runner's disk); macOS ranges are all three and
not reproducible.

| | XFS | ext4 | APFS (VM) |
| --- | ---: | ---: | ---: |
| cold: check out tree | 0.64 s | 0.49 s | 2.0–2.8 s |
| cold: build layer (`npm ci`) | 1.35 s | 1.56 s | 1.6–3.8 s |
| cold: clone layer into the graft | **1.13 s** | 0.50 s | 1.0–1.4 s |
| warm: clone layer into the graft | 0.43 s | 0.48 s | 1.3–1.5 s |
| `graft new`, cold | 3.17 s | 2.57 s | 4.5–7.4 s |
| `graft new`, warm | 1.15 s | 0.99 s | 2.8–3.9 s |
| `git worktree add` | 1.08 s | 1.34 s | 3.1–4.7 s |
| `npm ci` in the worktree | 1.30 s | 1.57 s | 2.0–2.6 s |

## Hot paths

1. **Cloning a just-built layer (XFS).** Reflinking `node_modules` straight
   after `npm ci` wrote it takes 1.13 s; the same clone from the cache later
   takes 0.43 s. FICLONE must write back the files' dirty data before it
   can share their blocks, and does so file by file on one thread. This is
   the whole cold loss on XFS: the build itself costs what `npm ci` costs
   in a worktree, and the checkout is faster than `git worktree add`.
2. **Cloning file by file on one thread (every backend).** The checkout
   (20k files) and the layer clone (10k files) are serial. A warm graft
   (1.15 s on XFS) is still slower to *create* than `git worktree add`
   (1.08 s), though far faster to be ready (the worktree still needs
   `npm ci`).
3. Planning, pinning, locking, recording and excluding are all ≤ 1 ms on
   Linux: no fixed overhead worth cutting in `graft new` itself.

Next: parallel reflink cloning (`perf/parallel-clone`), measured with the
same profile before and after.

## After parallel layer cloning (ADR 0011)

Profile run 42185454638 (`perf/parallel-clone`), raw tables in
`graft-new-<label>-parallel-clone.md`. The XFS runner was slower than in
the first profile (`git worktree add` 1.49 s against 1.08 s), so the ratios
against the worktree in the same run are what to compare.

| medium, runs 2–3 | XFS before → after | ext4 before → after |
| --- | ---: | ---: |
| clone layer, cold | 1.13 → 0.83 s | 0.50 → 0.34 s |
| clone layer, warm | 0.43 → 0.36 s | 0.48 → 0.34 s |
| warm `graft new` ÷ `git worktree add` | 1.07× → 0.90× | 0.74× → 0.62× |
| cold `graft new` ÷ (worktree add + `npm ci`) | 1.33× → 1.10× | 0.91× → 0.83× |

A warm graft now creates faster than a worktree on both filesystems. The
cold graft on XFS still loses by about 10%: cloning the freshly built layer
(0.83 s) still costs more than twice the warm clone (0.36 s), which is the
writeback FICLONE does first. macOS stays too noisy to read (cold clone
0.66–0.82 s, warm 0.65–1.50 s).

## Experiment: how the first graft gets its layer

Profile run 42189514203 (draft PR #17, not merged), raw tables in
`graft-new-<label>-experiment.md`. Three variants in the same run, medium,
runs 2–3:

| | XFS | ext4 |
| --- | ---: | ---: |
| default: clone the built layer into the graft | 0.48 s | 0.34 s |
| `sync`: `fdatasync` the layer on 8 threads first | 1.70 s, then 0.27 s clone | 1.28 s, then 0.34 s clone |
| `keep`: graft keeps the build's output, cache gets the clone | 0.51 s | 0.34 s |
| cold `graft new`, default ÷ (worktree add + `npm ci`) | 1.10× | 0.80× |

Neither variant helps. Flushing costs far more than it saves, and the
clone costs the same in either direction. On this runner, the cold clone
(0.48 s) is twice the warm one (0.23 s); that difference is the cost of
sharing blocks that have not been written back yet, and no ordering
avoids it. The build (1.23 s) equals `npm ci` in a worktree (1.21 s), and
the checkout beats `git worktree add`. What is left of the cold loss on
XFS (≈0.25 s) is that one clone.
