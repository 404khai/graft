#!/usr/bin/env bash
# Generate fixture repositories under tests/fixtures/. They are not committed
# (see .gitignore); rerun this script to recreate them.
#
#   scripts/make-fixtures.sh            # all fixtures
#
# Fixtures:
#   small     a git repository with a few files and three commits
#   node-app  a Node project whose graft.toml declares a node_modules layer;
#             its one dependency is a vendored tarball, so `npm ci` works
#             offline (requires node and npm)
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

make_node_app() {
  local dir="$fixtures/node-app"
  if ! command -v npm >/dev/null; then
    echo "skipping node-app: npm not found" >&2
    return
  fi
  rm -rf "$dir"
  mkdir -p "$dir/vendor" "$dir/.pkg/package"
  git -C "$dir" init -q -b main
  printf '{"name":"leftpad","version":"1.0.0","main":"index.js"}\n' > "$dir/.pkg/package/package.json"
  printf 'module.exports = (s, n, c = " ") => String(s).padStart(n, c);\n' > "$dir/.pkg/package/index.js"
  # Fixed mtimes and ordering so the tarball, and so the lockfile, is reproducible.
  (cd "$dir/.pkg" && find package -exec touch -t 202601010000 {} + &&
    tar -cf - package | gzip -n > "$dir/vendor/leftpad-1.0.0.tgz")
  rm -rf "$dir/.pkg"
  cat > "$dir/package.json" <<'JSON'
{
  "name": "node-app",
  "version": "1.0.0",
  "private": true,
  "scripts": { "test": "node test.js" },
  "dependencies": { "leftpad": "file:vendor/leftpad-1.0.0.tgz" }
}
JSON
  cat > "$dir/test.js" <<'JS'
const assert = require("assert");
const leftpad = require("leftpad");
assert.strictEqual(leftpad("7", 3, "0"), "007");
console.log("ok");
JS
  printf 'node_modules/\n' > "$dir/.gitignore"
  cat > "$dir/graft.toml" <<'TOML'
[[layer]]
path = "node_modules"
inputs = ["package.json", "package-lock.json"]
build = "npm ci --offline --no-audit --no-fund"
TOML
  (cd "$dir" && npm install --offline --no-audit --no-fund >/dev/null 2>&1)
  rm -rf "$dir/node_modules"
  export GIT_AUTHOR_DATE="2026-01-01T00:00:00Z" GIT_COMMITTER_DATE="2026-01-01T00:00:00Z"
  git -C "$dir" add -A
  git -C "$dir" commit -q -m "node app with a node_modules layer"
  echo "created $dir ($(git -C "$dir" rev-parse --short HEAD))"
}

make_small
make_node_app
