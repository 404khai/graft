#!/usr/bin/env bash
# Generate fixture repositories under tests/fixtures/. They are not committed
# (see .gitignore); rerun this script to recreate them.
#
#   scripts/make-fixtures.sh            # all fixtures
#
# Fixtures:
#   small   a git repository with a few files and three commits
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
fixtures="$root/tests/fixtures"

# Deterministic commits, isolated from the user's git config.
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME="Graft Fixture" GIT_AUTHOR_EMAIL="fixture@graft.invalid"
export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME" GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"

make_small() {
  local dir="$fixtures/small"
  rm -rf "$dir"
  mkdir -p "$dir/src"
  git -C "$dir" init -q -b main
  local n=0
  commit() {
    n=$((n + 1))
    export GIT_AUTHOR_DATE="2026-01-0${n}T00:00:00Z" GIT_COMMITTER_DATE="2026-01-0${n}T00:00:00Z"
    git -C "$dir" add -A
    git -C "$dir" commit -q -m "$1"
  }
  printf '# small\n\nA Graft test fixture.\n' > "$dir/README.md"
  commit "docs: add readme"
  printf 'pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n' > "$dir/src/lib.rs"
  commit "feat: add add"
  printf 'pub fn sub(a: i32, b: i32) -> i32 {\n    a - b\n}\n' >> "$dir/src/lib.rs"
  commit "feat: add sub"
  echo "created $dir ($(git -C "$dir" rev-parse --short HEAD))"
}

make_small
