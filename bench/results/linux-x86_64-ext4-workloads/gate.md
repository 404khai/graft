# Grafts vs worktrees

Default backend's method: `graft`. It must **beat** `worktree` on every create-matrix value (lower, by more than either side's spread; below the floor of 100 ms or 1 MiB, not higher) and must not lose on any workload inside a workspace. No graft method may **lose** anywhere.

Verdict: **fail**

| method | beats | ties | loses |
| --- | ---: | ---: | ---: |
| graft (default) | 1 | 4 | 6 |
| graft-fuse | 0 | 0 | 11 |

## medium

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| build-cold | graft | 587 ms ±5% | 580 ms ±4% | 1.01× | ties ⚠ |
| build-cold | graft-fuse | 4.54 s ±0% | 580 ms ±4% | 7.83× | loses ✗ |
| find | graft | 16 ms ±1% | 16 ms ±1% | 1.00× | beats |
| find | graft-fuse | 80 ms ±1% | 16 ms ±1% | 5.06× | loses ✗ |
| git-commit | graft | 24 ms ±1% | 24 ms ±5% | 1.00× | loses ✗ |
| git-commit | graft-fuse | 51 ms ±15% | 24 ms ±5% | 2.10× | loses ✗ |
| git-diff | graft | 13 ms ±11% | 12 ms ±4% | 1.08× | loses ✗ |
| git-diff | graft-fuse | 16 ms ±7% | 12 ms ±4% | 1.32× | loses ✗ |
| git-status | graft | 33 ms ±4% | 28 ms ±5% | 1.18× | loses ✗ |
| git-status | graft-fuse | 76 ms ±6% | 28 ms ±5% | 2.70× | loses ✗ |
| ls-R | graft | 111 ms ±2% | 112 ms ±2% | 0.99× | ties ⚠ |
| ls-R | graft-fuse | 1.61 s ±4% | 112 ms ±2% | 14.39× | loses ✗ |
| rebuild | graft | 210 ms ±2% | 200 ms ±1% | 1.05× | loses ✗ |
| rebuild | graft-fuse | 253 ms ±2% | 200 ms ±1% | 1.27× | loses ✗ |
| rg | graft | 46 ms ±1% | 45 ms ±2% | 1.02× | loses ✗ |
| rg | graft-fuse | 1.32 s ±3% | 45 ms ±2% | 29.36× | loses ✗ |
| test | graft | 142 ms ±2% | 142 ms ±4% | 1.00× | ties ⚠ |
| test | graft-fuse | 155 ms ±2% | 142 ms ±4% | 1.09× | loses ✗ |
| write-10k-small | graft | 109 ms ±1% | 107 ms ±1% | 1.02× | loses ✗ |
| write-10k-small | graft-fuse | 4.89 s ±2% | 107 ms ±1% | 45.78× | loses ✗ |
| write-1gib | graft | 2.93 s ±9% | 2.90 s ±4% | 1.01× | ties ⚠ |
| write-1gib | graft-fuse | 4.02 s ±0% | 2.90 s ±4% | 1.38× | loses ✗ |
