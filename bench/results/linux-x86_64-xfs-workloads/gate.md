# Grafts vs worktrees

Default backend's method: `graft`. It must **beat** `worktree` on every create-matrix value (lower, by more than either side's spread; below the floor of 100 ms or 1 MiB, not higher) and must not lose on any workload inside a workspace. No graft method may **lose** anywhere.

Verdict: **fail**

| method | beats | ties | loses |
| --- | ---: | ---: | ---: |
| graft (default) | 0 | 6 | 5 |
| graft-fuse | 0 | 0 | 11 |

## medium

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| build-cold | graft | 553 ms ±0% | 552 ms ±2% | 1.00× | ties ⚠ |
| build-cold | graft-fuse | 4.01 s ±3% | 552 ms ±2% | 7.27× | loses ✗ |
| find | graft | 13 ms ±5% | 13 ms ±5% | 1.02× | loses ✗ |
| find | graft-fuse | 83 ms ±0% | 13 ms ±5% | 6.60× | loses ✗ |
| git-commit | graft | 26 ms ±11% | 24 ms ±12% | 1.08× | loses ✗ |
| git-commit | graft-fuse | 48 ms ±30% | 24 ms ±12% | 2.03× | loses ✗ |
| git-diff | graft | 14 ms ±15% | 14 ms ±20% | 1.04× | loses ✗ |
| git-diff | graft-fuse | 19 ms ±4% | 14 ms ±20% | 1.42× | loses ✗ |
| git-status | graft | 30 ms ±10% | 24 ms ±3% | 1.25× | loses ✗ |
| git-status | graft-fuse | 66 ms ±4% | 24 ms ±3% | 2.72× | loses ✗ |
| ls-R | graft | 173 ms ±2% | 174 ms ±1% | 1.00× | ties ⚠ |
| ls-R | graft-fuse | 1.30 s ±1% | 174 ms ±1% | 7.46× | loses ✗ |
| rebuild | graft | 188 ms ±3% | 186 ms ±0% | 1.01× | ties ⚠ |
| rebuild | graft-fuse | 234 ms ±1% | 186 ms ±0% | 1.26× | loses ✗ |
| rg | graft | 53 ms ±3% | 52 ms ±3% | 1.02× | loses ✗ |
| rg | graft-fuse | 1.05 s ±1% | 52 ms ±3% | 20.32× | loses ✗ |
| test | graft | 122 ms ±5% | 118 ms ±3% | 1.03× | ties ⚠ |
| test | graft-fuse | 129 ms ±1% | 118 ms ±3% | 1.10× | loses ✗ |
| write-10k-small | graft | 226 ms ±19% | 223 ms ±3% | 1.01× | ties ⚠ |
| write-10k-small | graft-fuse | 8.60 s ±11% | 223 ms ±3% | 38.56× | loses ✗ |
| write-1gib | graft | 4.01 s ±33% | 4.09 s ±3% | 0.98× | ties ⚠ |
| write-1gib | graft-fuse | 4.74 s ±7% | 4.09 s ±3% | 1.16× | loses ✗ |
