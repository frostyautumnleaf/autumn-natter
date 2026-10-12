// A picture of the window, made without a display.
//
// This machine has no display server, and Iced 0.14 cannot draw without one.
// The window is therefore drawn here with the software renderer of Iced, and
// the bytes of each picture are written under `uidata/shot`. Nothing is
// started: no server, and no model. The chats and the model names are only
// written on disk, and the empty model files are never read.
//
// Run the pictures with:
//
//     AN_SHOTS=1 cargo test shot -- --test-threads=1
//
// Without `AN_SHOTS` the test does nothing, so it cannot take the data folder
// that the other tests work in.
//
// Each file holds red, green, blue and alpha bytes in a row. The name of the
// file gives the size. A short run of Python turns them into PNG files, which
// can then be looked at.
//
// The pictures cover these scenes, in the pale skin and in the dark one where
// the skin matters: the first screen, a chat with a block of code, a chat with
// every kind of block (a heading, a list, a block of code, the thinking of the
// model, a run of the terminal tool, and a line from the program), the row of
// previews, an answer that is still coming in, a list of the top bar, a window
// with a place of its own open, a narrow window, and the two windows of
// settings at the room they are opened with.

use std::path::PathBuf;

use iced::window;
use iced::{Element, Size, Theme};

use crate::config::{self, Config};
use crate::engine::Engine;
use crate::store::{self, Chat, Message};
use crate::ui::dialog_view;
use crate::ui::main_view;

use super::{App, Msg};

// The room the main window is drawn with.
const WIDE: f32 = 1240.0;
const TALL: f32 = 800.0;
const NARROW: f32 = 800.0;

// The answer of a model, with a block of code in it.
const CODE_ANSWER: &str = "Here is the short way to count the lines of a file.\n\n```rust\nlet n = std::fs::read_to_string(\"notes.txt\")?.lines().count();\nprintln!(\"{n} lines\");\n```\n\nThe read is one call, and the count walks the lines.";

// An answer with every kind of block the window can draw. The thinking of the
// model stands before the answer, and a run of the terminal tool stands after
// it, with a line from the program under both.
const RICH_THINKING: &str = "The reader wants a short list and one program. I will name the quick command first, then show the read in Rust, and check the count with the terminal tool.";

const RICH_ANSWER: &str = "## The short way\n\nOne command does the work for a file, and the count walks the lines as they come.\n\n- `wc -l notes.txt` counts one file.\n- `cat *.rs | wc -l` counts many files at once.\n- A program keeps the count beside the read, so no second read is needed.\n\n```rust\nlet n = std::fs::read_to_string(\"notes.txt\")?.lines().count();\nprintln!(\"{n} lines\");\n```\n\n### When the file is large\n\nRead the file in pieces, and count the line ends as they pass. The whole file never stands in the memory.";

// One message of a picture, with the role and the words it holds.
fn said(role: &str, body: &str, when: i64) -> Message {
    Message {
        role: role.to_string(),
        body: body.to_string(),
        when,
        ..Default::default()
    }
}

// Draw the main window as it stands now, and write the bytes of the picture.
fn picture(app: &App, name: &str) {
    let drawn = main_view::draw(app, window::Id::unique(), app.palette());
    write(app, name, app.width, app.height, drawn);
}

// Draw one window from a picture of it that is already made, and write the
// bytes of it. The main window and the two windows of settings stand here
// alike, because each is drawn by a function of its own.
fn write(app: &App, name: &str, wide: f32, tall: f32, drawn: Element<'_, Msg>) {
    let room = Size::new(wide, tall);
    let pal = app.palette();
    let skin = if app.snap.dark_mode {
        Theme::CatppuccinMocha
    } else {
        Theme::SolarizedLight
    };
    let mut element = drawn;

    let mut renderer = iced::Renderer::Secondary(iced_tiny_skia::Renderer::new(
        iced::Font::DEFAULT,
        iced::Pixels(16.0),
    ));
    let mut tree = iced_winit::core::widget::Tree::new(element.as_widget());
    let place = element.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &iced_winit::core::layout::Limits::new(Size::ZERO, room),
    );
    element.as_widget().draw(
        &tree,
        &mut renderer,
        &skin,
        &iced_winit::core::renderer::Style { text_color: pal.text },
        iced_winit::core::Layout::new(&place),
        iced_winit::core::mouse::Cursor::Unavailable,
        &iced_winit::core::Rectangle::with_size(room),
    );

    let iced::Renderer::Secondary(mut soft) = renderer else {
        panic!("the window was not drawn with the software renderer");
    };
    let view = iced_tiny_skia::graphics::Viewport::with_physical_size(
        Size::new(wide as u32, tall as u32),
        1.0,
    );
    let bytes = iced_tiny_skia::window::compositor::screenshot(&mut soft, &view, pal.bg);

    let hold = PathBuf::from("uidata/shot");
    let _ = std::fs::create_dir_all(&hold);
    let file = hold.join(format!("{name}.{}x{}.rgba", wide as u32, tall as u32));
    std::fs::write(&file, bytes).expect("write the picture");
    println!("picture: {}", file.display());
}

