# Grafts vs worktrees

Default backend's method: `graft`. It must **beat** `worktree` on every create-matrix value (lower, by more than either side's spread; below the floor of 100 ms or 1 MiB, not higher) and must not lose on any workload inside a workspace. No graft method may **lose** anywhere.

Verdict: **fail**

| method | beats | ties | loses |
| --- | ---: | ---: | ---: |
| graft (default) | 31 | 12 | 14 |
| graft-fuse | 42 | 2 | 13 |

## small

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| create x1 | graft | 313 ms ±13% | 73 ms ±1% | 4.27× | loses ✗ |
| ready, first agent x1 | graft | 432 ms ±12% | 425 ms ±4% | 1.02× | ties ⚠ |
| disk added x1 | graft | 992.0 KiB ±10% | 7.6 MiB ±0% | 0.13× | beats |
| prune x1 | graft | 48 ms ±202% | 28 ms ±4% | 1.70× | loses ✗ |
| create x4 | graft | 72 ms ±25% | 74 ms ±2% | 0.97× | beats |
| ready, first agent x4 | graft | 476 ms ±15% | 430 ms ±10% | 1.11× | ties ⚠ |
| ready, later agents x4 | graft | 187 ms ±3% | 424 ms ±3% | 0.44× | beats |
| disk added x4 | graft | 3.1 MiB ±0% | 30.9 MiB ±0% | 0.10× | beats |
| prune x4 | graft | 40 ms ±15% | 32 ms ±10% | 1.25× | loses ✗ |
| create x8 | graft | 72 ms ±46% | 74 ms ±2% | 0.98× | beats |
| ready, first agent x8 | graft | 434 ms ±12% | 414 ms ±4% | 1.05× | ties ⚠ |
| ready, later agents x8 | graft | 193 ms ±9% | 414 ms ±6% | 0.47× | beats |
| disk added x8 | graft | 6.0 MiB ±0% | 62.1 MiB ±0% | 0.10× | beats |
| prune x8 | graft | 45 ms ±21% | 32 ms ±26% | 1.41× | loses ✗ |
| create x16 | graft | 123 ms ±47% | 75 ms ±2% | 1.64× | ties ⚠ |
| ready, first agent x16 | graft | 488 ms ±16% | 425 ms ±4% | 1.15× | ties ⚠ |
| ready, later agents x16 | graft | 250 ms ±29% | 425 ms ±5% | 0.59× | beats |
| disk added x16 | graft | 11.8 MiB ±0% | 124.4 MiB ±0% | 0.09× | beats |
| prune x16 | graft | 46 ms ±18% | 32 ms ±29% | 1.41× | loses ✗ |
| create x1 | graft-fuse | 334 ms ±5% | 73 ms ±1% | 4.55× | loses ✗ |
| ready, first agent x1 | graft-fuse | 463 ms ±7% | 425 ms ±4% | 1.09× | loses ✗ |
| disk added x1 | graft-fuse | 648.0 KiB ±0% | 7.6 MiB ±0% | 0.08× | beats |
| prune x1 | graft-fuse | 10 ms ±3% | 28 ms ±4% | 0.35× | beats |
| create x4 | graft-fuse | 26 ms ±18% | 74 ms ±2% | 0.35× | beats |
| ready, first agent x4 | graft-fuse | 452 ms ±3% | 430 ms ±10% | 1.05× | ties ⚠ |
| ready, later agents x4 | graft-fuse | 141 ms ±2% | 424 ms ±3% | 0.33× | beats |
| disk added x4 | graft-fuse | 766.0 KiB ±0% | 30.9 MiB ±0% | 0.02× | beats |
| prune x4 | graft-fuse | 8 ms ±5% | 32 ms ±10% | 0.24× | beats |
| create x8 | graft-fuse | 19 ms ±10% | 74 ms ±2% | 0.26× | beats |
| ready, first agent x8 | graft-fuse | 451 ms ±6% | 414 ms ±4% | 1.09× | loses ✗ |
| ready, later agents x8 | graft-fuse | 141 ms ±14% | 414 ms ±6% | 0.34× | beats |
| disk added x8 | graft-fuse | 936.0 KiB ±0% | 62.1 MiB ±0% | 0.01× | beats |
| prune x8 | graft-fuse | 7 ms ±18% | 32 ms ±26% | 0.21× | beats |
| create x16 | graft-fuse | 19 ms ±13% | 75 ms ±2% | 0.25× | beats |
| ready, first agent x16 | graft-fuse | 469 ms ±23% | 425 ms ±4% | 1.10× | ties ⚠ |
| ready, later agents x16 | graft-fuse | 141 ms ±4% | 425 ms ±5% | 0.33× | beats |
| disk added x16 | graft-fuse | 1.2 MiB ±0% | 124.4 MiB ±0% | 0.01× | beats |
| prune x16 | graft-fuse | 6 ms ±19% | 32 ms ±29% | 0.18× | beats |

