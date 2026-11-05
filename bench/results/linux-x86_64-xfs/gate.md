# Grafts vs worktrees

Default backend's method: `graft`. It must **beat** `worktree` on every create-matrix value (lower, by more than either side's spread; below the floor of 100 ms or 1 MiB, not higher) and must not lose on any workload inside a workspace. No graft method may **lose** anywhere.

Verdict: **fail**

| method | beats | ties | loses |
| --- | ---: | ---: | ---: |
| graft (default) | 40 | 3 | 14 |
| graft-fuse | 42 | 1 | 14 |

## small

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| create x1 | graft | 398 ms ±2% | 71 ms ±1% | 5.60× | loses ✗ |
| ready, first agent x1 | graft | 543 ms ±2% | 530 ms ±3% | 1.02× | ties ⚠ |
| disk added x1 | graft | 928.0 KiB ±10% | 7.5 MiB ±0% | 0.12× | beats |
| prune x1 | graft | 39 ms ±5% | 33 ms ±12% | 1.18× | loses ✗ |
| create x4 | graft | 66 ms ±6% | 71 ms ±1% | 0.93× | beats |
| ready, first agent x4 | graft | 543 ms ±3% | 532 ms ±1% | 1.02× | ties ⚠ |
| ready, later agents x4 | graft | 213 ms ±0% | 535 ms ±1% | 0.40× | beats |
| disk added x4 | graft | 3.1 MiB ±1% | 30.9 MiB ±0% | 0.10× | beats |
| prune x4 | graft | 42 ms ±5% | 32 ms ±8% | 1.30× | loses ✗ |
| create x8 | graft | 66 ms ±6% | 71 ms ±1% | 0.93× | beats |
| ready, first agent x8 | graft | 540 ms ±1% | 536 ms ±1% | 1.01× | ties ⚠ |
| ready, later agents x8 | graft | 216 ms ±9% | 532 ms ±3% | 0.41× | beats |
| disk added x8 | graft | 6.0 MiB ±0% | 62.1 MiB ±0% | 0.10× | beats |
| prune x8 | graft | 41 ms ±5% | 32 ms ±25% | 1.27× | loses ✗ |
| create x16 | graft | 68 ms ±2% | 71 ms ±1% | 0.95× | beats |
| ready, first agent x16 | graft | 561 ms ±4% | 526 ms ±3% | 1.07× | loses ✗ |
| ready, later agents x16 | graft | 213 ms ±2% | 529 ms ±2% | 0.40× | beats |
| disk added x16 | graft | 11.7 MiB ±0% | 124.4 MiB ±0% | 0.09× | beats |
| prune x16 | graft | 43 ms ±1% | 34 ms ±17% | 1.25× | loses ✗ |
| create x1 | graft-fuse | 408 ms ±2% | 71 ms ±1% | 5.74× | loses ✗ |
| ready, first agent x1 | graft-fuse | 563 ms ±3% | 530 ms ±3% | 1.06× | loses ✗ |
| disk added x1 | graft-fuse | 648.0 KiB ±0% | 7.5 MiB ±0% | 0.08× | beats |
| prune x1 | graft-fuse | 12 ms ±3% | 33 ms ±12% | 0.36× | beats |
| create x4 | graft-fuse | 28 ms ±18% | 71 ms ±1% | 0.39× | beats |
| ready, first agent x4 | graft-fuse | 576 ms ±6% | 532 ms ±1% | 1.08× | loses ✗ |
| ready, later agents x4 | graft-fuse | 181 ms ±4% | 535 ms ±1% | 0.34× | beats |
| disk added x4 | graft-fuse | 766.0 KiB ±0% | 30.9 MiB ±0% | 0.02× | beats |
| prune x4 | graft-fuse | 8 ms ±23% | 32 ms ±8% | 0.26× | beats |
| create x8 | graft-fuse | 20 ms ±8% | 71 ms ±1% | 0.29× | beats |
| ready, first agent x8 | graft-fuse | 580 ms ±2% | 536 ms ±1% | 1.08× | loses ✗ |
| ready, later agents x8 | graft-fuse | 173 ms ±2% | 532 ms ±3% | 0.33× | beats |
| disk added x8 | graft-fuse | 904.0 KiB ±0% | 62.1 MiB ±0% | 0.01× | beats |
| prune x8 | graft-fuse | 7 ms ±9% | 32 ms ±25% | 0.21× | beats |
| create x16 | graft-fuse | 20 ms ±3% | 71 ms ±1% | 0.28× | beats |
| ready, first agent x16 | graft-fuse | 574 ms ±16% | 526 ms ±3% | 1.09× | ties ⚠ |
| ready, later agents x16 | graft-fuse | 171 ms ±3% | 529 ms ±2% | 0.32× | beats |
| disk added x16 | graft-fuse | 1.2 MiB ±0% | 124.4 MiB ±0% | 0.01× | beats |
| prune x16 | graft-fuse | 6 ms ±35% | 34 ms ±17% | 0.19× | beats |