// One chat on disk, with the messages it should hold. The messages are handed
// over whole, so a picture can show a tool run or a line from the program.
fn write_chat(store: &store::Store, title: &str, updated: i64, messages: Vec<Message>) {
    let mut chat = Chat::new("", "");
    chat.meta.title = title.to_string();
    chat.meta.updated = updated;
    chat.messages = messages;
    store.save_chat(&chat);
}

// Three chats on disk, so the top bar and the row of previews have something to
// show. The newest one is the chat the window opens.
fn write_three_chats(store: &store::Store, now: i64) {
    write_chat(
        store,
        "hello",
        now - 60,
        vec![
            said("You", "hello", now - 60),
            said("Assistant", "Hello. What shall we write today?", now - 59),
        ],
    );
    write_chat(
        store,
        "Counting lines",
        now - 3600,
        vec![
            said("You", "How do I count the lines of a file?", now - 3600),
            said("Assistant", CODE_ANSWER, now - 3599),
        ],
    );
    write_chat(
        store,
        "Autumn walk",
        now - 86400,
        vec![
            said("You", "Say something about an autumn walk.", now - 86400),
            said(
                "Assistant",
                "Leaves underfoot, a low sun, and a long way home.",
                now - 86399,
            ),
        ],
    );
}

// One chat with every kind of block the window draws. It is written over the
// chat the window opens, so the picture shows it at once.
fn write_rich_chat(store: &store::Store, now: i64) {
    let mut answer = said("Assistant", RICH_ANSWER, now - 120);
    answer.reasoning = RICH_THINKING.to_string();
    let mut tool = said("Tool", "wc -l notes.txt\nfor one file", now - 118);
    tool.tool = "42 notes.txt".to_string();
    tool.mono = true;
    write_chat(
        store,
        "Ways to count lines",
        now - 60,
        vec![
            said(
                "You",
                "Show me the ways to count lines, and say what you did.",
                now - 180,
            ),
            answer,
            tool,
            said("Note", "The answer used the terminal tool once.", now - 117),
        ],
    );
}

// The state of a window with the room asked for. The chats are written on disk
// first, because the engine reads them when it starts.
fn window(dark: bool, wide: f32, tall: f32, rich: bool) -> App {
    config::set_data_dir(PathBuf::from("uidata/shot"));
    config::ensure_dirs().expect("make the data folders");

    // Two model names, so the box of the top bar has something to choose from.
    // The files are empty and are never read.
    for name in ["Qwen3.5-4B-Q4_K_M.gguf", "Phi4-mini-I8.gguf"] {
        let _ = std::fs::write(config::paths().models.join(name), b"");
    }

    let store = store::Store::new(config::repositories_dir(&Config::default()));
    clear_chats();
    let now = store::now_secs();
    write_three_chats(&store, now);
    if rich {
        write_rich_chat(&store, now);
    }

    let cfg = Config {
        dark_mode: dark,
        ..Config::default()
    };
    let engine = Engine::start(cfg);
    // The first model of the list is picked, so the top bar shows a name.
    let first = engine
        .snapshot()
        .models
        .first()
        .map(|model| model.id())
        .unwrap_or_default();
    if !first.is_empty() {
        engine.select_model(first);
    }
    let looked_for = if rich {
        "Ways to count lines"
    } else {
        "Counting lines"
    };
    let wanted = engine
        .snapshot()
        .chats
        .iter()
        .find(|chat| chat.meta.title == looked_for)
        .map(|chat| chat.meta.id.clone())
        .unwrap_or_default();
    if !wanted.is_empty() {
        engine.open_chat(wanted);
    }

    let mut app = App::new(engine);
    app.width = wide;
    app.height = tall;
    app.pull();
    // The window is looked at a few times, so the card of the chat has grown
    // whole before the picture is taken.
    for _ in 0..6 {
        app.grow();
    }
    app
}