## medium

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| create x1 | graft | 4.18 s ±7% | 1.19 s ±1% | 3.51× | loses ✗ |
| ready, first agent x1 | graft | 4.30 s ±6% | 2.41 s ±1% | 1.79× | loses ✗ |
| disk added x1 | graft | 61.7 MiB ±0% | 190.2 MiB ±0% | 0.32× | beats |
| prune x1 | graft | 1.11 s ±9% | 1.13 s ±5% | 0.98× | ties ⚠ |
| create x4 | graft | 1.75 s ±5% | 1.20 s ±2% | 1.46× | loses ✗ |
| ready, first agent x4 | graft | 4.12 s ±5% | 2.49 s ±6% | 1.66× | loses ✗ |
| ready, later agents x4 | graft | 1.86 s ±2% | 2.41 s ±2% | 0.77× | beats |
| disk added x4 | graft | 114.6 MiB ±0% | 761.3 MiB ±0% | 0.15× | beats |
| prune x4 | graft | 1.19 s ±7% | 1.13 s ±3% | 1.05× | ties ⚠ |
| create x8 | graft | 1.80 s ±6% | 1.23 s ±6% | 1.47× | loses ✗ |
| ready, first agent x8 | graft | 4.21 s ±8% | 2.40 s ±10% | 1.76× | loses ✗ |
| ready, later agents x8 | graft | 1.91 s ±5% | 2.61 s ±11% | 0.73× | beats |
| disk added x8 | graft | 185.1 MiB ±0% | 1.5 GiB ±0% | 0.12× | beats |
| prune x8 | graft | 1.16 s ±4% | 1.14 s ±10% | 1.02× | ties ⚠ |
| create x16 | graft | 1.78 s ±1% | 1.29 s ±5% | 1.38× | loses ✗ |
| ready, first agent x16 | graft | 4.46 s ±6% | 2.43 s ±4% | 1.84× | loses ✗ |
| ready, later agents x16 | graft | 1.91 s ±1% | 2.57 s ±9% | 0.74× | beats |
| disk added x16 | graft | 326.1 MiB ±0% | 3.0 GiB ±0% | 0.11× | beats |
| prune x16 | graft | 1.15 s ±2% | 1.12 s ±6% | 1.02× | ties ⚠ |
| create x1 | graft-fuse | 2.82 s ±4% | 1.19 s ±1% | 2.37× | loses ✗ |
| ready, first agent x1 | graft-fuse | 2.96 s ±4% | 2.41 s ±1% | 1.23× | loses ✗ |
| disk added x1 | graft-fuse | 44.5 MiB ±0% | 190.2 MiB ±0% | 0.23× | beats |
| prune x1 | graft-fuse | 11 ms ±16% | 1.13 s ±5% | 0.01× | beats |
| create x4 | graft-fuse | 24 ms ±5% | 1.20 s ±2% | 0.02× | beats |
| ready, first agent x4 | graft-fuse | 2.88 s ±6% | 2.49 s ±6% | 1.16× | loses ✗ |
| ready, later agents x4 | graft-fuse | 154 ms ±2% | 2.41 s ±2% | 0.06× | beats |
| disk added x4 | graft-fuse | 44.6 MiB ±0% | 761.3 MiB ±0% | 0.06× | beats |
| prune x4 | graft-fuse | 8 ms ±14% | 1.13 s ±3% | 0.01× | beats |
| create x8 | graft-fuse | 18 ms ±11% | 1.23 s ±6% | 0.01× | beats |
| ready, first agent x8 | graft-fuse | 2.84 s ±11% | 2.40 s ±10% | 1.19× | loses ✗ |
| ready, later agents x8 | graft-fuse | 148 ms ±1% | 2.61 s ±11% | 0.06× | beats |
| disk added x8 | graft-fuse | 44.8 MiB ±0% | 1.5 GiB ±0% | 0.03× | beats |
| prune x8 | graft-fuse | 8 ms ±9% | 1.14 s ±10% | 0.01× | beats |
| create x16 | graft-fuse | 19 ms ±9% | 1.29 s ±5% | 0.01× | beats |
| ready, first agent x16 | graft-fuse | 3.00 s ±6% | 2.43 s ±4% | 1.24× | loses ✗ |
| ready, later agents x16 | graft-fuse | 152 ms ±2% | 2.57 s ±9% | 0.06× | beats |
| disk added x16 | graft-fuse | 45.0 MiB ±0% | 3.0 GiB ±0% | 0.01× | beats |
| prune x16 | graft-fuse | 6 ms ±2% | 1.12 s ±6% | 0.01× | beats |

