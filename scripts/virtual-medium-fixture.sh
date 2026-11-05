#!/usr/bin/env bash
# Builds and tests the medium benchmark fixture inside a virtual graft
# (AGENTS.md Phases 6 and 7): npm installs its layer, `npm test` runs in the
# graft, then an edit inside the graft is tested again and shows in
# `graft status`. Uses FUSE on Linux and NFS on macOS.
#
#   scripts/virtual-medium-fixture.sh <work-dir>
#
# Needs Node and a built workspace (target/debug).
set -euo pipefail

work=${1:?usage: $0 <work-dir>}
root=$(cd "$(dirname "$0")/.." && pwd)
bin="$root/target/debug"
export GRAFT_HOME="$work/home" GRAFTD="$bin/graftd"
mkdir -p "$work"

fixture=$("$bin/graft-bench" fixture medium "$work/fixtures")
cd "$fixture"
"$bin/graft" init
"$bin/graft" trust
trap '"$bin/graft" daemon stop >/dev/null 2>&1 || true' EXIT

case "$(uname -s)" in
    Linux) backend=fuse ;;
    Darwin) backend=nfs ;;
    *) echo "no virtual backend on $(uname -s)" >&2; exit 1 ;;
esac
now() { perl -MTime::HiRes=time -e 'printf "%d\n", time * 1000'; }
ms() { echo $(( $(now) - $1 )); }
start=$(now)
"$bin/graft" new medium --backend "$backend"
graft_dir=$("$bin/graft" path medium)
echo "graft new: $(ms "$start") ms"

cd "$graft_dir"
start=$(now)
npm test
echo "npm test in the graft: $(ms "$start") ms"

# Edit a module and an npm-managed file inside the graft, and test again.
# No `| head -1`: it would SIGPIPE git, which pipefail turns into a failure.
first=$(git ls-files 'src/*' | sed -n 1p)
echo "// edited in a graft" >> "$first"
npm test
git status --porcelain --untracked-files=all > "$work/git-status"
"$bin/graft" status medium > "$work/graft-status"
grep -q "M $first" "$work/graft-status"
grep -q "$first" "$work/git-status"
echo "graft status and git status both show $first"

cd "$fixture"
"$bin/graft" log medium | tail -3
"$bin/graft" prune medium --force
left=$(find "$GRAFT_HOME"/roots/*/blobs "$GRAFT_HOME"/roots/*/grafts -type f 2>/dev/null || true)
test -z "$left" || { echo "prune left: $left" >&2; exit 1; }
echo "medium fixture builds and tests inside a $backend graft"
