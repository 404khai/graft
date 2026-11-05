#!/usr/bin/env bash
# Run the whole benchmark and write bench/results/<label>/: the create
# matrix (results.csv, summary.md; AGENTS.md Phase 4), the in-graft
# workloads (workloads.csv, workloads.md; Phase 8) and the gate against
# worktrees (gate.md). Exits non-zero if the gate fails.
#
#   bench/run.sh [--suite all|matrix|workloads] [--agents 1,4,8,16]
#                [--fixtures small,medium,large] [--workload-fixtures medium]
#                [--methods graft,worktree,clone] [--backends reflink,fuse|nfs]
#                [--runs 3] [--samples 5] [--workloads build-cold,test,...]
#                [--default graft] [--no-gate]
#                [--work DIR] [--image-size GIB] [--label NAME]
#                [--machine "description"] [--margin-gib N] [--no-warmup]
#
# --default names the method of the platform's default backend, which the
# gate holds to "beats worktrees everywhere" (ADR 0010).
#
# Disk use is measured as the drop in free space, so the work directory
# should be a dedicated, quiet volume:
#   - macOS, no --work: a sparse APFS disk image is created and attached
#     (no sudo), sized to at most the host's free space minus 4 GiB, and
#     detached and deleted on exit.
#   - Linux, no --work: uses $TMPDIR/graft-bench, with a warning. CI passes a
#     loop-mounted filesystem (see .github/workflows/bench.yml).
# Cells that would not fit are skipped and reported, never attempted.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
work="" image_gib="" label="" machine="" suite=all default=graft gate=1
matrix=() inside=() workload_fixtures=()
while [[ $# -gt 0 ]]; do
  case $1 in
    --work) work=$2; shift 2 ;;
    --image-size) image_gib=${2%[gG]}; shift 2 ;;
    --label) label=$2; shift 2 ;;
    --machine) machine=$2; shift 2 ;;
    --suite) suite=$2; shift 2 ;;
    --default) default=$2; shift 2 ;;
    --no-gate) gate=0; shift ;;
    --workload-fixtures) workload_fixtures=(--fixtures "$2"); shift 2 ;;
    # Shared by both suites.
    --methods|--backends|--runs) matrix+=("$1" "$2"); inside+=("$1" "$2"); shift 2 ;;
    --samples|--workloads) inside+=("$1" "$2"); shift 2 ;;
    --no-warmup) matrix+=("$1"); shift ;;
    -h|--help) sed -n '2,27p' "$0"; exit 0 ;;
    *) matrix+=("$1" "$2"); shift 2 ;;
  esac
done
case $suite in all|matrix|workloads) ;; *) echo "bench/run.sh: --suite must be all, matrix or workloads" >&2; exit 2 ;; esac

cargo build --quiet --release --locked --manifest-path "$root/Cargo.toml" -p graft-cli -p graft-daemon -p graft-bench
graft="$root/target/release/graft"
harness="$root/target/release/graft-bench"

cleanup=()
# "${a[@]+...}" keeps bash 3.2 (macOS) from failing on an empty array under set -u.
trap 'for c in ${cleanup[@]+"${cleanup[@]}"}; do eval "$c"; done' EXIT

if [[ -z $work ]]; then
  if [[ $(uname -s) == Darwin ]]; then
    host_free_gib=$(( $(df -k "${TMPDIR:-/tmp}" | awk 'NR==2 {print $4}') / 1024 / 1024 ))
    cap=$(( host_free_gib - 4 ))
    size=${image_gib:-40}
    (( size > cap )) && size=$cap
    (( size >= 4 )) || { echo "bench/run.sh: only ${host_free_gib} GiB free on the host; need at least 8" >&2; exit 1; }
    scratch=$(mktemp -d)
    img="$scratch/graft-bench.sparseimage"
    mnt="$scratch/volume"
    echo "bench/run.sh: creating a ${size} GiB sparse APFS volume (host has ${host_free_gib} GiB free)" >&2
    hdiutil create -quiet -size "${size}g" -type SPARSE -fs APFS -volname graft-bench "$img"
    mkdir -p "$mnt"
    hdiutil attach -quiet -nobrowse -noverify -mountpoint "$mnt" "$img"
    # Keep Spotlight from indexing the volume while we time it.
    touch "$mnt/.metadata_never_index"
    cleanup+=("hdiutil detach -quiet '$mnt' || hdiutil detach -force -quiet '$mnt'; rm -rf '$scratch'")
    work=$mnt
  else
    work="${TMPDIR:-/tmp}/graft-bench"
    echo "bench/run.sh: warning: no --work given; measuring on $(df -P "${TMPDIR:-/tmp}" | awk 'NR==2 {print $1}'), a shared filesystem. Disk numbers will be noisy." >&2
  fi
fi
mkdir -p "$work"

if [[ -z $label ]]; then
  os=$(uname -s | tr '[:upper:]' '[:lower:]'); [[ $os == darwin ]] && os=macos
  fs=$(GRAFT_HOME="$work/.doctor" "$graft" doctor "$work" | awk '$1 == "filesystem" {print $2; exit}')
  label="$os-$(uname -m)-$fs"
fi
out="$root/bench/results/$label"
echo "bench/run.sh: results go to $out" >&2

if [[ $suite != workloads ]]; then
  "$harness" run --graft "$graft" --work "$work/bench" --out "$out" --machine "$machine" ${matrix[@]+"${matrix[@]}"}
fi
if [[ $suite != matrix ]]; then
  "$harness" workloads --graft "$graft" --work "$work/bench" --out "$out" --machine "$machine" \
    ${workload_fixtures[@]+"${workload_fixtures[@]}"} ${inside[@]+"${inside[@]}"}
fi
if (( gate )); then
  "$harness" gate "$out" --default "$default"
fi
