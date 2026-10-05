#!/usr/bin/env bash
#
# Autumn Natter - one command bootstrap.
#
# Installs the Rust toolchain into this checkout, builds the program, and runs
# it. Nothing is written outside this directory, and nothing needs sudo.
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
Autumn Natter bootstrap. Installs the toolchain locally, builds, runs.

    ./run.sh                 build the release binary and open the window
    ./run.sh --remote        build and serve the page to the local network
    ./run.sh --with-llama    also build llama.cpp here, so you can chat

Build flags
    --check                 run cargo check instead of a full build
    --debug                 build the debug profile (compiles much faster)
    --build-only            stop after building
    --offline               build with no network, from a vendored copy
    --force-build           rebuild even if the binary already exists

Toolchain flags
    --use-system-toolchain  reuse a cargo already on PATH. Saves a few hundred
                            MB, but the crate cache then lives in your user
                            folder instead of in this checkout.
    --force-toolchain       re-download the local toolchain

Runtime flags (passed on to the program)
    -r, --remote            no window, serve a page for the LAN
    -p, --port PORT         port for the remote page
    --data-dir DIR          keep the program data somewhere else
    --with-llama            clone and build llama.cpp into ./.local/llama.cpp
    --llama-dir DIR         use an existing llama-server build

    -h, --help              this text

Any unrecognised argument is passed straight to the program.

The first run downloads a Rust toolchain and every crate, so it needs a network
connection and roughly 2 GB of free space. Later runs work offline and take
seconds.
HELP
}

# ---------------------------------------------------------------- args
MODE="build"            # build | check
PROFILE="release"
RUN=1
EXTRA=()                # arguments for autumn-natter
USE_SYSTEM_TC=0
FORCE_TC=0
OFFLINE=0
LLAMA_DIR=""
WITH_LLAMA=0

while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help)          usage; exit 0 ;;
    --check)            MODE="check" ;;
    --debug)            PROFILE="debug" ;;
    --build-only|--no-run) RUN=0 ;;
    --offline)          OFFLINE=1 ;;
    --force-build)      rm -f "$LOCAL/target/$PROFILE/autumn-natter" 2>/dev/null || true ;;
    --use-system-toolchain) USE_SYSTEM_TC=1 ;;
    --force-toolchain)  FORCE_TC=1 ;;
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

printf '\033[1mAutumn Natter bootstrap\033[0m\n'
say "  checkout: $ROOT"

# ---------------------------------------------------------------- preflight
step "Checking the basics"

OS="$(uname -s)"
case "$OS" in
  Linux|Darwin) ;;
  *) die "unsupported operating system: $OS. On Windows use run.ps1." ;;
esac

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64)  HOST="x86_64-unknown-linux-gnu" ;;
  aarch64|arm64)
    if [ "$OS" = Darwin ]; then HOST="aarch64-apple-darwin"; else HOST="aarch64-unknown-linux-gnu"; fi ;;
  *) HOST="" ;;
esac

# Crates build native code, so a C compiler is unavoidable. It comes from the
# system; only the Rust side is installed into the checkout.
if command -v cc >/dev/null 2>&1 || command -v clang >/dev/null 2>&1; then
  ok "C compiler: $(command -v cc 2>/dev/null || command -v clang)"
else
  cat >&2 <<EOF

A C compiler is required, and it cannot be installed into the checkout.

    Debian/Ubuntu:  sudo apt install build-essential
    Fedora:         sudo dnf groupinstall 'Development Tools'
    Arch:           sudo pacman -S base-devel
    macOS:          xcode-select --install

EOF
  die "no C compiler found"
fi

command -v git >/dev/null 2>&1 || die "git is required"
if command -v curl >/dev/null 2>&1; then
  ok "git and curl: present"
elif command -v wget >/dev/null 2>&1; then
  ok "git and wget: present"
else
  die "curl or wget is required to download the Rust toolchain"
