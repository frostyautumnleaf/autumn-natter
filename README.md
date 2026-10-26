<div align="center">
<img src="assets/readme/autumn-natter-plate.svg" width="112" height="112" alt="Autumn Natter: an autumn maple leaf with frost on its edges" />

# Autumn Natter

A chat program for models that run on your own machine. Rust and Tauri on the
outside, llama.cpp underneath.

The program starts `llama-server` for each answer and talks to it over the
OpenAI compatible HTTP interface. The model runs on your own cards, the chats
are kept in your own folders, and nothing of the conversation leaves the
machine.
</div>

## Features

- <img src="assets/readme/chat.svg" width="20" height="20" alt="" /> **Chats kept per repository.** Every repository keeps its own list of chats on disk. Two chats can be open side by side, and the full list of a repository opens above the input box with the Alt key.
- <img src="assets/readme/plus.svg" width="20" height="20" alt="" /> **A new chat and a context meter.** A new chat starts in the working folder of the repository with one click. A meter above the chat states what the conversation costs of the context window the model has, and changes colour before the window is full.
- <img src="assets/readme/send.svg" width="20" height="20" alt="" /> **Pictures and files in a message.** Attachments can be dropped in, pasted, or chosen from the disk. A vision model receives a picture as part of the message rather than as encoded text. Text pasted above a set length is attached as a file instead of filling the input box. A picture that the model opens for itself is shown in the block of that run, under an eye that marks it as one the model can see.
- <img src="assets/readme/think.svg" width="20" height="20" alt="" /> **Reasoning shown as it arrives.** Reasoning stands in a block of its own. It can be folded while it is being written, opened with a click, held at a fixed height, or allowed to grow with its text. A tool the model calls while reasoning can be placed inside the reasoning block that led to it, so the thought and its tool read as one part.
- <img src="assets/readme/terminal.svg" width="20" height="20" alt="" /> **A terminal the model can use.** One shell command per call, run in the working folder of the chat, with its output written back into the block. The block can be folded while the command runs, and a command that is stopped keeps the output it had already produced.
- <img src="assets/readme/code.svg" width="20" height="20" alt="" /> **File tools that either apply or refuse.** A file in the working folder of the chat can be changed or created. Every change in a call is matched against the file as it stood before the call, and the batch is written together or not at all.
- <img src="assets/readme/shield.svg" width="20" height="20" alt="" /> **A sandbox for the commands.** With bubblewrap a command sees only the working folder of the chat. Other files of the machine, the home folder included, are not visible, there is no network access, and a file written by one command is not seen by the next. The file tools reach the same files whether the sandbox is on or off.
- <img src="assets/readme/tooltip.svg" width="20" height="20" alt="" /> **Web search and page reading.** One tool searches the web and returns the text of a page as plain words, stripped of markup and shortened, so the model can answer from material it can reach.
- <img src="assets/readme/snowflake.svg" width="20" height="20" alt="" /> **Context reclaimed without losing the conversation.** When the context window of the model runs short, the program asks the model for a handoff summary. The text before the summary is left out of what is sent back, and the summary takes its place. Everything the model wrote stays in the chat, and a line at the right holds one node for every summarisation.
- <img src="assets/readme/chevron-down.svg" width="20" height="20" alt="" /> **Chapters, folding, and colour.** The model can mark where a new part of a long chat begins. The marks sit on a line at the left of the chat, and a click moves the reading position there. Code blocks are coloured by kind, and both the colouring and the marks can be switched off.
- <img src="assets/readme/model.svg" width="20" height="20" alt="" /> **The settings of the model, on a page.** Context size, sampling, speculative decoding with a draft model, context checkpoints, the chat template, and extra arguments. The command line that the next answer will run is written out as the fields are changed.
- <img src="assets/readme/gpu.svg" width="20" height="20" alt="" /> **Every graphics card accounted for.** The cards are read from the machine and each can be turned on or off. A budget of their video memory can be set, and the model can be split across the cards that are on.
- <img src="assets/readme/settings.svg" width="20" height="20" alt="" /> **The same settings in the browser.** The remote mode serves the page to the local network with no window at all: the chats, the tools, and both pages of the settings. UPnP can open it beyond this machine, on a port of your choosing.
- <img src="assets/readme/warning.svg" width="20" height="20" alt="" /> **Faults stated plainly.** Where a summarisation failed, where the sandbox is missing, or where the context ran out while a command was being written, the block states what happened, and the detail view states what to do next.

