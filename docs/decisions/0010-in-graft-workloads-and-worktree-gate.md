# 0010: In-graft workloads and the worktree gate
Status: accepted

## Context

Phase 8 requires a graft to be faster than a `git worktree` not only to
create, but for everything an agent does inside it: build, test, git,
search, walk and write. The create matrix (ADR 0005) measures none of
that. Phase 8 also defines **beats** (lower median, by more than either
side's run-to-run spread; below the measurement floor, not higher) and
asks for a bench job that fails when a graft loses.

Inside a workspace, results are easily distorted by a workload that one
method happens to favour, by warm-up effects (a fresh index, empty caches)
and by state left behind by an earlier workload.

## Options considered

**What to build.** The fixtures are generated Node projects with no build
step.

- A. Use `npm test` as the "build". It reads few files and writes none,
  so it says nothing about a build's read and write pattern.
- B. Add a real bundler (esbuild, webpack). Adds a dependency tarball per
  fixture and measures the bundler more than the filesystem.
- C. A small generated `build.js` that reads every module, writes one
  bundle per directory into `dist/`, and keeps a size-and-mtime signature
  per directory so a rebuild after a one-file edit rewrites one bundle.
  This is the stat, read and write pattern of an incremental compiler,
  with no dependencies.

**How to sample.**

- D. One timed run per workload per workspace. Mostly noise below 1 s.
- E. Per run, a fresh workspace per method (setup not timed); per
  workload, one discarded warm-up sample and then `--samples` timed ones.
  The run's value is the median sample; the reported value is the median
  over runs, and the spread is (max − min) / median over runs (with a
  single run, the range of its samples).

**Where the gate lives.**

- F. Assertions in the harness while it runs. Couples measuring to
  judging, and cannot re-judge committed results.
- G. A separate `graft-bench gate <dir>` that reads `results.csv` and
  `workloads.csv`, writes `gate.md` and exits non-zero on failure.

## Decision

C + E + G.

Workloads, in order (each leaves the tree as it found it):

| name | timed | setup / undo (untimed) |
| --- | --- | --- |
| `build-cold` | `npm run build` then `npm test` | remove `dist/` |
| `rebuild` | `npm run build` | append a line to one module |
| `test` | `npm test` | — |
| `git-status` | `git status --porcelain` | exactly one modified file |
| `git-diff` | `git diff` | as above |
| `git-commit` | `git commit -a` of a one-line change | `git reset --soft HEAD~1` |
| `rg` | `rg --count-matches` over the tree | — |
| `find` | `find` of every file, `node_modules` included | — |
| `ls-R` | `ls -lR` (a stat per file) | — |
| `write-10k-small` | 10,000 × 1 KiB files in 100 directories | remove them |
| `write-1gib` | one 1 GiB file in 1 MiB chunks, then `fsync` | remove it |

The small-file write is not `fsync`ed, like a build tool's output; the
large one is, so a write-back cache cannot report memory speed as disk
speed. Workloads run on the medium fixture by default. `clone` is not run
by default: inside, it is the same plain directory as a worktree.

The gate compares each graft method (`graft`, `graft-fuse`, `graft-nfs`)
with `worktree` on every create-matrix metric (create, ready for the first
and later agents, disk added, prune; per fixture and agent count) and every
workload. Verdicts: **beats**, **ties** (within the noise), **loses**. It
passes only if the method named by `--default` beats on every value and no
graft method loses on any. `bench/run.sh` runs it last and exits with its
status; the bench workflow runs it after uploading results. It never
becomes a required check (AGENTS.md §6).

## Consequences

- Fixtures move to generator v3 (adds `build.js` and `dist/` in
  `.gitignore`); cached fixtures are regenerated once, and tracked-file
  counts grow by one.
- A tie fails the gate for the default backend. On a noisy machine
  (GitHub's macOS VMs, ADR 0005) genuine wins can show as ties; the
  criterion is then measured on a machine that can reproduce it, and the
  PR says which.
- `--default` is `graft` (reflink), today's default backend, until the
  Phase 8 ADR on default backends changes it per platform.
- Each run's workspace is created fresh, so first-touch effects inside a
  graft (cold page cache, lazy reads from the object store) are paid by
  the warm-up sample, not hidden by an earlier workload of another run.
