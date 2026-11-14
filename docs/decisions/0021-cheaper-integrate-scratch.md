# 0021: Reusing Integrate's scratch checkout and linking layers lazily
Status: accepted

## Context

The owner's benchmark (`graft-vs-worktree-report.md`, §3) measured `graft
integrate a b c d` at 8.1 s against 0.18 s for four `git merge`s, on a
3,009-file Node project with a 49 MB `node_modules` layer and no
`[integrate]` section in `graft.toml`. Per request, Integrate (ADR 0017)
checked out a fresh scratch reflink tree of the target's head, cloned
every cached layer into it, applied the graft's changes, committed,
pushed the commit into Root, moved the ref, and removed the tree.

Step timings (`GRAFT_LOG=graft::timing=debug`, `scripts/measure-integrate.sh`,
debug build, 4 grafts, summed, median of 3, Apple M3, load 18–24):

| step | ms |
| --- | ---: |
| check out scratch tree | 2387 |
| remove scratch tree | 1159 |
| link layers | 515 |
| land (push + update-ref) | 542 |
| commit | 210 |
| apply changes | 3 |

With no test gate and no resolver, nothing runs in the scratch tree: the
layers are never read, and the tree only exists to apply files and make
a commit.

## Options considered

- A. **Keep one fresh checkout per request, skip layers when nothing will
  run.** Saves the layer clone only (≈ 0.5 s of 4.8 s).
- B. **A. plus reuse the checkout across requests while nothing has run
  in it.** After a request lands in commit mode, the checkout is exactly
  the commit now at the target's head, so the next request on that
  target can start from it. Any request that runs a resolver or a gate,
  stops (conflict, failure, dry run), or lands elsewhere (`branch`/`push`
  mode) gives the checkout up.
- C. **No working tree at all when nothing runs:** build the commit with
  index-only plumbing (`git read-tree`, `update-index`, `write-tree`,
  `commit-tree`). Fastest, but the merge step (ADR 0017: per-file
  three-way merge and Mergiraf) works on files, so this would be a second
  merge path with its own bugs, for a gain over B of one checkout per
  queue run.
- D. **Reuse even after a gate ran**, cleaning the tree with `git clean
  -fdx` and `git reset --hard`. Saves more with gates, but the gate's
  isolation would then depend on clean-up being complete (ignored build
  outputs, caches in the tree, files a gate made read-only), and a gate
  that passes only because of the previous gate's leftovers could let a
  failing commit land.

## Decision

B.

- Layers are linked into the scratch checkout only right before a
  resolver or test gate runs in it.
- A checkout is kept for the next request only when the request landed in
  `commit` mode and neither a resolver nor a gate ran. Before reusing it,
  Integrate checks that its `HEAD` is the target's current head and that
  `git status --porcelain --ignored=matching --untracked-files=all` is
  empty; otherwise it is removed and a fresh one made. The kept checkout
  is removed when the queue run ends, or by `Drop` on an error path.
- Requests with a gate or resolver still get a fresh checkout each, with
  layers, exactly as before.
- `graft integrate` now prints `note: no test gate ran: set [integrate]
  test in graft.toml to test before landing` when any request passed
  without a gate.

## Consequences

- Guarantees are unchanged: the gate runs in a fresh tree per request;
  Root never receives a commit that failed it; conflicts are recorded;
  the target ref still moves by compare-and-swap (`update-ref new old`,
  retried once from the new head), and a reused checkout is only used
  when its `HEAD` equals that head.
- Measured with `scripts/measure-integrate.sh` (4 grafts, debug build):
  see the PR for interleaved before/after numbers. The first request of a
  queue run still pays one checkout and the run pays one removal; the
  remaining per-request cost is the commit and the push into Root.
- Tests: `ungated_queue_runs_share_one_scratch_and_skip_layers`,
  `gated_requests_get_layers_and_a_fresh_scratch_each`,
  `a_dry_run_without_a_gate_says_none_ran` (tests/e2e/integrate.rs), plus
  every existing Integrate test unchanged.
