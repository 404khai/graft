#!/usr/bin/env bash
# Measure how much new disk one graft costs, for the reflink target in
# AGENTS.md (Phase 2: < 5 MB for a 1 GB working tree on APFS/btrfs).
#
#   scripts/measure-graft-disk.sh <work-dir> [size-mib] [files]
#
# Builds a repository of <files> random files totalling <size-mib> MiB in
# <work-dir>, registers it, creates one graft with $GRAFT_HOME inside
# <work-dir> (same filesystem), and reports:
#   - the DISK column of `graft ls` (unshared blocks, as Graft measures them)
#   - the drop in the filesystem's free space (an independent check)
#   - the wall time of `graft new`
set -euo pipefail

work=${1:?usage: $0 <work-dir> [size-mib] [files]}
size_mib=${2:-1024}
files=${3:-10000}
root=$(cd "$(dirname "$0")/.." && pwd)
graft=${GRAFT_BIN:-$root/target/release/graft}
[[ -x $graft ]] || { echo "build first: cargo build --release -p graft-cli" >&2; exit 1; }

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=bench GIT_AUTHOR_EMAIL=bench@graft.invalid
export GIT_COMMITTER_NAME=bench GIT_COMMITTER_EMAIL=bench@graft.invalid
export GRAFT_HOME="$work/graft-home"

repo="$work/repo"
rm -rf "$repo" "$GRAFT_HOME"
mkdir -p "$repo" "$work"
per_file=$(( size_mib * 1024 * 1024 / files ))
echo "creating $files files of $per_file bytes ($size_mib MiB) in $repo" >&2
python3 - "$repo" "$files" "$per_file" <<'PY'
import os, sys
repo, files, size = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
for i in range(files):
    d = os.path.join(repo, f"d{i // 100:03d}")
    os.makedirs(d, exist_ok=True)
    with open(os.path.join(d, f"f{i:05d}.bin"), "wb") as f:
        f.write(os.urandom(size))
PY
git -C "$repo" init -q -b main
git -C "$repo" add -A
git -C "$repo" -c core.bigFileThreshold=1m commit -q -m "bench tree"
(cd "$repo" && "$graft" init >/dev/null)

free_kib() { df -Pk "$work" | awk 'NR==2 {print $4}'; }
sync
before=$(free_kib)
start=$(python3 -c 'import time; print(time.time())')
(cd "$repo" && "$graft" new bench)
end=$(python3 -c 'import time; print(time.time())')
sync
after=$(free_kib)

disk=$(cd "$repo" && "$graft" ls | awk 'NR==2 {print $5, $6}')
fs=$(cd "$repo" && "$graft" doctor | awk '$1=="filesystem" {print $2; exit}')
printf '\nfilesystem       %s\n' "$fs"
printf 'tree             %s MiB in %s files\n' "$size_mib" "$files"
printf 'graft ls DISK    %s\n' "$disk"
printf 'free space drop  %s KiB\n' "$(( before - after ))"
printf 'graft new time   %.2f s\n' "$(python3 -c "print($end - $start)")"
(cd "$repo" && "$graft" prune bench --force >/dev/null)
