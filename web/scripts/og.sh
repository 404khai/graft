#!/bin/sh
# Renders og/ to public/og.png (1200×630) with vite dev and headless Chrome.
set -eu
cd "$(dirname "$0")/.."
CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
npx vite --port 5199 --strictPort >/dev/null 2>&1 &
PID=$!
trap 'kill $PID' EXIT
sleep 3
"$CHROME" --headless=new --force-prefers-reduced-motion --hide-scrollbars --virtual-time-budget=8000 \
  --window-size=1200,630 --screenshot="$PWD/public/og.png" http://localhost:5199/og/ 2>/dev/null
echo "wrote public/og.png"
