# Grafts vs worktrees

Default backend's method: `graft`. It must **beat** `worktree` on every create-matrix value (lower, by more than either side's spread; below the floor of 100 ms or 1 MiB, not higher) and must not lose on any workload inside a workspace. No graft method may **lose** anywhere.

Verdict: **fail**

| method | beats | ties | loses |
| --- | ---: | ---: | ---: |
| graft (default) | 1 | 5 | 5 |
| graft-fuse | 0 | 1 | 10 |

## medium

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| build-cold | graft | 641 ms ±1% | 640 ms ±3% | 1.00× | ties ⚠ |
| build-cold | graft-fuse | 4.62 s ±3% | 640 ms ±3% | 7.21× | loses ✗ |
| find | graft | 15 ms ±1% | 15 ms ±1% | 1.00× | beats |
| find | graft-fuse | 102 ms ±2% | 15 ms ±1% | 6.80× | loses ✗ |
| git-commit | graft | 30 ms ±8% | 28 ms ±12% | 1.08× | loses ✗ |
| git-commit | graft-fuse | 57 ms ±5% | 28 ms ±12% | 2.07× | loses ✗ |
| git-diff | graft | 16 ms ±4% | 15 ms ±11% | 1.03× | loses ✗ |
| git-diff | graft-fuse | 22 ms ±29% | 15 ms ±11% | 1.42× | loses ✗ |
| git-status | graft | 35 ms ±5% | 28 ms ±2% | 1.25× | loses ✗ |
| git-status | graft-fuse | 83 ms ±3% | 28 ms ±2% | 2.92× | loses ✗ |
| ls-R | graft | 204 ms ±3% | 204 ms ±1% | 1.00× | ties ⚠ |
| ls-R | graft-fuse | 1.61 s ±1% | 204 ms ±1% | 7.89× | loses ✗ |
| rebuild | graft | 223 ms ±3% | 221 ms ±1% | 1.01× | ties ⚠ |
| rebuild | graft-fuse | 284 ms ±2% | 221 ms ±1% | 1.28× | loses ✗ |
| rg | graft | 60 ms ±1% | 58 ms ±2% | 1.04× | loses ✗ |
| rg | graft-fuse | 1.34 s ±3% | 58 ms ±2% | 23.37× | loses ✗ |
| test | graft | 146 ms ±2% | 144 ms ±5% | 1.01× | ties ⚠ |
| test | graft-fuse | 164 ms ±21% | 144 ms ±5% | 1.14× | ties ⚠ |
| write-10k-small | graft | 309 ms ±2% | 292 ms ±2% | 1.06× | loses ✗ |
| write-10k-small | graft-fuse | 8.22 s ±2% | 292 ms ±2% | 28.16× | loses ✗ |
| write-1gib | graft | 2.59 s ±6% | 2.60 s ±1% | 1.00× | ties ⚠ |
| write-1gib | graft-fuse | 3.37 s ±1% | 2.60 s ±1% | 1.30× | loses ✗ |
