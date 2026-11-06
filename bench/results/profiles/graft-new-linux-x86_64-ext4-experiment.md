# graft new profile: medium fixture

| | |
| --- | --- |
| OS | Linux 7.0.0-1019-azure (x86_64) |
| volume | /mnt/graft-bench/mnt/ext4/profile (ext4) |
| runs | 3 |

| run | graft | step | ms |
| ---: | --- | --- | ---: |
| 1 | cold (default) | plan layers | 0 |
| 1 | cold (default) | check out tree | 618 |
| 1 | cold (default) | pin base | 1 |
| 1 | cold (default) | lock layer | 0 |
| 1 | cold (default) | build layer | 2091 |
| 1 | cold (default) | install layer | 0 |
| 1 | cold (default) | record layer | 0 |
| 1 | cold (default) | clone layer | 383 |
| 1 | cold (default) | exclude layers | 0 |
| 1 | cold (default) | graft new | 3129 |
| 1 | warm (default) | plan layers | 0 |
| 1 | warm (default) | check out tree | 812 |
| 1 | warm (default) | pin base | 1 |
| 1 | warm (default) | lock layer | 0 |
| 1 | warm (default) | record layer | 0 |
| 1 | warm (default) | clone layer | 378 |
| 1 | warm (default) | exclude layers | 0 |
| 1 | warm (default) | graft new | 1399 |
| 1 | cold (sync) | plan layers | 0 |
| 1 | cold (sync) | check out tree | 574 |
| 1 | cold (sync) | pin base | 1 |
| 1 | cold (sync) | lock layer | 0 |
| 1 | cold (sync) | build layer | 1737 |
| 1 | cold (sync) | install layer | 0 |
| 1 | cold (sync) | sync layer | 2590 |
| 1 | cold (sync) | record layer | 0 |
| 1 | cold (sync) | clone layer | 343 |
| 1 | cold (sync) | exclude layers | 0 |
| 1 | cold (sync) | graft new | 5255 |
| 1 | warm (sync) | plan layers | 0 |
| 1 | warm (sync) | check out tree | 506 |
| 1 | warm (sync) | pin base | 1 |
| 1 | warm (sync) | lock layer | 0 |
| 1 | warm (sync) | record layer | 0 |
| 1 | warm (sync) | clone layer | 344 |
| 1 | warm (sync) | exclude layers | 0 |
| 1 | warm (sync) | graft new | 865 |
| 1 | cold (keep) | plan layers | 0 |
| 1 | cold (keep) | check out tree | 500 |
| 1 | cold (keep) | pin base | 1 |
| 1 | cold (keep) | lock layer | 0 |
| 1 | cold (keep) | build layer | 1619 |
| 1 | cold (keep) | clone layer into cache | 342 |
| 1 | cold (keep) | install layer | 0 |
| 1 | cold (keep) | record layer | 0 |
| 1 | cold (keep) | exclude layers | 0 |
| 1 | cold (keep) | graft new | 2481 |
| 1 | warm (keep) | plan layers | 0 |
| 1 | warm (keep) | check out tree | 501 |
| 1 | warm (keep) | pin base | 1 |
| 1 | warm (keep) | lock layer | 0 |
| 1 | warm (keep) | record layer | 0 |
| 1 | warm (keep) | clone layer | 342 |
| 1 | warm (keep) | exclude layers | 0 |
| 1 | warm (keep) | graft new | 858 |
| 1 | worktree | git worktree add | 1374 |
| 1 | worktree | npm ci | 1634 |
| 2 | cold (default) | plan layers | 0 |
| 2 | cold (default) | check out tree | 506 |
| 2 | cold (default) | pin base | 1 |
| 2 | cold (default) | lock layer | 0 |
| 2 | cold (default) | build layer | 1628 |
| 2 | cold (default) | install layer | 0 |
| 2 | cold (default) | record layer | 0 |
| 2 | cold (default) | clone layer | 342 |
| 2 | cold (default) | exclude layers | 0 |
| 2 | cold (default) | graft new | 2494 |
| 2 | warm (default) | plan layers | 0 |
| 2 | warm (default) | check out tree | 502 |
| 2 | warm (default) | pin base | 1 |
| 2 | warm (default) | lock layer | 0 |
| 2 | warm (default) | record layer | 0 |
| 2 | warm (default) | clone layer | 352 |
| 2 | warm (default) | exclude layers | 0 |
| 2 | warm (default) | graft new | 884 |
| 2 | cold (sync) | plan layers | 0 |
| 2 | cold (sync) | check out tree | 498 |
| 2 | cold (sync) | pin base | 1 |
| 2 | cold (sync) | lock layer | 0 |
| 2 | cold (sync) | build layer | 1598 |
| 2 | cold (sync) | install layer | 0 |
| 2 | cold (sync) | sync layer | 1377 |
| 2 | cold (sync) | record layer | 0 |
| 2 | cold (sync) | clone layer | 348 |
| 2 | cold (sync) | exclude layers | 0 |
| 2 | cold (sync) | graft new | 3844 |
| 2 | warm (sync) | plan layers | 0 |
| 2 | warm (sync) | check out tree | 499 |
| 2 | warm (sync) | pin base | 1 |
| 2 | warm (sync) | lock layer | 0 |
| 2 | warm (sync) | record layer | 0 |
| 2 | warm (sync) | clone layer | 340 |
| 2 | warm (sync) | exclude layers | 0 |
| 2 | warm (sync) | graft new | 856 |
| 2 | cold (keep) | plan layers | 0 |
| 2 | cold (keep) | check out tree | 502 |
| 2 | cold (keep) | pin base | 1 |
| 2 | cold (keep) | lock layer | 0 |
| 2 | cold (keep) | build layer | 1601 |
| 2 | cold (keep) | clone layer into cache | 341 |
| 2 | cold (keep) | install layer | 0 |
| 2 | cold (keep) | record layer | 0 |
| 2 | cold (keep) | exclude layers | 0 |
| 2 | cold (keep) | graft new | 2460 |
| 2 | warm (keep) | plan layers | 0 |
| 2 | warm (keep) | check out tree | 508 |
| 2 | warm (keep) | pin base | 1 |
| 2 | warm (keep) | lock layer | 0 |
| 2 | warm (keep) | record layer | 0 |
| 2 | warm (keep) | clone layer | 344 |
| 2 | warm (keep) | exclude layers | 0 |
| 2 | warm (keep) | graft new | 885 |
| 2 | worktree | git worktree add | 1369 |
| 2 | worktree | npm ci | 1564 |
| 3 | cold (default) | plan layers | 0 |
| 3 | cold (default) | check out tree | 503 |
| 3 | cold (default) | pin base | 1 |
| 3 | cold (default) | lock layer | 0 |
| 3 | cold (default) | build layer | 1597 |
| 3 | cold (default) | install layer | 0 |
| 3 | cold (default) | record layer | 0 |
| 3 | cold (default) | clone layer | 345 |
| 3 | cold (default) | exclude layers | 0 |
| 3 | cold (default) | graft new | 2462 |
| 3 | warm (default) | plan layers | 0 |
| 3 | warm (default) | check out tree | 514 |
| 3 | warm (default) | pin base | 1 |
| 3 | warm (default) | lock layer | 0 |
| 3 | warm (default) | record layer | 0 |
| 3 | warm (default) | clone layer | 346 |
| 3 | warm (default) | exclude layers | 0 |
| 3 | warm (default) | graft new | 875 |
| 3 | cold (sync) | plan layers | 0 |
| 3 | cold (sync) | check out tree | 502 |
| 3 | cold (sync) | pin base | 1 |
| 3 | cold (sync) | lock layer | 0 |
| 3 | cold (sync) | build layer | 1609 |
| 3 | cold (sync) | install layer | 0 |
| 3 | cold (sync) | sync layer | 1186 |
| 3 | cold (sync) | record layer | 0 |
| 3 | cold (sync) | clone layer | 340 |
| 3 | cold (sync) | exclude layers | 0 |
| 3 | cold (sync) | graft new | 3648 |
| 3 | warm (sync) | plan layers | 0 |
| 3 | warm (sync) | check out tree | 503 |
| 3 | warm (sync) | pin base | 1 |
| 3 | warm (sync) | lock layer | 0 |
| 3 | warm (sync) | record layer | 0 |
| 3 | warm (sync) | clone layer | 340 |
| 3 | warm (sync) | exclude layers | 0 |
| 3 | warm (sync) | graft new | 856 |
| 3 | cold (keep) | plan layers | 0 |
| 3 | cold (keep) | check out tree | 569 |
| 3 | cold (keep) | pin base | 1 |
| 3 | cold (keep) | lock layer | 0 |
| 3 | cold (keep) | build layer | 1620 |
| 3 | cold (keep) | clone layer into cache | 341 |
| 3 | cold (keep) | install layer | 0 |
| 3 | cold (keep) | record layer | 0 |
| 3 | cold (keep) | exclude layers | 0 |
| 3 | cold (keep) | graft new | 2546 |
| 3 | warm (keep) | plan layers | 0 |
| 3 | warm (keep) | check out tree | 497 |
| 3 | warm (keep) | pin base | 1 |
| 3 | warm (keep) | lock layer | 0 |
| 3 | warm (keep) | record layer | 0 |
| 3 | warm (keep) | clone layer | 341 |
| 3 | warm (keep) | exclude layers | 0 |
| 3 | warm (keep) | graft new | 854 |
| 3 | worktree | git worktree add | 1373 |
| 3 | worktree | npm ci | 1570 |
