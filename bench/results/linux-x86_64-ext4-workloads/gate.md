# Grafts vs worktrees

Default backend's method: `graft`. It must **beat** `worktree` on every create-matrix value (lower, by more than either side's spread; below the floor of 100 ms or 1 MiB, not higher) and must not lose on any workload inside a workspace. No graft method may **lose** anywhere.

Verdict: **fail**

| method | beats | ties | loses |
| --- | ---: | ---: | ---: |
| graft (default) | 1 | 6 | 4 |
| graft-fuse | 0 | 0 | 11 |

## medium

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| build-cold | graft | 728 ms ±9% | 715 ms ±4% | 1.02× | ties ⚠ |
| build-cold | graft-fuse | 4.65 s ±4% | 715 ms ±4% | 6.50× | loses ✗ |
| find | graft | 22 ms ±0% | 22 ms ±1% | 1.00× | beats |
| find | graft-fuse | 106 ms ±2% | 22 ms ±1% | 4.77× | loses ✗ |
| git-commit | graft | 32 ms ±9% | 30 ms ±1% | 1.06× | loses ✗ |
| git-commit | graft-fuse | 61 ms ±6% | 30 ms ±1% | 2.01× | loses ✗ |
| git-diff | graft | 18 ms ±10% | 17 ms ±6% | 1.02× | loses ✗ |
| git-diff | graft-fuse | 22 ms ±8% | 17 ms ±6% | 1.25× | loses ✗ |
| git-status | graft | 42 ms ±5% | 36 ms ±5% | 1.18× | loses ✗ |
| git-status | graft-fuse | 86 ms ±2% | 36 ms ±5% | 2.42× | loses ✗ |
| ls-R | graft | 199 ms ±3% | 201 ms ±2% | 0.99× | ties ⚠ |
| ls-R | graft-fuse | 1.62 s ±1% | 201 ms ±2% | 8.06× | loses ✗ |
| rebuild | graft | 250 ms ±9% | 253 ms ±11% | 0.99× | ties ⚠ |
| rebuild | graft-fuse | 304 ms ±7% | 253 ms ±11% | 1.20× | loses ✗ |
| rg | graft | 58 ms ±3% | 57 ms ±3% | 1.01× | loses ✗ |
| rg | graft-fuse | 1.36 s ±2% | 57 ms ±3% | 23.76× | loses ✗ |
| test | graft | 168 ms ±6% | 166 ms ±4% | 1.01× | ties ⚠ |
| test | graft-fuse | 177 ms ±5% | 166 ms ±4% | 1.06× | loses ✗ |
| write-10k-small | graft | 240 ms ±2% | 238 ms ±1% | 1.01× | ties ⚠ |
| write-10k-small | graft-fuse | 6.34 s ±2% | 238 ms ±1% | 26.64× | loses ✗ |
| write-1gib | graft | 2.90 s ±4% | 2.75 s ±11% | 1.05× | ties ⚠ |
| write-1gib | graft-fuse | 4.15 s ±3% | 2.75 s ±11% | 1.51× | loses ✗ |
