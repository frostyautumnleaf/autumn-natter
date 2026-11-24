#!/bin/sh
# Capture a page-shot scene through a local HTTP server and system Chromium.
set -eu
scene=${1:-home-dark}
image=${2:-screenshot.png}
shift 2 || true
AN_CHROME=${AN_CHROME:-$(command -v chromium || command -v chromium-browser || command -v google-chrome)}
export AN_CHROME
port=${AN_SCREENSHOT_PORT:-18765}
python3 -m http.server "$port" --bind 127.0.0.1 --directory . >/tmp/autumn-natter-shot-http.log 2>&1 &
server=$!
trap 'kill "$server" 2>/dev/null || true' EXIT INT TERM
export AN_PAGE_URL="http://127.0.0.1:$port/frontend/index.html"
node "$(dirname "$0")/page-shot.js" "$scene" "$image" "$@"