## large

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| create x1 | graft | 10.90 s ±1% | 12.58 s ±2% | 0.87× | beats |
| ready, first agent x1 | graft | 11.02 s ±1% | 13.17 s ±3% | 0.84× | beats |
| disk added x1 | graft | 123.7 MiB ±0% | 1.4 GiB ±0% | 0.09× | beats |
| prune x1 | graft | 7.05 s ±4% | 6.76 s ±3% | 1.04× | loses ✗ |
| create x4 | graft | 10.63 s ±1% | 15.11 s ±5% | 0.70× | beats |
| ready, first agent x4 | graft | 11.01 s ±1% | 13.18 s ±0% | 0.84× | beats |
| ready, later agents x4 | graft | 10.72 s ±1% | 15.86 s ±2% | 0.68× | beats |
| disk added x4 | graft | 494.1 MiB ±0% | 5.7 GiB ±0% | 0.09× | beats |
| prune x4 | graft | 6.79 s ±2% | 6.89 s ±2% | 0.99× | ties ⚠ |
| create x8 | graft | 10.39 s ±2% | 14.64 s ±0% | 0.71× | beats |
| ready, first agent x8 | graft | 11.07 s ±1% | 13.24 s ±4% | 0.84× | beats |
| ready, later agents x8 | graft | 10.42 s ±2% | 15.10 s ±2% | 0.69× | beats |
| disk added x8 | graft | 988.0 MiB ±0% | 11.3 GiB ±0% | 0.09× | beats |
| prune x8 | graft | 6.94 s ±1% | 7.11 s ±3% | 0.98× | ties ⚠ |
| create x16 | graft | 10.55 s ±1% | 15.12 s ±3% | 0.70× | beats |
| ready, first agent x16 | graft | 10.81 s ±2% | 13.13 s ±2% | 0.82× | beats |
| ready, later agents x16 | graft | 10.65 s ±0% | 15.56 s ±2% | 0.68× | beats |
| disk added x16 | graft | 1.9 GiB ±0% | 22.7 GiB ±0% | 0.09× | beats |
| prune x16 | graft | 6.97 s ±4% | 7.63 s ±27% | 0.91× | ties ⚠ |
| create x1 | graft-fuse | 17.54 s ±1% | 12.58 s ±2% | 1.39× | loses ✗ |
| ready, first agent x1 | graft-fuse | 17.66 s ±1% | 13.17 s ±3% | 1.34× | loses ✗ |
| disk added x1 | graft-fuse | 640.0 KiB ±1% | 1.4 GiB ±0% | 0.00× | beats |
| prune x1 | graft-fuse | 10 ms ±6% | 6.76 s ±3% | 0.00× | beats |
| create x4 | graft-fuse | 25 ms ±13% | 15.11 s ±5% | 0.00× | beats |
| ready, first agent x4 | graft-fuse | 17.10 s ±1% | 13.18 s ±0% | 1.30× | loses ✗ |
| ready, later agents x4 | graft-fuse | 143 ms ±7% | 15.86 s ±2% | 0.01× | beats |
| disk added x4 | graft-fuse | 750.0 KiB ±1% | 5.7 GiB ±0% | 0.00× | beats |
| prune x4 | graft-fuse | 9 ms ±23% | 6.89 s ±2% | 0.00× | beats |
| create x8 | graft-fuse | 18 ms ±1% | 14.64 s ±0% | 0.00× | beats |
| ready, first agent x8 | graft-fuse | 16.82 s ±2% | 13.24 s ±4% | 1.27× | loses ✗ |
| ready, later agents x8 | graft-fuse | 138 ms ±0% | 15.10 s ±2% | 0.01× | beats |
| disk added x8 | graft-fuse | 872.0 KiB ±0% | 11.3 GiB ±0% | 0.00× | beats |
| prune x8 | graft-fuse | 7 ms ±16% | 7.11 s ±3% | 0.00× | beats |
| create x16 | graft-fuse | 18 ms ±14% | 15.12 s ±3% | 0.00× | beats |
| ready, first agent x16 | graft-fuse | 16.82 s ±1% | 13.13 s ±2% | 1.28× | loses ✗ |
| ready, later agents x16 | graft-fuse | 135 ms ±5% | 15.56 s ±2% | 0.01× | beats |
| disk added x16 | graft-fuse | 1.1 MiB ±0% | 22.7 GiB ±0% | 0.00× | beats |
| prune x16 | graft-fuse | 6 ms ±17% | 7.63 s ±27% | 0.00× | beats |