## medium

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| create x1 | graft | 3.74 s ±3% | 1.21 s ±1% | 3.09× | loses ✗ |
| ready, first agent x1 | graft | 3.90 s ±3% | 2.64 s ±1% | 1.48× | loses ✗ |
| disk added x1 | graft | 61.7 MiB ±0% | 190.2 MiB ±0% | 0.32× | beats |
| prune x1 | graft | 956 ms ±2% | 1.07 s ±2% | 0.89× | beats |
| create x4 | graft | 1.51 s ±3% | 1.21 s ±1% | 1.24× | loses ✗ |
| ready, first agent x4 | graft | 3.85 s ±2% | 2.65 s ±1% | 1.45× | loses ✗ |
| ready, later agents x4 | graft | 1.65 s ±1% | 2.66 s ±2% | 0.62× | beats |
| disk added x4 | graft | 114.5 MiB ±0% | 761.3 MiB ±0% | 0.15× | beats |
| prune x4 | graft | 969 ms ±0% | 1.08 s ±1% | 0.90× | beats |
| create x8 | graft | 1.55 s ±2% | 1.26 s ±3% | 1.23× | loses ✗ |
| ready, first agent x8 | graft | 3.84 s ±1% | 2.66 s ±0% | 1.44× | loses ✗ |
| ready, later agents x8 | graft | 1.71 s ±3% | 2.79 s ±5% | 0.61× | beats |
| disk added x8 | graft | 185.0 MiB ±0% | 1.5 GiB ±0% | 0.12× | beats |
| prune x8 | graft | 993 ms ±2% | 1.11 s ±3% | 0.90× | beats |
| create x16 | graft | 1.52 s ±2% | 1.23 s ±2% | 1.23× | loses ✗ |
| ready, first agent x16 | graft | 4.09 s ±9% | 2.74 s ±6% | 1.49× | loses ✗ |
| ready, later agents x16 | graft | 1.68 s ±2% | 2.73 s ±4% | 0.61× | beats |
| disk added x16 | graft | 326.0 MiB ±0% | 3.0 GiB ±0% | 0.11× | beats |
| prune x16 | graft | 995 ms ±3% | 1.11 s ±2% | 0.90× | beats |
| create x1 | graft-fuse | 2.82 s ±7% | 1.21 s ±1% | 2.33× | loses ✗ |
| ready, first agent x1 | graft-fuse | 2.98 s ±6% | 2.64 s ±1% | 1.13× | loses ✗ |
| disk added x1 | graft-fuse | 44.5 MiB ±0% | 190.2 MiB ±0% | 0.23× | beats |
| prune x1 | graft-fuse | 12 ms ±33% | 1.07 s ±2% | 0.01× | beats |
| create x4 | graft-fuse | 26 ms ±9% | 1.21 s ±1% | 0.02× | beats |
| ready, first agent x4 | graft-fuse | 3.01 s ±4% | 2.65 s ±1% | 1.14× | loses ✗ |
| ready, later agents x4 | graft-fuse | 190 ms ±3% | 2.66 s ±2% | 0.07× | beats |
| disk added x4 | graft-fuse | 44.6 MiB ±0% | 761.3 MiB ±0% | 0.06× | beats |
| prune x4 | graft-fuse | 10 ms ±7% | 1.08 s ±1% | 0.01× | beats |
| create x8 | graft-fuse | 20 ms ±8% | 1.26 s ±3% | 0.02× | beats |
| ready, first agent x8 | graft-fuse | 2.96 s ±3% | 2.66 s ±0% | 1.11× | loses ✗ |
| ready, later agents x8 | graft-fuse | 185 ms ±2% | 2.79 s ±5% | 0.07× | beats |
| disk added x8 | graft-fuse | 44.8 MiB ±0% | 1.5 GiB ±0% | 0.03× | beats |
| prune x8 | graft-fuse | 8 ms ±17% | 1.11 s ±3% | 0.01× | beats |
| create x16 | graft-fuse | 20 ms ±7% | 1.23 s ±2% | 0.02× | beats |
| ready, first agent x16 | graft-fuse | 2.97 s ±3% | 2.74 s ±6% | 1.08× | loses ✗ |
| ready, later agents x16 | graft-fuse | 184 ms ±2% | 2.73 s ±4% | 0.07× | beats |
| disk added x16 | graft-fuse | 45.0 MiB ±0% | 3.0 GiB ±0% | 0.01× | beats |
| prune x16 | graft-fuse | 7 ms ±10% | 1.11 s ±2% | 0.01× | beats |

