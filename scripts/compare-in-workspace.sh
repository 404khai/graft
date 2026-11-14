#!/usr/bin/env bash
# Time the same commands inside several workspaces (a worktree, a reflink
# graft, an NFS graft, ...), interleaved: every round runs each command
# once in every workspace, in a rotating order, so load changes on a busy
# machine hit all columns alike. Prints a Markdown table of medians
# (min-max) in milliseconds.
#
#   scripts/compare-in-workspace.sh ROUNDS LABEL=DIR [LABEL=DIR ...]
#
# Commands are read from $COMPARE_COMMANDS (one per line) or default to
# the in-workspace operations of the macOS benchmark report: git status,
# git diff, rg and find. Each command runs with `sh -c` in DIR, stdin
# from /dev/null and output discarded. Make the same edit in every DIR
# first if you want `git diff` to have something to show.
set -euo pipefail

rounds=${1:?usage: $0 ROUNDS LABEL=DIR [LABEL=DIR ...]}
shift
[ $# -ge 1 ] || { echo "need at least one LABEL=DIR" >&2; exit 2; }

default_commands='git status
git diff
rg -q format .
find . -type f | wc -l'
commands=${COMPARE_COMMANDS:-$default_commands}

labels=() dirs=()
for spec in "$@"; do
  labels+=("${spec%%=*}")
  dirs+=("${spec#*=}")
done
n=${#labels[@]}

out=$(mktemp)
trap 'rm -f "$out"' EXIT

now_us() { perl -MTime::HiRes=time -e 'printf "%d\n", time * 1e6'; }

ci=0
while IFS= read -r cmd; do
  [ -n "$cmd" ] || continue
  # One untimed warm-up per workspace.
  for d in "${dirs[@]}"; do (cd "$d" && sh -c "$cmd" </dev/null >/dev/null 2>&1) || true; done
  for ((r = 0; r < rounds; r++)); do
    for ((k = 0; k < n; k++)); do
      i=$(((k + r) % n))
      start=$(now_us)
      (cd "${dirs[$i]}" && sh -c "$cmd" </dev/null >/dev/null 2>&1) || true
      end=$(now_us)
      printf '%s\t%s\t%s\n' "$ci" "$i" "$((end - start))" >>"$out"
    done
  done
  ci=$((ci + 1))
done <<<"$commands"

printf '| command |'; for l in "${labels[@]}"; do printf ' %s |' "$l"; done; echo
printf '| --- |'; for _ in "${labels[@]}"; do printf ' --- |'; done; echo
ci=0
while IFS= read -r cmd; do
  [ -n "$cmd" ] || continue
  printf '| `%s` |' "$cmd"
  for ((i = 0; i < n; i++)); do
    awk -F'\t' -v c="$ci" -v w="$i" '$1 == c && $2 == w { print $3 }' "$out" | sort -n |
      awk '{ v[NR] = $1 } END {
        m = (NR % 2) ? v[(NR + 1) / 2] : (v[NR / 2] + v[NR / 2 + 1]) / 2
        printf " %.0f (%.0f–%.0f) |", m / 1000, v[1] / 1000, v[NR] / 1000 }'
  done
  echo
  ci=$((ci + 1))
done <<<"$commands"
echo
echo "$rounds interleaved rounds; ms, median (min–max); load: $(uptime | sed 's/.*load averages*: //')"
