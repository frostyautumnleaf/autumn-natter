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

## Compaction

A chat goes on for ever once the room of the talk is made again on the way. The
Compaction section of the settings says how that is done.

- `Nothing` leaves the chat alone until the room is gone.
- `Handoff summary` (the default) asks the model, between two answers, for a
  summary of the talk. The words before that summary are then left out of what
  goes back to the model, and the summary rides in their place.

The summarisation is set going either at a share of the room of the model (the
default, 85 per cent) or when the room left over falls under a number of tokens
(the default, 10240). One line in `src/config.rs` owns the rule, so the fields
of the window and the check of the worker cannot drift apart.

The first message you sent is never dropped. It stands at the front of the
prompt as it always did, word for word, so the server keeps the work it did on
those words and on the pictures hung on them: the summary is added after it, and
only the summary is worked through anew. The text the model is asked to write is
yours to change in the settings (the prompt of the program is given back when the
field is left empty, and it is cut at 4096 characters).

What the model wrote is kept in the chat as a mark of its own, so nothing of the
talk is thrown away: the reader still sees all of it. In the talk the mark stands
as `Summarised #N` — a hairline with the count of it in the middle, for a
handoff summary is not a saying of the model to the reader. Hover over it and the
card says the day and the hour it stood, the tokens the talk took before it, and
the tokens the summary itself takes. Beside the chat, at the right, a vertical
line holds one node for every summarisation, the oldest at the top; a node says
which summarisation it is and shows a little of what it says, and a click takes
you there. The line is a switch in the same section.

The ask for a summary is bounded on purpose, for it carries the whole of the talk.
The room the summary is written in is only what is left in the window, between 512
and 4096 tokens; the pictures of the talk are turned into words and left out, for
a summary is written of words and a picture costs a great deal of room; the prompt
carries a guard that says no tool is offered and none is wanted; and the body says
not to read a call of a tool out of the answer, for a model that answers a session
full of tool calls with a tool call of its own, cut short by the window, makes the
server fault where the summary should have stood. An ask that comes back with a
fault is made once more before the reader is told, and a failed ask leaves the
meter of the talk at what the words of the chat cost of themselves, not at the
window that ask filled.

A summary that never came — the server went away, the answer was stopped, or the
model gave no words — leaves its mark empty. The node turns the colour of a
fault, the chat holds every word it had, and no message goes out over a talk that
no longer fits: the card of the mark offers `Write the summary again`, which
starts the summary from the place the chat was cut. A summary that the room cut
short while it was written is kept, and said so, for the room was full when it
was asked for.

The limit is looked at while the model is writing, and not only between two
answers. A model with a tool call bigger than the room it has never stops of
itself: it writes on until the room is gone, which is the very thing the limit was
set aside to prevent. So the stream is cut off at the piece of the answer the talk
came to the limit under — `stream_chat` in `src/llama.rs` is given a hand of the
program's own to stop it, beside the stop that is the reader's. What the model had
written stands in the chat, and the call it was writing is written down after the
summarisation with a `cancelled` mark on it, so the block says what the tool was
pointed at and the reader sees a call that never ran. The words of the call that
had come so far go back to the model as the words of a turn and never as a call,
for a server set to read arguments out of one would fault over words that stop in
the middle of a string; only the head of a long call is sent, bounded by the room
the summarisation left over (`cut_bound`, at most 4000 characters), since the
whole of a file the model was writing out would eat up the room again and have the
model stopped before it had a word. The
model is told the call was cancelled and what it had written, and is asked again,
so the answer goes on from where it stood rather than beginning afresh. A writing
stopped at the limit three times in one answer is writing something no room will
hold, and the reader hears so.

The rule lives in `compaction_wanted` in `src/config.rs`, the work in
`run_handoff`, `maybe_compact` and `make_room` in `src/engine.rs`, the words of a
cut-off call in `cut_words` there too, and the line and the marks in the two
pages.

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