## Long conversations

A chat can continue indefinitely once space in the context window is reclaimed
as it fills. The Compaction section of the settings decides how.

- `Nothing` leaves the chat alone until the context window is full.
- `Handoff summary`, the default, asks the model between two answers for a
  summary of the conversation. It is triggered either at a share of the context
  window, 85 per cent by default, or when the space left falls below a number
  of tokens, 10240 by default.
- The first message you sent is never dropped, so the server keeps the work it
  did on those words and on the pictures attached to them.
- The request for a summary is limited on purpose. It is given only the space
  left in the window, between 512 and 4096 tokens. Pictures are turned into
  words and left out, and no tool is offered, because a model that answers a
  conversation full of tool calls with a tool call of its own makes the server
  fail where the summary should have stood.
- A summary that never arrived leaves its mark empty, and the detail view of the
  mark offers `Write the summary again`. Nothing of the conversation is thrown
  away, and no message is sent over a conversation that no longer fits.
- The limit is watched while the model is writing, and not only between two
  answers. A model with a tool call larger than the space it has does not stop
  by itself, so the stream is cut at the point where the conversation reached
  the limit. What was written stays in the chat, the call is recorded after the
  summarisation with a `cancelled` mark, and its words are sent back as the text
  of a turn rather than as a call. Three cut-offs in one answer is a text that
  no window will hold, and the reader is informed.

The rule lives in `compaction_wanted` in `src/config.rs`, the work in
`run_handoff`, `maybe_compact`, and `make_room` in `src/engine.rs`, and the
nodes and the marks in the desktop page and the browser page.

## Running the program

| Command | What it does |
| :-- | :-- |
| `./autumn-natter` | the window on this machine |
| `./autumn-natter --remote` | no window, a page for the local network, settings included |
| `cargo test -p autumn-natter-core` | the rules of the chats, with no display needed |

Run `--help` for the arguments of the program, and `./run.sh --help` for the
flags of the build.

## Build

The window needs the web view of the system.

    Debian/Ubuntu:  sudo apt install libwebkit2gtk-4.1-dev build-essential libssl-dev libayatana-appindicator3-dev librsvg2-dev pkg-config
    Fedora:         sudo dnf install webkit2gtk4.1-devel openssl-devel libappindicator-gtk3-devel librsvg2-devel pkgconf-pkg-config
    Arch:           sudo pacman -S webkit2gtk-4.1 base-devel openssl libappindicator-gtk3 librsvg

Then, in one command, with nothing written outside the checkout and no need for
sudo:

    ./run.sh                  # build, then open the window
    ./run.sh --remote         # build, then serve the page to the network
    ./run.sh --with-llama     # also build llama.cpp here, so you can chat

The script calls `./build.sh`, which keeps the toolchain, the crates, and the
build output under `./.local`, about 4 GB once built. `--fast` builds without
LTO, `--debug` compiles fastest, `--check` only checks, and `--offline` builds
from a vendored copy. To build by hand instead:

    cargo build --release     # the window lands in target/release

## Layout

The program is a workspace of two crates.

- `.` is the core: the engine, the settings, the store, the llama.cpp reader,
  the tools, the device list, UPnP, and the page of the remote mode. It has no
  window, so it builds and tests with no web view.
- `src-tauri` is the desktop window. Rust on the back, and an HTML page under
  `frontend/` drawn by the web view of the system.

The page served to a browser is `web/page.html`. The two pages are kept in step,
so a fault in the layout is found in a headless browser with
`tools/page-shot.js` before it is seen by a user. The pictures in this README are
made by `tools/readme_icons.py` from the icons of the program.

## Licence

MIT. See `LICENSE`.
