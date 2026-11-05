# Benchmark results

One directory per machine and filesystem (`<os>-<arch>-<fs>`), written by
`bench/run.sh`; the bench workflow's macOS job splits the matrix into
`-small-medium` and `-large`, and workload runs end in `-workloads`. See
[../README.md](../README.md). Each directory with a `NOTE.md` has caveats
worth reading before its numbers.

| Directory | Contents | Source |
| --- | --- | --- |
| `linux-x86_64-xfs/` | Create matrix, reflink and FUSE | Phase 8 baseline, run 42005356393 |
| `macos-arm64-apfs-small-medium/` | Create matrix, small and medium, reflink and NFS | Phase 8 baseline, run 42005356393 |
| `macos-arm64-apfs-large/` | Create matrix, large (1 and 4 agents), reflink and NFS | Phase 8 baseline, run 42005356393 |
| `linux-x86_64-xfs-workloads/` | In-graft workloads, reflink and FUSE | Phase 8 baseline, run 42005356393 |
| `linux-x86_64-ext4-workloads/` | In-graft workloads, reflink (copy fallback) and FUSE | Phase 8 baseline, run 42005356393 |
| `macos-arm64-apfs-workloads/` | In-graft workloads, reflink and NFS | Phase 8 baseline, run 42005356393 |
| `nfs-backend/` | NFS against reflink on macOS (small-medium and large) | Phase 7, run 41999454838 |

The matrix and workload jobs ran on different runner machines (the CPU is in
each summary's header), so compare methods within a directory, not across
directories. Earlier results (Phase 4) are in git history at 62afd44.
