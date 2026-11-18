# Grafts vs worktrees

Default backend's method: `graft`. It must **beat** `worktree` on every create-matrix value (lower, by more than either side's spread; below the floor of 100 ms or 1 MiB, not higher) and must not lose on any workload inside a workspace. No graft method may **lose** anywhere.

Verdict: **fail**

| method | beats | ties | loses |
| --- | ---: | ---: | ---: |
| graft (default) | 49 | 12 | 7 |
| graft-fuse | 45 | 12 | 11 |

## small

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| create x1 | graft | 847 ms ±4% | 164 ms ±6% | 5.18× | loses ✗ |
| ready, first agent x1 | graft | 1.69 s ±4% | 1.62 s ±5% | 1.04× | ties ⚠ |
| disk added x1 | graft | 88.0 KiB ±0% | 6.0 MiB ±0% | 0.01× | beats |
| prune x1 | graft | 63 ms ±3% | 54 ms ±12% | 1.17× | loses ✗ |
| create x4 | graft | 88 ms ±15% | 165 ms ±4% | 0.53× | beats |
| ready, first agent x4 | graft | 1.68 s ±4% | 1.67 s ±14% | 1.01× | ties ⚠ |
| ready, later agents x4 | graft | 881 ms ±10% | 1.68 s ±6% | 0.52× | beats |
| disk added x4 | graft | 374.0 KiB ±0% | 23.9 MiB ±0% | 0.02× | beats |
| prune x4 | graft | 62 ms ±36% | 56 ms ±8% | 1.10× | loses ✗ |
| create x8 | graft | 91 ms ±22% | 167 ms ±5% | 0.54× | beats |
| ready, first agent x8 | graft | 1.61 s ±19% | 1.71 s ±13% | 0.94× | ties ⚠ |
| ready, later agents x8 | graft | 852 ms ±23% | 1.66 s ±3% | 0.51× | beats |
| disk added x8 | graft | 740.0 KiB ±0% | 47.8 MiB ±0% | 0.02× | beats |
| prune x8 | graft | 60 ms ±12% | 54 ms ±2% | 1.10× | loses ✗ |
| create x16 | graft | 83 ms ±4% | 165 ms ±10% | 0.50× | beats |
| ready, first agent x16 | graft | 1.75 s ±19% | 1.60 s ±39% | 1.09× | ties ⚠ |
| ready, later agents x16 | graft | 822 ms ±6% | 1.65 s ±12% | 0.50× | beats |
| disk added x16 | graft | 1.4 MiB ±0% | 95.5 MiB ±0% | 0.01× | beats |
| prune x16 | graft | 56 ms ±10% | 58 ms ±8% | 0.96× | beats |
| create x1 | graft-fuse | 836 ms ±3% | 164 ms ±6% | 5.11× | loses ✗ |
| ready, first agent x1 | graft-fuse | 1.62 s ±3% | 1.62 s ±5% | 1.00× | ties ⚠ |
| disk added x1 | graft-fuse | 72.0 KiB ±0% | 6.0 MiB ±0% | 0.01× | beats |
| prune x1 | graft-fuse | 22 ms ±4% | 54 ms ±12% | 0.41× | beats |
| create x4 | graft-fuse | 42 ms ±15% | 165 ms ±4% | 0.25× | beats |
| ready, first agent x4 | graft-fuse | 1.65 s ±10% | 1.67 s ±14% | 0.99× | ties ⚠ |
| ready, later agents x4 | graft-fuse | 834 ms ±6% | 1.68 s ±6% | 0.50× | beats |
| disk added x4 | graft-fuse | 156.0 KiB ±0% | 23.9 MiB ±0% | 0.01× | beats |
| prune x4 | graft-fuse | 18 ms ±6% | 56 ms ±8% | 0.31× | beats |
| create x8 | graft-fuse | 26 ms ±2% | 167 ms ±5% | 0.16× | beats |
| ready, first agent x8 | graft-fuse | 1.75 s ±11% | 1.71 s ±13% | 1.02× | ties ⚠ |
| ready, later agents x8 | graft-fuse | 808 ms ±9% | 1.66 s ±3% | 0.49× | beats |
| disk added x8 | graft-fuse | 268.0 KiB ±0% | 47.8 MiB ±0% | 0.01× | beats |
| prune x8 | graft-fuse | 15 ms ±9% | 54 ms ±2% | 0.28× | beats |
| create x16 | graft-fuse | 29 ms ±15% | 165 ms ±10% | 0.17× | beats |
| ready, first agent x16 | graft-fuse | 1.68 s ±10% | 1.60 s ±39% | 1.05× | ties ⚠ |
| ready, later agents x16 | graft-fuse | 806 ms ±14% | 1.65 s ±12% | 0.49× | beats |
| disk added x16 | graft-fuse | 496.0 KiB ±0% | 95.5 MiB ±0% | 0.01× | beats |
| prune x16 | graft-fuse | 16 ms ±23% | 58 ms ±8% | 0.28× | beats |

