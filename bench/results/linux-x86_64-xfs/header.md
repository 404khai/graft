# Graft benchmark: grafts vs git worktrees vs full clones

| | |
| --- | --- |
| machine | GitHub Actions ubuntu-latest runner |
| OS | Linux 7.0.0-1017-azure (x86_64) |
| CPU | AMD EPYC 9V74 80-Core Processor, 4 threads |
| filesystem | xfs, reflinks supported (dedicated volume) |
| graft | graft 0.0.0 |
| git | git version 2.56.0 |
| node / npm | v24.22.1 / 11.19.1 |
| runs | 3 (after one discarded warm-up cell per fixture and method); cells with fewer than 5 agents repeat until they have 5 timed creations |
| load average at start | 2.46, 0.67, 0.23 |
| command | `/home/runner/work/graft/graft/target/release/graft-bench run --graft /home/runner/work/graft/graft/target/release/graft --work /mnt/graft-bench/mnt/xfs/bench --out /home/runner/work/graft/graft/bench/results/linux-x86_64-xfs --machine GitHub Actions ubuntu-latest runner --runs 3 --agents 1,4,8,16` |

Each cell creates N workspaces one after another from the same fixture.
**create** is the workspace command alone. **ready** (first agent, and median of the later agents) adds `npm ci --offline` (worktree and clone need one per workspace; grafts reuse a `node_modules` layer that the first graft builds) and a passing `npm test`. **disk added** is the drop in free space on the volume with all N workspaces present, including Graft's layer cache and index. Local clones hardlink git objects, as `git clone` does by default. Values are medians over runs, ± the spread ((max − min) / median).