## large

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| create x1 | graft | 10.29 s ±0% | 12.76 s ±2% | 0.81× | beats |
| ready, first agent x1 | graft | 10.44 s ±1% | 13.31 s ±2% | 0.78× | beats |
| disk added x1 | graft | 123.6 MiB ±0% | 1.4 GiB ±0% | 0.09× | beats |
| prune x1 | graft | 6.12 s ±3% | 6.83 s ±3% | 0.90× | beats |
| create x4 | graft | 10.18 s ±1% | 14.69 s ±1% | 0.69× | beats |
| ready, first agent x4 | graft | 10.56 s ±1% | 13.50 s ±3% | 0.78× | beats |
| ready, later agents x4 | graft | 10.33 s ±0% | 15.50 s ±1% | 0.67× | beats |
| disk added x4 | graft | 494.1 MiB ±0% | 5.7 GiB ±0% | 0.09× | beats |
| prune x4 | graft | 6.08 s ±1% | 6.90 s ±1% | 0.88× | beats |
| create x8 | graft | 10.19 s ±1% | 14.84 s ±3% | 0.69× | beats |
| ready, first agent x8 | graft | 11.00 s ±3% | 13.22 s ±4% | 0.83× | beats |
| ready, later agents x8 | graft | 10.29 s ±2% | 15.41 s ±3% | 0.67× | beats |
| disk added x8 | graft | 987.9 MiB ±0% | 11.3 GiB ±0% | 0.09× | beats |
| prune x8 | graft | 6.11 s ±1% | 6.95 s ±4% | 0.88× | beats |
| create x16 | graft | 10.24 s ±1% | 14.92 s ±1% | 0.69× | beats |
| ready, first agent x16 | graft | 10.85 s ±5% | 13.37 s ±4% | 0.81× | beats |
| ready, later agents x16 | graft | 10.34 s ±1% | 15.50 s ±1% | 0.67× | beats |
| disk added x16 | graft | 1.9 GiB ±0% | 22.7 GiB ±0% | 0.09× | beats |
| prune x16 | graft | 6.10 s ±2% | 7.34 s ±1% | 0.83× | beats |
| create x1 | graft-fuse | 16.53 s ±2% | 12.76 s ±2% | 1.30× | loses ✗ |
| ready, first agent x1 | graft-fuse | 16.69 s ±2% | 13.31 s ±2% | 1.25× | loses ✗ |
| disk added x1 | graft-fuse | 616.0 KiB ±0% | 1.4 GiB ±0% | 0.00× | beats |
| prune x1 | graft-fuse | 12 ms ±7% | 6.83 s ±3% | 0.00× | beats |
| create x4 | graft-fuse | 27 ms ±9% | 14.69 s ±1% | 0.00× | beats |
| ready, first agent x4 | graft-fuse | 16.07 s ±3% | 13.50 s ±3% | 1.19× | loses ✗ |
| ready, later agents x4 | graft-fuse | 178 ms ±1% | 15.50 s ±1% | 0.01× | beats |
| disk added x4 | graft-fuse | 750.0 KiB ±0% | 5.7 GiB ±0% | 0.00× | beats |
| prune x4 | graft-fuse | 9 ms ±3% | 6.90 s ±1% | 0.00× | beats |
| create x8 | graft-fuse | 20 ms ±9% | 14.84 s ±3% | 0.00× | beats |
| ready, first agent x8 | graft-fuse | 15.96 s ±3% | 13.22 s ±4% | 1.21× | loses ✗ |
| ready, later agents x8 | graft-fuse | 172 ms ±2% | 15.41 s ±3% | 0.01× | beats |
| disk added x8 | graft-fuse | 904.0 KiB ±0% | 11.3 GiB ±0% | 0.00× | beats |
| prune x8 | graft-fuse | 8 ms ±6% | 6.95 s ±4% | 0.00× | beats |
| create x16 | graft-fuse | 21 ms ±7% | 14.92 s ±1% | 0.00× | beats |
| ready, first agent x16 | graft-fuse | 15.92 s ±3% | 13.37 s ±4% | 1.19× | loses ✗ |
| ready, later agents x16 | graft-fuse | 174 ms ±3% | 15.50 s ±1% | 0.01× | beats |
| disk added x16 | graft-fuse | 1.2 MiB ±0% | 22.7 GiB ±0% | 0.00× | beats |
| prune x16 | graft-fuse | 7 ms ±9% | 7.34 s ±1% | 0.00× | beats |