## medium

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| create x1 | graft | 3.92 s ±5% | 2.10 s ±7% | 1.87× | loses ✗ |
| ready, first agent x1 | graft | 4.61 s ±3% | 5.30 s ±9% | 0.87× | beats |
| disk added x1 | graft | 1.7 MiB ±0% | 119.8 MiB ±0% | 0.01× | beats |
| prune x1 | graft | 1.28 s ±2% | 1.62 s ±8% | 0.79× | beats |
| create x4 | graft | 1.62 s ±7% | 2.08 s ±11% | 0.78× | beats |
| ready, first agent x4 | graft | 4.72 s ±4% | 5.71 s ±12% | 0.83× | beats |
| ready, later agents x4 | graft | 2.31 s ±6% | 5.47 s ±6% | 0.42× | beats |
| disk added x4 | graft | 6.8 MiB ±0% | 479.0 MiB ±0% | 0.01× | beats |
| prune x4 | graft | 1.30 s ±1% | 1.51 s ±10% | 0.86× | beats |
| create x8 | graft | 1.42 s ±5% | 2.07 s ±2% | 0.69× | beats |
| ready, first agent x8 | graft | 4.62 s ±13% | 5.47 s ±31% | 0.84× | ties ⚠ |
| ready, later agents x8 | graft | 2.17 s ±5% | 5.24 s ±4% | 0.41× | beats |
| disk added x8 | graft | 13.5 MiB ±0% | 958.1 MiB ±0% | 0.01× | beats |
| prune x8 | graft | 1.34 s ±10% | 1.51 s ±5% | 0.89× | beats |
| create x16 | graft | 1.50 s ±12% | 2.10 s ±1% | 0.71× | beats |
| ready, first agent x16 | graft | 5.04 s ±12% | 5.27 s ±10% | 0.96× | ties ⚠ |
| ready, later agents x16 | graft | 2.31 s ±11% | 5.41 s ±4% | 0.43× | beats |
| disk added x16 | graft | 27.0 MiB ±0% | 1.9 GiB ±0% | 0.01× | beats |
| prune x16 | graft | 1.28 s ±4% | 1.59 s ±1% | 0.80× | beats |
| create x1 | graft-fuse | 4.45 s ±5% | 2.10 s ±7% | 2.12× | loses ✗ |
| ready, first agent x1 | graft-fuse | 5.30 s ±4% | 5.30 s ±9% | 1.00× | ties ⚠ |
| disk added x1 | graft-fuse | 100.0 KiB ±0% | 119.8 MiB ±0% | 0.00× | beats |
| prune x1 | graft-fuse | 23 ms ±24% | 1.62 s ±8% | 0.01× | beats |
| create x4 | graft-fuse | 44 ms ±7% | 2.08 s ±11% | 0.02× | beats |
| ready, first agent x4 | graft-fuse | 5.15 s ±11% | 5.71 s ±12% | 0.90× | ties ⚠ |
| ready, later agents x4 | graft-fuse | 842 ms ±2% | 5.47 s ±6% | 0.15× | beats |
| disk added x4 | graft-fuse | 188.0 KiB ±0% | 479.0 MiB ±0% | 0.00× | beats |
| prune x4 | graft-fuse | 19 ms ±13% | 1.51 s ±10% | 0.01× | beats |
| create x8 | graft-fuse | 34 ms ±23% | 2.07 s ±2% | 0.02× | beats |
| ready, first agent x8 | graft-fuse | 5.31 s ±11% | 5.47 s ±31% | 0.97× | ties ⚠ |
| ready, later agents x8 | graft-fuse | 853 ms ±6% | 5.24 s ±4% | 0.16× | beats |
| disk added x8 | graft-fuse | 300.0 KiB ±0% | 958.1 MiB ±0% | 0.00× | beats |
| prune x8 | graft-fuse | 16 ms ±33% | 1.51 s ±5% | 0.01× | beats |
| create x16 | graft-fuse | 32 ms ±30% | 2.10 s ±1% | 0.02× | beats |
| ready, first agent x16 | graft-fuse | 5.23 s ±25% | 5.27 s ±10% | 0.99× | ties ⚠ |
| ready, later agents x16 | graft-fuse | 811 ms ±12% | 5.41 s ±4% | 0.15× | beats |
| disk added x16 | graft-fuse | 528.0 KiB ±0% | 1.9 GiB ±0% | 0.00× | beats |
| prune x16 | graft-fuse | 17 ms ±19% | 1.59 s ±1% | 0.01× | beats |
| build-cold | graft | 2.34 s ±14% | 2.29 s ±12% | 1.02× | ties ⚠ |
| build-cold | graft-fuse | 8.26 s ±9% | 2.29 s ±12% | 3.61× | loses ✗ |
| find | graft | 27 ms ±13% | 29 ms ±7% | 0.91× | beats |
| find | graft-fuse | 151 ms ±92% | 29 ms ±7% | 5.14× | ties ⚠ |
| git-commit | graft | 43 ms ±19% | 45 ms ±6% | 0.95× | beats |
| git-commit | graft-fuse | 76 ms ±22% | 45 ms ±6% | 1.69× | loses ✗ |
| git-diff | graft | 23 ms ±15% | 23 ms ±3% | 1.01× | loses ✗ |
| git-diff | graft-fuse | 33 ms ±6% | 23 ms ±3% | 1.44× | loses ✗ |
| git-status | graft | 46 ms ±11% | 49 ms ±2% | 0.94× | beats |
| git-status | graft-fuse | 117 ms ±19% | 49 ms ±2% | 2.40× | loses ✗ |
| ls-R | graft | 242 ms ±7% | 252 ms ±15% | 0.96× | ties ⚠ |
| ls-R | graft-fuse | 1.28 s ±8% | 252 ms ±15% | 5.09× | loses ✗ |
| rebuild | graft | 911 ms ±4% | 857 ms ±5% | 1.06× | loses ✗ |
| rebuild | graft-fuse | 1.03 s ±12% | 857 ms ±5% | 1.20× | loses ✗ |
| rg | graft | 76 ms ±17% | 77 ms ±17% | 0.99× | beats |
| rg | graft-fuse | 1.70 s ±2% | 77 ms ±17% | 22.16× | loses ✗ |
| test | graft | 730 ms ±14% | 758 ms ±7% | 0.96× | ties ⚠ |
| test | graft-fuse | 862 ms ±13% | 758 ms ±7% | 1.14× | ties ⚠ |
| write-10k-small | graft | 444 ms ±6% | 460 ms ±12% | 0.96× | ties ⚠ |
| write-10k-small | graft-fuse | 16.40 s ±4% | 460 ms ±12% | 35.63× | loses ✗ |
| write-1gib | graft | 1.65 s ±12% | 1.70 s ±18% | 0.97× | ties ⚠ |
| write-1gib | graft-fuse | 3.65 s ±4% | 1.70 s ±18% | 2.15× | loses ✗ |