// Take the chats of this place away, so the next window starts with none.
fn clear_chats() {
    let chats = config::paths().chats;
    if let Ok(entries) = std::fs::read_dir(&chats) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

// The state of the first screen. The place holds no chat at all, which is when
// the window shows the mark of the program and the line for this hour.
fn first_screen(dark: bool, wide: f32, tall: f32) -> App {
    config::set_data_dir(PathBuf::from("uidata/shot"));
    config::ensure_dirs().expect("make the data folders");
    clear_chats();
    let cfg = Config {
        dark_mode: dark,
        ..Config::default()
    };
    let engine = Engine::start(cfg);
    let mut app = App::new(engine);
    app.width = wide;
    app.height = tall;
    app.pull();
    app
}

// Take one repository of the pictures away, so a run starts clean and a place
// made for one picture does not stand in the list of the next.
fn remove_repository(name: &str) {
    let repos_dir = config::repositories_dir(&Config::default());
    let Ok(entries) = std::fs::read_dir(&repos_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file = path.join("repo.json");
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        if let Ok(repo) = serde_json::from_str::<store::Repo>(&text) {
            if repo.name == name {
                let _ = std::fs::remove_dir_all(&path);
            }
        }
    }
}

// A window with one place open. The repository box of the top bar shows the
// name of the place, and the chat of the place stands in the carousel.
fn repository_window(dark: bool, wide: f32, tall: f32) -> App {
    let mut app = window(dark, wide, tall, false);
    let store = store::Store::new(config::repositories_dir(&Config::default()));
    // A place of the same name from an older run would refuse the making, so
    // the old one goes first.
    remove_repository("The garden");
    let made = store.create_repo("The garden").expect("make the place");
    let now = store::now_secs();
    let mut chat = Chat::new(&made.id, "");
    chat.meta.title = String::from("Pruning the roses");
    chat.meta.updated = now - 300;
    chat.messages = vec![
        said("You", "When do I prune the roses?", now - 300),
        said(
            "Assistant",
            "After the last frost, and again before the summer heat.",
            now - 299,
        ),
    ];
    store.save_chat(&chat);
    app.engine.select_repository(made.id.clone());
    app.pull();
    for _ in 0..6 {
        app.grow();
    }
    app
}

// The pictures of the main window: the first screen, a chat, a chat with every
// kind of block, the row of previews, an answer that is still being written, a
// list of the top bar, the dark skin, and a narrow window.
#[test]
fn pictures_of_the_window() {
    // One data folder serves the whole run of the tests, and the first test
    // that asks for one fixes it. So the pictures are only made when they are
    // asked for, and the other tests keep the folder they expect.
    if std::env::var("AN_SHOTS").as_deref() != Ok("1") {
        println!("the pictures are not made with the rest of the tests. Ask for them:");
        println!("    AN_SHOTS=1 cargo test shot -- --test-threads=1");
        return;
    }
    // The pictures take the folder of the data, so the other tests wait at
    // the lock while they are made.
    let _turn = config::test_turn();

    picture(&first_screen(false, WIDE, TALL), "home-pale");
    picture(&first_screen(true, WIDE, TALL), "home-dark");

    picture(&window(false, WIDE, TALL, false), "chat-pale");
    picture(&window(true, WIDE, TALL, false), "chat-dark");
    picture(&window(false, NARROW, TALL, false), "chat-narrow");

    picture(&window(false, WIDE, TALL, true), "rich-pale");
    picture(&window(true, WIDE, TALL, true), "rich-dark");

    let mut row = window(false, WIDE, TALL, false);
    row.snap.overview = true;
    row.rows = super::rows_of(&row.snap);
    picture(&row, "previews-pale");

    let mut row = window(true, WIDE, TALL, false);
    row.snap.overview = true;
    row.rows = super::rows_of(&row.snap);
    picture(&row, "previews-dark");

    let mut live = window(false, WIDE, TALL, false);
    live.snap.generating = true;
    live.snap.status = String::from("thinking");
    live.snap.live_text =
        String::from("Counting lines is easy once the file is read in. Watch this space.");
    live.rows = super::rows_of(&live.snap);
    picture(&live, "streaming-pale");

    let mut menu = window(false, WIDE, TALL, false);
    menu.menu = super::Menu::Model;
    picture(&menu, "menu-pale");

    // A word that was written into the box of a new message, as it stands
    // before it is sent.
    let mut draft = window(false, WIDE, TALL, false);
    let words = String::from("Count the lines of every file in the folder");
    draft.draft = iced::widget::text_editor::Content::with_text(&words);
    draft.draft_text = words;
    picture(&draft, "draft-pale");

    // A place of its own, with the repository box of the top bar and, in one
    // of the pictures, the list under it.
    picture(&repository_window(false, WIDE, TALL), "repo-pale");
    picture(&repository_window(true, WIDE, TALL), "repo-dark");
    let mut repo_menu = repository_window(false, WIDE, TALL);
    repo_menu.menu = super::Menu::Repository;
    picture(&repo_menu, "repo-menu-pale");
    // The place goes away again, so the pictures that follow see no
    // repository at all.
    remove_repository("The garden");
}

// The pictures of the two windows of settings: the places of the program, and
// one model with its command line. They are drawn with the same software
// renderer as the main window, in the pale skin and in the dark one, and with
// the room the windows are really opened with.
#[test]
fn pictures_of_the_settings_windows() {
    if std::env::var("AN_SHOTS").as_deref() != Ok("1") {
        return;
    }
    let _turn = config::test_turn();
    for dark in [false, true] {
        let skin = if dark { "dark" } else { "pale" };
        let app = window(dark, WIDE, TALL, false);
        let pal = app.palette();
        let room = window::Id::unique();

        let form = app.settings_form();
        let page = dialog_view::program(&app, room, &form, pal);
        let (wide, tall) = super::PROGRAM_ROOM;
        write(&app, &format!("settings-program-{skin}"), wide, tall, page);

        let model = app.snap.models.first();
        let id = model.map(|m| m.id()).unwrap_or_default();
        let name = model.map(|m| m.name.clone()).unwrap_or_default();
        let mform = app.model_form(&id, &name);
        let page = dialog_view::model(&app, room, &mform, pal);
        let (wide, tall) = super::MODEL_ROOM;
        write(&app, &format!("settings-model-{skin}"), wide, tall, page);
    }
}
