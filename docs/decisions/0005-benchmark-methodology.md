# 0005: Benchmark methodology
Status: accepted

## Context

Phase 4 compares grafts with `git worktree` and full clones on create time,
disk added, time to first passing test and prune time, for 1–16 agents and
three fixture sizes, reproducibly within ±10%. Each of these numbers can be
measured in ways that flatter one method.

## Options considered

**Disk added**

- A. Sum file sizes or `st_blocks`. This counts reflinked blocks (grafts) and
  hardlinked objects (local clones) as if they were private, overstating
  both.
- B. Per-file unshared size (FIEMAP, `ATTR_CMNEXT_PRIVATESIZE`), as
  `graft ls` uses. Exact for reflinks, but blind to hardlinks, and specific
  to the measuring tool.
- C. The drop in the filesystem's free space with all workspaces present.
  Method-neutral: it counts what the disk actually lost, including Graft's
  layer cache and index, and git's hardlinks. It needs a quiet volume, since
  any other writer on the same filesystem pollutes it.

**Where to measure**

- D. The developer's normal disk. Noisy (Spotlight, browsers, builds), and a
  large matrix could fill it.
- E. A dedicated volume: a sparse APFS disk image on macOS (`hdiutil`, no
  root, capped below the host's free space) and a loop-mounted filesystem on
  Linux. Spotlight indexing is turned off on the image.

**Timing noise**

- F. One sample per cell. On a desktop a single 150 ms sample swings by 2–3×.
- G. Medians over several samples: each cell is repeated until it has at
  least five timed creations, with every workspace pruned and the method
  reset between repetitions (for grafts, `graft gc`, so the first agent of
  every repetition builds the layer cold again). One discarded warm-up cell
  per fixture and method precedes the first run.

## Decision

C + E + G.

- **Fixtures** are deterministic, generated Node projects (≈1k, ≈20k and
  ≈200k tracked files) with vendored tarball dependencies, so every method
  runs a real, offline `npm ci` and `npm test`. Grafts get `node_modules`
  through a layer; worktrees and clones install it per workspace, as they
  would in practice.
- **Create** is the workspace command alone. **Ready** is create + install
  (if the method needs one) + a passing test. **Prune** is removal of one
  workspace. Workspaces are created one after another.
- **Reproducibility** is reported per value as (max − min) / median over
  runs. Values under 100 ms or 1 MiB are shown but not held to ±10%, because
  at that scale jitter dominates. This floor is printed in every summary.
- **Safety**: before each cell the harness estimates its disk need and skips
  the cell, recording why, if it would leave less than a margin (default
  2 GiB) free.

## Consequences

- Results depend on the machine's quietness; each summary records the load
  average at start. Results from GitHub's dedicated runners are the
  reference; desktop runs are indicative.
- Local clones hardlink objects (git's default), so a clone's disk cost is
  mostly its checkout. This is what users get, and the summary says so.
- Large cells (≈200k files × 16 agents) may not fit on small runners or
  laptops; they appear as skipped, never as missing.
