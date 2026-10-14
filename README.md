# autumn-natter

The millionth LLM natter software. Powered by Rust and Tauri.

Autumn Natter starts llama-server and talks to it over the OpenAI compatible
HTTP interface. Nothing leaves your computer.

## Layout

The program is a workspace of two crates.

- `.` is the core: the engine, the settings, the store, the llama.cpp reader,
  the terminal tool, the device list, UPnP, and the page of the remote mode. It
  has no window, so it builds and tests with no web view.
- `src-tauri` is the desktop window. Rust on the back, and an HTML page under
  `frontend/` drawn by the web view of the system.

## Build

The window needs the web view of the system.

    Debian/Ubuntu:  sudo apt install libwebkit2gtk-4.1-dev build-essential libssl-dev libayatana-appindicator3-dev librsvg2-dev pkg-config
    Fedora:         sudo dnf install webkit2gtk4.1-devel openssl-devel libappindicator-gtk3-devel librsvg2-devel pkgconf-pkg-config
    Arch:           sudo pacman -S webkit2gtk-4.1 base-devel openssl libappindicator-gtk3 librsvg

Then:

    cargo build --release          # the window lands in target/release
    ./autumn-natter                # open the window
    ./autumn-natter --remote       # no window, serve a page for the local network
    cargo test -p autumn-natter-core   # the rules of the carousel, on the engine

Run `--help` for the arguments.
