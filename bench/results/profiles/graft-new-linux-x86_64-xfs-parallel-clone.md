# graft new profile: medium fixture

| | |
| --- | --- |
| OS | Linux 7.0.0-1019-azure (x86_64) |
| volume | /mnt/graft-bench/mnt/xfs/profile (xfs) |
| runs | 3 |

| run | graft | step | ms |
| ---: | --- | --- | ---: |
| 1 | cold | plan layers | 0 |
| 1 | cold | check out tree | 2883 |
| 1 | cold | pin base | 1 |
| 1 | cold | lock layer | 0 |
| 1 | cold | build layer | 1981 |
| 1 | cold | install layer | 0 |
| 1 | cold | record layer | 0 |
| 1 | cold | clone layer | 1738 |
| 1 | cold | exclude layers | 0 |
| 1 | cold | graft new | 6861 |
| 1 | warm | plan layers | 0 |
| 1 | warm | check out tree | 1315 |
| 1 | warm | pin base | 1 |
| 1 | warm | lock layer | 0 |
| 1 | warm | record layer | 0 |
| 1 | warm | clone layer | 572 |
| 1 | warm | exclude layers | 0 |
| 1 | warm | graft new | 1923 |
| 1 | worktree | git worktree add | 1488 |
| 1 | worktree | npm ci | 1616 |
| 2 | cold | plan layers | 0 |
| 2 | cold | check out tree | 876 |
| 2 | cold | pin base | 1 |
| 2 | cold | lock layer | 0 |
| 2 | cold | build layer | 1662 |
| 2 | cold | install layer | 0 |
| 2 | cold | record layer | 0 |
| 2 | cold | clone layer | 837 |
| 2 | cold | exclude layers | 0 |
| 2 | cold | graft new | 3388 |
| 2 | warm | plan layers | 0 |
| 2 | warm | check out tree | 935 |
| 2 | warm | pin base | 1 |
| 2 | warm | lock layer | 0 |
| 2 | warm | record layer | 0 |
| 2 | warm | clone layer | 369 |
| 2 | warm | exclude layers | 0 |
| 2 | warm | graft new | 1350 |
| 2 | worktree | git worktree add | 1511 |
| 2 | worktree | npm ci | 1601 |
| 3 | cold | plan layers | 0 |
| 3 | cold | check out tree | 890 |
| 3 | cold | pin base | 1 |
| 3 | cold | lock layer | 0 |
| 3 | cold | build layer | 1667 |
| 3 | cold | install layer | 0 |
| 3 | cold | record layer | 0 |
| 3 | cold | clone layer | 827 |
| 3 | cold | exclude layers | 0 |
| 3 | cold | graft new | 3480 |
| 3 | warm | plan layers | 0 |
| 3 | warm | check out tree | 943 |
| 3 | warm | pin base | 1 |
| 3 | warm | lock layer | 0 |
| 3 | warm | record layer | 0 |
| 3 | warm | clone layer | 360 |
| 3 | warm | exclude layers | 0 |
| 3 | warm | graft new | 1350 |
| 3 | worktree | git worktree add | 1477 |
| 3 | worktree | npm ci | 1668 |
