#!/usr/bin/env bash
# Time `graft integrate` of N non-overlapping grafts, step by step.
#
#   scripts/measure-integrate.sh WORK_DIR REPO [GRAFTS] [ROUNDS]
#
# REPO is a git repository to clone (with a graft.toml whose layers
# `npm ci` or similar can build); GRAFTS defaults to 4, ROUNDS to 3. Each
# round clones REPO into WORK_DIR with a fresh GRAFT_HOME, creates GRAFTS
# grafts that each append a line to a different tracked file and commit
# it, then times `graft integrate` of all of them onto a side branch, so
# the measurement is landing alone. Set GRAFT to the binary to measure
# (default: target/debug/graft) and BACKEND to `nfs` for virtual grafts.
# Prints total wall time per round and the median of each
# `graft::timing` step, summed over the grafts of a round.
set -euo pipefail

work=${1:?usage: $0 WORK_DIR REPO [GRAFTS] [ROUNDS]}
repo=${2:?usage: $0 WORK_DIR REPO [GRAFTS] [ROUNDS]}
grafts=${3:-4}
rounds=${4:-3}
here=$(cd "$(dirname "$0")/.." && pwd)
graft=${GRAFT:-$here/target/debug/graft}
backend=${BACKEND:+--backend $BACKEND}

now_us() { perl -MTime::HiRes=time -e 'printf "%d\n", time * 1e6'; }
steps=$(mktemp)
trap 'rm -f "$steps"' EXIT

for ((r = 1; r <= rounds; r++)); do
  dir=$work/round-$r
  rm -rf "$dir"
  mkdir -p "$dir"
  git clone -q "$repo" "$dir/root"
  export GRAFT_HOME=$dir/home
  cd "$dir/root"
  git branch -q land
  "$graft" init >/dev/null
  "$graft" trust >/dev/null
  files=$(git ls-files '*.ts' '*.rs' '*.py' '*.md' | awk -v n="$grafts" 'NR <= n')
  names=()
  i=0
  for f in $files; do
    i=$((i + 1))
    n=g$i
    # shellcheck disable=SC2086
    "$graft" new "$n" $backend >/dev/null 2>&1
    p=$("$graft" path "$n")
    echo "// graft $n" >>"$p/$f"
    git -C "$p" commit -qam "edit $f in $n"
    names+=("$n")
  done
  start=$(now_us)
  GRAFT_LOG=graft::timing=debug "$graft" integrate --onto land "${names[@]}" 2>"$dir/timing.log" >"$dir/out.log"
  end=$(now_us)
  sed 's/\x1b\[[0-9;]*m//g' "$dir/timing.log" |
    awk -F'step=' -v r="$r" '/ timing / && NF > 1 {
      split($2, a, "\""); step = a[2]
      match($0, /us=[0-9]+/); us = substr($0, RSTART + 3, RLENGTH - 3)
      sum[step] += us } END { for (s in sum) printf "%s\t%s\t%d\n", r, s, sum[s] }' >>"$steps"
  printf 'round %d: integrate of %d grafts took %d ms (load %s)\n' "$r" "$grafts" \
    $(((end - start) / 1000)) "$(uptime | sed 's/.*load averages*: //')"
  cat "$dir/out.log"
  cd "$work"
  "$graft" daemon stop >/dev/null 2>&1 || true
done

echo
echo "| step (summed over $grafts grafts) | median ms |"
echo "| --- | --- |"
cut -f2 "$steps" | sort -u | while IFS= read -r s; do
  awk -F'\t' -v s="$s" '$2 == s { print $3 }' "$steps" | sort -n |
    awk -v s="$s" '{ v[NR] = $1 } END {
      m = (NR % 2) ? v[(NR + 1) / 2] : (v[NR / 2] + v[NR / 2 + 1]) / 2
      printf "| %s | %.0f |\n", s, m / 1000 }'
done
