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
