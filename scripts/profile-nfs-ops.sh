#!/usr/bin/env bash
# Profile graftd's NFS procedures while a command runs in an NFS graft
# (macOS). Prints, per procedure, how many calls graftd served and their
# total, mean and p99 time inside graftd, plus the wall time of the
# command and the client's RPC counts (`nfsstat -c`) for the same window.
# The gap between wall time and graftd's own time is spent in the kernel
# NFS client, the loopback transport and the command itself.
#
#   scripts/profile-nfs-ops.sh GRAFT_HOME GRAFT_DIR -- COMMAND...
#
# GRAFT_HOME is the home graftd serves the graft from; graftd is restarted
# with GRAFT_LOG=graft::nfs=debug so it logs one timing line per call (set
# GRAFT to the graft binary; default target/release/graft). Run it from
# the Root of the graft.
set -euo pipefail

home=${1:?usage: $0 GRAFT_HOME GRAFT_DIR -- COMMAND...}
dir=${2:?usage: $0 GRAFT_HOME GRAFT_DIR -- COMMAND...}
shift 2
[ "${1:-}" = -- ] && shift
[ $# -ge 1 ] || { echo "no command" >&2; exit 2; }
here=$(cd "$(dirname "$0")/.." && pwd)
graft=${GRAFT:-$here/target/release/graft}
export GRAFT_HOME=$home

"$graft" daemon stop >/dev/null 2>&1 || true
# Wait for the old graftd to exit, so the next command starts a new one.
while pgrep -f "graftd --home $home\$" >/dev/null; do sleep 0.2; done
GRAFT_LOG=graft::nfs=debug "$graft" ls >/dev/null
ls "$dir" >/dev/null
log=$home/run/graftd.log
: >"$log"

counts() {
  nfsstat -c | awk '/^NLM/ { exit } NR > 2 { if (NR % 2 == 1) split($0, k); else for (i = 1; i <= NF; i++) print k[i], $i }'
}
before=$(mktemp)
after=$(mktemp)
trap 'rm -f "$before" "$after"' EXIT
counts >"$before"
start=$(perl -MTime::HiRes=time -e 'printf "%d\n", time * 1e6')
(cd "$dir" && "$@" </dev/null >/dev/null 2>&1) || echo "(command failed: $?)"
end=$(perl -MTime::HiRes=time -e 'printf "%d\n", time * 1e6')
sleep 1.5 # let idle writes journal
counts >"$after"

echo "wall: $(((end - start) / 1000)) ms; load $(uptime | sed 's/.*load averages*: //')"
echo
echo "| procedure | calls | total ms | mean us | p99 us |"
echo "| --- | ---: | ---: | ---: | ---: |"
sed 's/\x1b\[[0-9;]*m//g' "$log" | awk '/graft::nfs/ && / timing/ {
    match($0, /op="?[a-z]+/); op = substr($0, RSTART + 3, RLENGTH - 3); sub(/^"/, "", op)
    match($0, /us=[0-9]+/); us = substr($0, RSTART + 3, RLENGTH - 3)
    print op, us }' | sort -k1,1 -k2,2n | awk '
  { op[NR] = $1; us[NR] = $2 }
  END {
    i = 1
    while (i <= NR) {
      j = i; sum = 0
      while (j <= NR && op[j] == op[i]) { sum += us[j]; j++ }
      n = j - i; p = i + int(0.99 * (n - 1))
      printf "| %s | %d | %.0f | %.0f | %d |\n", op[i], n, sum / 1000, sum / n, us[p]
      i = j
    }
  }'
echo
echo "client RPCs:"
join "$before" "$after" | awk '$3 != $2 { printf "  %s %d\n", $1, $3 - $2 }'