## large

| metric | method | graft | worktree | ratio | verdict |
| --- | --- | ---: | ---: | ---: | --- |
| create x1 | graft | 11.09 s ±13% | 22.12 s ±5% | 0.50× | beats |
| ready, first agent x1 | graft | 11.98 s ±12% | 23.65 s ±5% | 0.51× | beats |
| disk added x1 | graft | 16.8 MiB ±0% | 1.2 GiB ±0% | 0.01× | beats |
| prune x1 | graft | 8.63 s ±13% | 10.19 s ±11% | 0.85× | beats |
| create x4 | graft | 10.54 s ±11% | 22.12 s ±8% | 0.48× | beats |
| ready, first agent x4 | graft | 11.08 s ±19% | 23.34 s ±1% | 0.47× | beats |
| ready, later agents x4 | graft | 11.85 s ±9% | 23.78 s ±8% | 0.50× | beats |
| disk added x4 | graft | 67.4 MiB ±0% | 4.6 GiB ±0% | 0.01× | beats |
| prune x4 | graft | 9.29 s ±5% | 11.27 s ±11% | 0.82× | beats |
| create x8 | graft | 11.46 s ±8% | 22.17 s ±1% | 0.52× | beats |
| ready, first agent x8 | graft | 11.45 s ±19% | 23.17 s ±3% | 0.49× | beats |
| ready, later agents x8 | graft | 12.50 s ±8% | 23.80 s ±1% | 0.53× | beats |
| disk added x8 | graft | 134.8 MiB ±0% | 9.3 GiB ±0% | 0.01× | beats |
| prune x8 | graft | 9.26 s ±10% | 12.06 s ±3% | 0.77× | beats |
| create x16 | graft | 11.02 s ±4% | 21.94 s ±5% | 0.50× | beats |
| ready, first agent x16 | graft | 11.82 s ±8% | 23.96 s ±3% | 0.49× | beats |
| ready, later agents x16 | graft | 11.80 s ±4% | 23.38 s ±5% | 0.50× | beats |
| disk added x16 | graft | 270.0 MiB ±759% | 18.6 GiB ±0% | 0.01× | beats |
| prune x16 | graft | 12.49 s ±11% | 13.49 s ±5% | 0.93× | ties ⚠ |
| create x1 | graft-fuse | 19.10 s ±7% | 22.12 s ±5% | 0.86× | beats |
| ready, first agent x1 | graft-fuse | 19.88 s ±7% | 23.65 s ±5% | 0.84× | beats |
| disk added x1 | graft-fuse | 92.0 KiB ±4% | 1.2 GiB ±0% | 0.00× | beats |
| prune x1 | graft-fuse | 22 ms ±8% | 10.19 s ±11% | 0.00× | beats |
| create x4 | graft-fuse | 48 ms ±13% | 22.12 s ±8% | 0.00× | beats |
| ready, first agent x4 | graft-fuse | 21.78 s ±7% | 23.34 s ±1% | 0.93× | ties ⚠ |
| ready, later agents x4 | graft-fuse | 845 ms ±4% | 23.78 s ±8% | 0.04× | beats |
| disk added x4 | graft-fuse | 186.0 KiB ±1% | 4.6 GiB ±0% | 0.00× | beats |
| prune x4 | graft-fuse | 21 ms ±26% | 11.27 s ±11% | 0.00× | beats |
| create x8 | graft-fuse | 34 ms ±8% | 22.17 s ±1% | 0.00× | beats |
| ready, first agent x8 | graft-fuse | 22.07 s ±7% | 23.17 s ±3% | 0.95× | ties ⚠ |
| ready, later agents x8 | graft-fuse | 839 ms ±12% | 23.80 s ±1% | 0.04× | beats |
| disk added x8 | graft-fuse | 296.0 KiB ±1% | 9.3 GiB ±0% | 0.00× | beats |
| prune x8 | graft-fuse | 18 ms ±10% | 12.06 s ±3% | 0.00× | beats |
| create x16 | graft-fuse | 36 ms ±11% | 21.94 s ±5% | 0.00× | beats |
| ready, first agent x16 | graft-fuse | 20.36 s ±2% | 23.96 s ±3% | 0.85× | beats |
| ready, later agents x16 | graft-fuse | 845 ms ±1% | 23.38 s ±5% | 0.04× | beats |
| disk added x16 | graft-fuse | 524.0 KiB ±2% | 18.6 GiB ±0% | 0.00× | beats |
| prune x16 | graft-fuse | 18 ms ±45% | 13.49 s ±5% | 0.00× | beats |
