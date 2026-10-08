#!/usr/bin/env bash
#
# Autumn Natter - build and run in one command.
#
# Calls build.sh to set up the toolchain and build the program, then runs it.
# Nothing is written outside this directory, and nothing needs sudo.
#
#   git clone https://github.com/frostyautumnleaf/autumn-natter
#   cd autumn-natter
#   ./run.sh
#
# Everything it installs lives in ./.local . Delete that folder to undo it all.
#
# Run ./run.sh --help for the flags.

set -euo pipefail

# ---------------------------------------------------------------- locations
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

LOCAL="$ROOT/.local"

# ---------------------------------------------------------------- helpers
say()  { printf '%s\n' "$*"; }
step() { printf '\n\033[1;36m==>\033[0m \033[1m%s\033[0m\n' "$*"; }
ok()   { printf '    \033[1;32m+\033[0m %s\n' "$*"; }
warn() { printf '    \033[1;33m!\033[0m %s\n' "$*"; }
die()  { printf '\n\033[1;31mx\033[0m %s\n' "$*" >&2; exit 1; }

usage() {
  cat <<'HELP'
Autumn Natter. Builds (if needed) and runs.

    ./run.sh                 build the release binary and open the window
    ./run.sh --remote        build and serve the page to the local network
    ./run.sh --with-llama    also build llama.cpp here, so you can chat

Build flags
    --fast                   build without LTO (much faster, larger binary)
    --debug                  build the debug profile (compiles much faster)
    --check                  run cargo check instead of a full build
    --build-only             stop after building
    --offline                build with no network, from a vendored copy
    --force-build            rebuild even if the binary already exists

Toolchain flags
    --use-system-toolchain   reuse a cargo already on PATH. Saves a few hundred
                             MB, but the crate cache then lives in your user
                             folder instead of in this checkout.
    --force-toolchain        re-download the local toolchain

Runtime flags (passed on to the program)
    -r, --remote             no window, serve a page for the LAN
    -p, --port PORT          port for the remote page
    --data-dir DIR           keep the program data somewhere else
    --with-llama             clone and build llama.cpp into ./.local/llama.cpp
    --llama-dir DIR          use an existing llama-server build

    -h, --help               this text

Any unrecognised argument is passed straight to the program.

The first run downloads a Rust toolchain and every crate, so it needs a network
connection. Expect ./.local to reach about 4 GB once built (roughly 600 MB
toolchain, 550 MB crate cache, the rest build output). Later runs work offline
and take seconds.
HELP
}

# ---------------------------------------------------------------- args
BUILD_ARGS=()
RUN=1
EXTRA=()                # arguments for autumn-natter
LLAMA_DIR=""
WITH_LLAMA=0

while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help)          usage; exit 0 ;;
    --fast|--debug|--check|--offline|--force-build|--use-system-toolchain|--force-toolchain)
                        BUILD_ARGS+=("$1") ;;
    --build-only|--no-run) RUN=0 ;;
    --with-llama)       WITH_LLAMA=1 ;;
    --llama-dir)        [ $# -ge 2 ] || die "--llama-dir needs a folder"
                        LLAMA_DIR="$(cd -- "$2" && pwd)"; shift ;;
    -r|--remote|-p|--port|--data-dir)
                        EXTRA+=("$1")
                        case "$1" in
                          -p|--port|--data-dir)
                            [ $# -ge 2 ] || die "$1 needs a value"
                            EXTRA+=("$2"); shift ;;
                        esac ;;
    *)                  EXTRA+=("$1") ;;
  esac
  shift
done

printf '\033[1mAutumn Natter\033[0m\n'
say "  checkout: $ROOT"

# ---------------------------------------------------------------- llama.cpp
if [ "$WITH_LLAMA" = 1 ]; then
  step "llama.cpp, the slow part"
  command -v cmake >/dev/null 2>&1 \
    || die "cmake is needed to build llama.cpp. Or use --llama-dir with a build you have."
  LLAMA_SRC="$LOCAL/llama.cpp"
  if [ -d "$LLAMA_SRC/.git" ]; then
    say "    updating the existing checkout"
    git -C "$LLAMA_SRC" fetch --depth 1 origin >/dev/null 2>&1 || true
    git -C "$LLAMA_SRC" reset --hard FETCH_HEAD >/dev/null 2>&1 || true
  else
    say "    cloning llama.cpp"
    git clone --depth 1 https://github.com/ggml-org/llama.cpp "$LLAMA_SRC" \
      || die "clone failed. Check the network, or use --llama-dir."
  fi
  say "    building llama-server. A small machine can take 20 minutes or more."
  cmake -S "$LLAMA_SRC" -B "$LLAMA_SRC/build" -DCMAKE_BUILD_TYPE=Release -DGGML_CCACHE=OFF >/dev/null \
    || die "cmake configure failed"
  cmake --build "$LLAMA_SRC/build" -j "$(nproc 2>/dev/null || sysctl -n hw.ncpu)" \
    --target llama-server >/dev/null \
    || die "llama-server build failed. Build llama.cpp yourself and pass --llama-dir."
  LLAMA_DIR="$LLAMA_SRC/build/bin"
  ok "built: $LLAMA_DIR/llama-server"
fi

if [ -z "$LLAMA_DIR" ]; then
  # Find a server the user already has, so the run is useful out of the box.
  for guess in "$LOCAL/llama.cpp/build/bin" "$ROOT/llamacpp/build/bin" /usr/local/bin /usr/bin; do
    if [ -x "$guess/llama-server" ] || [ -x "$guess/bin/llama/llama-server" ]; then
      LLAMA_DIR="$guess"
      break
    fi
  done
fi

# ---------------------------------------------------------------- build
if [ "$RUN" = 0 ]; then
  ./build.sh "${BUILD_ARGS[@]}"
  exit 0
fi

./build.sh "${BUILD_ARGS[@]}"

# ---------------------------------------------------------------- run
step "Starting"

if [ -n "$LLAMA_DIR" ]; then
  export AUTUMN_NATTER_LLAMA_DIR="$LLAMA_DIR"
  ok "llama-server folder: $LLAMA_DIR"
else
  warn "no llama-server found. The window opens, but no model can load."
  warn "Fix it with:  ./run.sh --with-llama"
  warn "or:           ./run.sh --llama-dir /path/to/dir/containing/llama-server"
fi

REMOTE=0
for a in ${EXTRA[@]+"${EXTRA[@]}"}; do
  case "$a" in -r|--remote) REMOTE=1 ;; esac
done

if [ "$REMOTE" = 0 ] && [ -z "${WAYLAND_DISPLAY:-}" ] && [ -z "${DISPLAY:-}" ]; then
  warn "no Wayland or X11 display is set, so the window cannot open."
  warn "On a headless machine use:  ./run.sh --remote"
fi

say ""
say "  Ctrl-C quits. Headless alternative: ./run.sh --remote"
say ""

# Find the binary (build.sh may have used --fast or --debug)
if [ "${BUILD_ARGS[*]}" = *"--debug"* ]; then
  BIN="$LOCAL/target/debug/autumn-natter"
elif [ "${BUILD_ARGS[*]}" = *"--check"* ]; then
  die "--check only checks, it does not produce a binary"
else
  BIN="$LOCAL/target/release/autumn-natter"
fi

exec "$BIN" ${EXTRA[@]+"${EXTRA[@]}"}
