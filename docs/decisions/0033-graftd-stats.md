# 0033: graftd statistics
Status: accepted

## Context

"File access in my graft is slow" was hard to investigate: nothing said
which operation was slow, how often requests failed, or whether the content
cache was doing anything, short of profiling graftd (Phase 8's flamegraphs).
Peak memory against the cache limits was also measured only by the bench
harness.

## Options considered

1. **A metrics crate** (`metrics` plus an exporter, or Prometheus). These are
   standard and come with histograms, but they add dependencies and a
   server or port, which a local daemon has no use for.
2. **Process-wide atomics in graft-fs**, read through a JSON-RPC `stats`
   method that graftd already has the socket for.
3. **Per-graft counters.** These would show which agent is busy, but they
   need a lookup per request or a counter set per mount. Left for later,
   if the global view proves too coarse.

## Decision

Option 2.

- `graft_fs::stats`: for each of 17 operations, a count, errors, total and
  maximum latency, and 24 power-of-two microsecond buckets, all `AtomicU64`
  with relaxed ordering. It also counts content cache hits and misses.
  Instrumentation sits at the adapters only: each FUSE callback, and each
  method of the NFS `SharedFs`. Recording costs two `Instant::now()` calls
  and up to six atomic adds, with no lock and no allocation.
- Percentiles are estimated as the upper bound of the bucket that holds
  them, which is honest to within a factor of two and labelled that way.
  The error count includes expected errors (a `lookup` of a missing name is
  `ENOENT`), and the output says so.
- graftd's `stats` method adds its version, pid, uptime, the number of
  mounts, and resident memory on Linux (`/proc/self/status`; nothing on
  macOS, where reading it would need `unsafe` in a crate that forbids it).
- `graft stats [--json]` never starts graftd; with no daemon it says there
  is nothing to count. `graft_stats` over MCP is the same, and
  `graft doctor --report` includes the stats when graftd is running.
- Counters live as long as graftd and are never persisted or reset.
- No new dependencies.

## Consequences

- A slow-graft report can now say which operation is slow and how slow,
  and whether the cache is effective.
- The counters cover the whole process, not individual grafts or Roots.
- There is no memory figure on macOS.
- The Phase 8 regression gate is unaffected: the overhead is a few tens of
  nanoseconds against microsecond-scale requests. The PR records any
  benchmark run.