fi

[ -f Cargo.toml ] && [ -f src/main.rs ] || die "run.sh must sit next to Cargo.toml"
ok "checkout looks right"

# ---------------------------------------------------------------- toolchain
step "Rust toolchain"

if [ "$USE_SYSTEM_TC" = 1 ]; then
  command -v cargo >/dev/null 2>&1 \
    || die "--use-system-toolchain was given but no cargo is on PATH"
  ok "using the cargo on PATH: $(cargo --version)"
  say "    the crate cache stays in your user folder, not in this checkout"
elif [ -x "$LOCAL/cargo/bin/cargo" ] && [ "$FORCE_TC" = 0 ]; then
  export RUSTUP_HOME="$LOCAL/rustup" CARGO_HOME="$LOCAL/cargo"
  export PATH="$LOCAL/cargo/bin:$PATH"
  ok "using the local toolchain: $(cargo --version)"
else
  [ "$FORCE_TC" = 1 ] && rm -rf "$LOCAL/rustup" "$LOCAL/cargo"

  FREE_KB="$(df -Pk "$ROOT" | awk 'NR==2 {print $4}' || true)"
  if [ -n "${FREE_KB:-}" ] && [ "$FREE_KB" -lt 2000000 ]; then
    warn "only $((FREE_KB / 1024)) MB free here. Toolchain plus build needs about 2 GB."
  fi

  say "    installing a private toolchain into $LOCAL"
  say "    a few hundred MB, once. Nothing system wide is touched."
  mkdir -p "$LOCAL"
  export RUSTUP_HOME="$LOCAL/rustup" CARGO_HOME="$LOCAL/cargo"

  TMP_INIT="$LOCAL/rustup-init"
  URL="https://static.rust-lang.org/rustup/dist"
  if command -v curl >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -fL -o "$TMP_INIT" "$URL/$HOST/rustup-init" \
      || die "could not download rustup-init. Check the network."
  else
    wget -O "$TMP_INIT" "$URL/$HOST/rustup-init" \
      || die "could not download rustup-init. Check the network."
  fi
  chmod +x "$TMP_INIT"

  # minimal profile: rustc, cargo and the standard library, no docs.
  "$TMP_INIT" -y --no-modify-path --profile minimal \
    ${HOST:+--default-toolchain "stable-$HOST"} >/dev/null 2>&1 \
    || die "rustup-init failed. See https://rustup.rs for a manual install."
  rm -f "$TMP_INIT"

  export PATH="$LOCAL/cargo/bin:$PATH"
  [ -x "$LOCAL/cargo/bin/cargo" ] || die "rustup finished but cargo is missing"
  ok "installed: $(cargo --version)"
fi

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
if [ "$MODE" = "check" ]; then
  step "cargo check"
  if [ "$OFFLINE" = 1 ]; then cargo check --offline; else cargo check; fi
  ok "no errors"
  exit 0
fi

step "Building the $PROFILE profile"
# The build output stays in the checkout too, so nothing lands in a shared cache.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$LOCAL/target}"
BIN="$CARGO_TARGET_DIR/$PROFILE/autumn-natter"

if [ -x "$BIN" ]; then
  ok "$BIN already exists, skipping the build (use --force-build to redo it)"
else
  say "    the release profile uses LTO and one codegen unit, so it is slow on purpose"
  if [ "$PROFILE" = release ]; then
    if [ "$OFFLINE" = 1 ]; then cargo build --offline --release; else cargo build --release; fi
  else
    if [ "$OFFLINE" = 1 ]; then cargo build --offline; else cargo build; fi
  fi
  ok "built: $BIN"
fi

if [ "$RUN" = 0 ]; then
  say ""
  say "Built. Start it later with:  ./run.sh"
  say "Or run the binary direct:    $BIN"
  exit 0
fi

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

exec "$BIN" ${EXTRA[@]+"${EXTRA[@]}"}
