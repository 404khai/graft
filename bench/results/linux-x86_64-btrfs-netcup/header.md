# Graft benchmark: grafts vs git worktrees vs full clones

| | |
| --- | --- |
| machine | netcup VPS 1000 G12.5 (4 shared vCPU, 8 GiB), Debian 13, btrfs loop volume; host also runs unrelated Docker services |
| OS | Linux 6.12.113+deb13-amd64 (x86_64) |
| CPU | AMD EPYC-Rome Processor, 4 threads |
| filesystem | btrfs, reflinks supported (dedicated volume) |
| graft | graft 0.1.1 |
| git | git version 2.47.3 |
| node / npm | v20.19.2 / 9.2.0 |
| runs | 3 (after one discarded warm-up cell per fixture and method); cells with fewer than 5 agents repeat until they have 5 timed creations |
| load average at start | 1.95, 1.78, 1.43 |
| command | `/mnt/btrfs/graft/target/release/graft-bench run --graft /mnt/btrfs/graft/target/release/graft --work /mnt/bench/bench --out /mnt/btrfs/graft/bench/results/linux-x86_64-btrfs-netcup --machine netcup VPS 1000 G12.5 (4 shared vCPU, 8 GiB), Debian 13, btrfs loop volume; host also runs unrelated Docker services --backends reflink,fuse --agents 1,4,8,16` |

Each cell creates N workspaces one after another from the same fixture.
**create** is the workspace command alone. **ready** (first agent, and median of the later agents) adds `npm ci --offline` (worktree and clone need one per workspace; grafts reuse a `node_modules` layer that the first graft builds) and a passing `npm test`. **disk added** is the drop in free space on the volume with all N workspaces present, including Graft's layer cache and index. Local clones hardlink git objects, as `git clone` does by default. Values are medians over runs, ± the spread ((max − min) / median).
