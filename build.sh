#!/usr/bin/env bash
#
# Autumn Natter - build script.
#
# Installs the Rust toolchain into this checkout (if needed) and builds the
# program. Nothing is written outside this directory, and nothing needs sudo.
#
#   ./build.sh              build the release binary
#   ./build.sh --fast       build without LTO (much faster, larger binary)
#   ./build.sh --debug      build the debug profile
#
# Run ./build.sh --help for all flags.

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
Autumn Natter build. Installs the toolchain locally and builds.

    ./build.sh                 build the release binary (small, slow to build)
    ./build.sh --fast          build without LTO (fast, larger binary)
    ./build.sh --debug         build the debug profile (fastest build)
    ./build.sh --check         run cargo check instead of a full build

Build flags
    --offline                  build with no network, from a vendored copy
    --force-build              rebuild even if the binary already exists

Toolchain flags
    --use-system-toolchain     reuse a cargo already on PATH
    --force-toolchain          re-download the local toolchain

    -h, --help                 this text

The first run downloads a Rust toolchain and every crate, so it needs a network
connection. Expect ./.local to reach about 4 GB once built.
HELP
}

# ---------------------------------------------------------------- args
MODE="build"            # build | check
PROFILE="release"
FAST=0
OFFLINE=0
FORCE_BUILD=0
USE_SYSTEM_TC=0
FORCE_TC=0

while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help)          usage; exit 0 ;;
    --check)            MODE="check" ;;
    --debug)            PROFILE="debug" ;;
    --fast)             FAST=1 ;;
    --offline)          OFFLINE=1 ;;
    --force-build)      FORCE_BUILD=1 ;;
    --use-system-toolchain) USE_SYSTEM_TC=1 ;;
    --force-toolchain)  FORCE_TC=1 ;;
    *)                  die "unknown flag: $1 (try --help)" ;;
  esac
  shift
done

if [ "$FAST" = 1 ] && [ "$PROFILE" != "release" ]; then
  die "--fast only works with the release profile"
fi

printf '\033[1mAutumn Natter build\033[0m\n'
say "  checkout: $ROOT"

# ---------------------------------------------------------------- preflight
step "Checking the basics"

OS="$(uname -s)"
case "$OS" in
  Linux|Darwin) ;;
  *)
    cat >&2 <<EOF

This script covers Linux and macOS. On Windows ($OS) do it by hand:

  1. Install Rust for Windows from https://rustup.rs and open a new shell.
  2. cargo build --release
  3. target\\release\\autumn-natter.exe

EOF
    die "unsupported operating system: $OS" ;;
esac

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64)  HOST="x86_64-unknown-linux-gnu" ;;
  aarch64|arm64)
    if [ "$OS" = Darwin ]; then HOST="aarch64-apple-darwin"; else HOST="aarch64-unknown-linux-gnu"; fi ;;
  *) HOST="" ;;
esac

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

[ -f Cargo.toml ] && [ -f src/main.rs ] || die "build.sh must sit next to Cargo.toml"
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
  if [ "$FORCE_TC" = 1 ]; then
    say "    dropping the old local toolchain"
    rm -rf "$LOCAL/rustup" "$LOCAL/cargo"
  fi

  FREE_KB="$(df -Pk "$ROOT" | awk 'NR==2 {print $4}' || true)"
  if [ -n "${FREE_KB:-}" ] && [ "$FREE_KB" -lt 4000000 ]; then
    warn "only $((FREE_KB / 1024)) MB free here. Toolchain plus build needs about 4 GB."
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

  "$TMP_INIT" -y --no-modify-path --profile minimal \
    ${HOST:+--default-toolchain "stable-$HOST"} >/dev/null 2>&1 \
    || die "rustup-init failed. See https://rustup.rs for a manual install."
  rm -f "$TMP_INIT"

  export PATH="$LOCAL/cargo/bin:$PATH"
  [ -x "$LOCAL/cargo/bin/cargo" ] || die "rustup finished but cargo is missing"
  ok "installed: $(cargo --version)"
fi

# ---------------------------------------------------------------- build
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$LOCAL/target}"
BIN="$CARGO_TARGET_DIR/$PROFILE/autumn-natter"

if [ "$MODE" = "check" ]; then
  step "cargo check"
  if [ "$OFFLINE" = 1 ]; then cargo check --offline; else cargo check; fi
  ok "no errors"
  exit 0
fi

# Only skip if the binary exists and --force-build was not given.
# Cargo's incremental build makes a no-change rebuild fast.
if [ -x "$BIN" ] && [ "$FORCE_BUILD" = 0 ]; then
  # Check if any source file is newer than the binary.
  NEED_BUILD=0
  if find src ui Cargo.toml build.rs -newer "$BIN" -print -quit 2>/dev/null | grep -q .; then
    NEED_BUILD=1
  fi
  if [ "$NEED_BUILD" = 0 ]; then
    ok "$BIN already exists and is up to date"
    exit 0
  fi
  say "    source changed since last build, rebuilding"
fi

if [ "$FAST" = 1 ]; then
  step "Building the release profile without LTO (fast)"
  say "    this skips link-time optimisation. The binary is larger but builds faster."
  if [ "$OFFLINE" = 1 ]; then
    cargo build --offline --release --config 'profile.release.lto=false' --config 'profile.release.codegen-units=16'
  else
    cargo build --release --config 'profile.release.lto=false' --config 'profile.release.codegen-units=16'
  fi
else
  step "Building the $PROFILE profile"
  say "    the release profile uses LTO and one codegen unit, so the final link is slow"
  if [ "$PROFILE" = "release" ]; then
    if [ "$OFFLINE" = 1 ]; then cargo build --offline --release; else cargo build --release; fi
  else
    if [ "$OFFLINE" = 1 ]; then cargo build --offline; else cargo build; fi
  fi
fi

ok "built: $BIN"
