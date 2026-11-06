# graft new profile: medium fixture

| | |
| --- | --- |
| OS | Linux 7.0.0-1019-azure (x86_64) |
| volume | /mnt/graft-bench/mnt/ext4/profile (ext4) |
| runs | 3 |

| run | graft | step | ms |
| ---: | --- | --- | ---: |
| 1 | cold | plan layers | 0 |
| 1 | cold | check out tree | 627 |
| 1 | cold | pin base | 1 |
| 1 | cold | lock layer | 0 |
| 1 | cold | build layer | 1951 |
| 1 | cold | install layer | 0 |
| 1 | cold | record layer | 0 |
| 1 | cold | clone layer | 677 |
| 1 | cold | exclude layers | 0 |
| 1 | cold | graft new | 3291 |
| 1 | warm | plan layers | 0 |
| 1 | warm | check out tree | 689 |
| 1 | warm | pin base | 1 |
| 1 | warm | lock layer | 0 |
| 1 | warm | record layer | 0 |
| 1 | warm | clone layer | 539 |
| 1 | warm | exclude layers | 0 |
| 1 | warm | graft new | 1248 |
| 1 | worktree | git worktree add | 1758 |
| 1 | worktree | npm ci | 1711 |
| 2 | cold | plan layers | 0 |
| 2 | cold | check out tree | 492 |
| 2 | cold | pin base | 1 |
| 2 | cold | lock layer | 0 |
| 2 | cold | build layer | 1564 |
| 2 | cold | install layer | 0 |
| 2 | cold | record layer | 0 |
| 2 | cold | clone layer | 497 |
| 2 | cold | exclude layers | 0 |
| 2 | cold | graft new | 2573 |
| 2 | warm | plan layers | 0 |
| 2 | warm | check out tree | 495 |
| 2 | warm | pin base | 1 |
| 2 | warm | lock layer | 0 |
| 2 | warm | record layer | 0 |
| 2 | warm | clone layer | 485 |
| 2 | warm | exclude layers | 0 |
| 2 | warm | graft new | 995 |
| 2 | worktree | git worktree add | 1353 |
| 2 | worktree | npm ci | 1586 |
| 3 | cold | plan layers | 0 |
| 3 | cold | check out tree | 493 |
| 3 | cold | pin base | 1 |
| 3 | cold | lock layer | 0 |
| 3 | cold | build layer | 1556 |
| 3 | cold | install layer | 0 |
| 3 | cold | record layer | 0 |
| 3 | cold | clone layer | 500 |
| 3 | cold | exclude layers | 0 |
| 3 | cold | graft new | 2566 |
| 3 | warm | plan layers | 0 |
| 3 | warm | check out tree | 491 |
| 3 | warm | pin base | 1 |
| 3 | warm | lock layer | 0 |
| 3 | warm | record layer | 0 |
| 3 | warm | clone layer | 484 |
| 3 | warm | exclude layers | 0 |
| 3 | warm | graft new | 993 |
| 3 | worktree | git worktree add | 1335 |
| 3 | worktree | npm ci | 1545 |
