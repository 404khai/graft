#!/usr/bin/env bash
# Profile `graft new` step by step against `git worktree add` + `npm ci` on
# the same volume (Phase 8: measure first). Prints a Markdown report.
#
#   scripts/profile-graft-new.sh WORK_DIR [FIXTURE] [RUNS]
#
# FIXTURE is small, medium (default) or large; RUNS defaults to 3. Each run
# uses a fresh GRAFT_HOME, so its first graft builds the layer (cold) and
# its second reuses it (warm). Needs release builds of graft and
# graft-bench (cargo build --release -p graft-cli -p graft-bench).
set -euo pipefail

work=${1:?usage: $0 WORK_DIR [FIXTURE] [RUNS]}
fixture=${2:-medium}
runs=${3:-3}
root=$(cd "$(dirname "$0")/.." && pwd)
graft=$root/target/release/graft
bench=$root/target/release/graft-bench
install="npm ci --offline --no-audit --no-fund --ignore-scripts"

mkdir -p "$work"
repo=$("$bench" fixture "$fixture" "$work/fixtures" | tail -n 1)

now_us() { perl -MTime::HiRes=time -e 'printf "%d\n", time * 1e6'; }
ms() { awk -v us="$1" 'BEGIN { printf "%.0f", us / 1000 }'; }

# Prints "step<TAB>us" per timing line of one `graft new`.
timed_new() {
  GRAFT_LOG=graft::timing=debug "$graft" new "$1" 2>&1 >/dev/null |
    sed 's/\x1b\[[0-9;]*m//g' |
    awk -F'step=' '/ timing / && NF > 1 {
      split($2, a, "\""); step = a[2]
      match($0, /us=[0-9]+/); us = substr($0, RSTART + 3, RLENGTH - 3)
      printf "%s\t%s\n", step, us }'
}

echo "# graft new profile: $fixture fixture"
echo
echo "| | |"
echo "| --- | --- |"
echo "| OS | $(uname -sr) ($(uname -m)) |"
echo "| volume | $work ($("$graft" doctor "$work" 2>/dev/null | awk '$1 == "filesystem" {print $2; exit}')) |"
echo "| runs | $runs |"
echo
echo "| run | graft | step | ms |"
echo "| ---: | --- | --- | ---: |"
for run in $(seq 1 "$runs"); do
  for variant in ${GRAFT_PROFILE_VARIANTS:-default}; do
    export GRAFT_HOME=$work/home-$run-$variant
    rm -rf "$GRAFT_HOME"
    (cd "$repo" && "$graft" init >/dev/null && "$graft" trust >/dev/null)
    [[ $variant == default ]] && unset GRAFT_EXPERIMENT_LAYER || export GRAFT_EXPERIMENT_LAYER=$variant
    for which in cold warm; do
      (cd "$repo" && timed_new "$which") | while IFS=$'\t' read -r step us; do
        echo "| $run | $which ($variant) | $step | $(ms "$us") |"
      done
    done
    (cd "$repo" && "$graft" prune cold --force >/dev/null && "$graft" prune warm --force >/dev/null)
    rm -rf "$GRAFT_HOME"
  done
  unset GRAFT_EXPERIMENT_LAYER

  wt=$work/worktree-$run
  t0=$(now_us)
  git -C "$repo" worktree add --quiet --detach "$wt" HEAD
  t1=$(now_us)
  (cd "$wt" && $install >/dev/null 2>&1)
  t2=$(now_us)
  git -C "$repo" worktree remove --force "$wt"
  echo "| $run | worktree | git worktree add | $(ms $((t1 - t0))) |"
  echo "| $run | worktree | npm ci | $(ms $((t2 - t1))) |"
done
