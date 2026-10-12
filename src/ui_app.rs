// The desktop window. It shows a snapshot of the engine and sends the actions of
// the user back to it.
//
// The window never waits for the model. A timer reads the state about twenty
// times a second, so an answer appears while it is written. The window draws
// nothing by itself: the files under `ui` do that from the snapshot.
//
// Three windows exist. The main window holds the chats. The other two hold a
// form each: one for the program, and one for a model.

use std::time::{Duration, Instant};

use iced::widget::text_editor::{self, Action as Edit};
use iced::{window, Element, Subscription, Task, Theme};

use crate::config::{Config, ModelSettings};
use crate::engine::{Engine, Snap};
use crate::gpu::Gpu;
use crate::llama;
use crate::store;
use crate::ui::message_view;
use crate::ui::palette::Pal;
use crate::ui::parts;
use crate::ui::shape::Shape;
use crate::ui::{dialog_view, main_view};

// How often the window looks at the engine.
const TICK: Duration = Duration::from_millis(50);

// How long the row of previews takes to settle on a chat.
const SETTLE: Duration = Duration::from_millis(260);

// How long the pointer must stay on the mark of a control before its help
// bubble comes. A bubble that comes at once gets in the way of the mouse.
const TIP_WAIT: Duration = Duration::from_millis(500);

// The room a window of settings was opened with.
const PROGRAM_ROOM: (f32, f32) = (760.0, 700.0);
const MODEL_ROOM: (f32, f32) = (760.0, 760.0);

// Which list the top bar holds open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    None,
    Model,
    Repository,
}

// One chat of the carousel, as the window needs it.
pub struct Card {
    pub id: String,
    pub title: String,
    pub when: String,
    pub preview: String,
    pub repo: String,
}

// What the window of the program holds while it stands open.
pub struct SettingsForm {
    pub llama_dir: String,
    pub models_dir: String,
    pub repositories_dir: String,
    pub port: String,
    pub upnp: bool,
    pub dark: bool,
    // The name of a place that is not made yet.
    pub new_repo: String,
}

// What the window of one model holds while it stands open.
pub struct ModelForm {
    // The whole file of the model, which is also its id.
    pub id: String,
    pub name: String,
    // GGUF or Safetensors.
    pub kind: String,
    pub held: ModelSettings,
    // The three lists of many choices. The words are what the reader sees, and
    // the number is what the settings of the model keep.
    pub kinds: Vec<parts::Choice>,
    pub loads: Vec<parts::Choice>,
    pub specs: Vec<parts::Choice>,
    // The devices of the machine, before the switches of the form are read.
    pub gpus: Vec<Gpu>,
    // The command line the next answer will use.
    pub preview: String,
    // The two boxes of many lines. They hold their own words, so typing in one
    // of them does not send a message of its own.
    pub template: text_editor::Content,
    pub extra: text_editor::Content,
}

// A help bubble that was asked for. It comes into view a while after the
// pointer stays on the mark of a control, next to where the pointer stands.
pub struct Tip {
    pub note: String,
    pub asked: Instant,
}

impl Tip {
    // True when the pointer has stayed long enough for the bubble to show.
    pub fn shown(&self) -> bool {
        self.asked.elapsed() >= TIP_WAIT
    }
}

// Everything a click, a key or a tick can say.
#[derive(Clone, Debug)]
pub enum Msg {
    // The window looked at the engine.
    Tick,
    // A window changed size.
    Sized(window::Id, f32, f32),
    // A key was let go, for the row of previews.
    KeyUp(String),
    // The pointer moved in a window. A help bubble stands near it.
    Pointer(window::Id, f32, f32),
    // A window asked to be put away.
    WindowClose(window::Id),
    // A window was put away, by the program or by the reader.
    WindowClosed(window::Id),
    // Something was done in the box for a new message.
    Draft(Edit),
    Send,
    NewChat,
    // The pointer in the row of previews.
    RowAt(f32),
    RowGrab,
    RowWheel(f32),
    RowRelease,
    // The three marks at the foot of the box.
    Code(bool),
    Sandbox(bool),
    Thinking,
    // The two lists of the top bar.
    MenuOpen(Menu),
    PickModel(usize),
    PickRepository(usize),
    // One message was pointed at.
    RowLit(String),
    RowDim,
    // A help bubble was asked for, or given up.
    TipOn(String),
    TipOff,
    // The two windows of settings.
    Settings,
    // The window of one model, picked from the list of models by its place.
    ModelSettings(usize),
    LlamaDir(String),
    ModelsDir(String),
    ReposDir(String),
    Port(String),
    Upnp(bool),
    Dark(bool),
    // The words typed for the name of a new place, and the ask to make it.
    NewRepoText(String),
    NewRepository(String),
    Ctx(i64),
    KvKind(usize),
    // The file of the small model that guesses ahead, and the words the server
    // adds when the room for thinking is gone.
    DraftModel(String),
    ReasonMessage(String),
    SpecKind(usize),
    DraftTokens(i64),
    Parallel(i64),
    Batch(i64),
    Ubatch(i64),
    Vision(bool),
    Budget(i64),
    Template(Edit),
    Extra(Edit),
    LoadKind(usize),
    Memory(f64),
    CardOn(usize),
    // The two buttons of each window of settings. Each window has its own pair,
    // so a button of one window cannot put the other one away.
    SaveSettings,
    CloseSettings,
    SaveModel,
    CloseModel,
}

// The state of the window.
pub struct App {
    pub engine: Engine,
    pub snap: Snap,
    // The room the window has. The whole window is drawn from these two.
    pub width: f32,
    pub height: f32,
    // The box a message is written in, and the words it holds.
    pub draft: text_editor::Content,
    pub draft_text: String,
    // What the window draws. These follow the snapshot.
    pub rows: Vec<message_view::Row>,
    pub cards: Vec<Card>,
    // The chat the card was grown for. Another chat here starts the growth
    // again, so a chat is drawn in from the top down when it is opened.
    pub card_chat: String,
    pub rev: u64,
    pub structure: u64,
    pub chats: String,
    // The top bar and the pointer.
    pub menu: Menu,
    pub hovered: Option<String>,
    pub tip: Option<Tip>,
    // Where the pointer stands in each window, and how much room each window
    // was given. Both are needed to place a help bubble inside its own window.
    pub pointer: Option<(window::Id, f32, f32)>,
    pub rooms: Vec<(window::Id, f32, f32)>,
    // The last place the pointer stood along the row of previews.
    pub at: f32,
    // The box of a new message, so it can be given the keys.
    pub draft_id: iced::widget::Id,
    // Motion. The window moves things by hand, so it can be calm about it.
    pub pulse: usize,
    pub spin: f32,
    // The speed a flick of the row left behind, in room of the window for every
    // look at the engine. The row keeps gliding with it, and loses a little of
    // it each time.
    pub flick: f32,
    pub grab: Option<f32>,
    pub dragged: bool,
    pub turn: f32,
    pub open_t: f32,
    pub opened: Option<Instant>,
    // True while no chat is open, so the mark and the hour line show.
    pub greet: bool,
    // The two windows of settings.
    pub settings: Option<SettingsForm>,
    pub settings_window: Option<window::Id>,
    pub model: Option<ModelForm>,
    pub model_window: Option<window::Id>,
}

impl App {
    // The state, from an engine that was started with this config.
    fn new(engine: Engine) -> App {
        let snap = engine.snapshot();
        let mut app = App {
            engine,
            snap,
            width: 1180.0,
            height: 760.0,
            draft: text_editor::Content::new(),
            draft_text: String::new(),
            rows: Vec::new(),
            cards: Vec::new(),
            card_chat: String::new(),
            rev: 0,
            structure: 0,
            chats: String::new(),
            menu: Menu::None,
            hovered: None,
            tip: None,
            pointer: None,
            rooms: Vec::new(),
            at: 0.0,
            draft_id: iced::widget::Id::unique(),
            pulse: 0,
            spin: 0.0,
            flick: 0.0,
            grab: None,
            dragged: false,
            // The first picture of the window shows the chat whole. The growth
            // from the top down is for a chat that is opened later.
            turn: 1.0,
            open_t: 1.0,
            opened: None,
            greet: false,
            settings: None,
            settings_window: None,
            model: None,
            model_window: None,
        };
        app.pull();
        app
    }

    // The places and the sizes of the window, at the size it has now.
    pub fn shape(&self) -> Shape {
        Shape::new(self.width, self.height, self.snap.home)
    }

    // True for the window that holds the chats. The other two hold a form.
    pub fn is_main(&self, window: window::Id) -> bool {
        Some(window) != self.settings_window && Some(window) != self.model_window
    }

    // The room one window has now. A window of settings keeps its own size in
    // the list, because its size is not the size of the main window.
    pub fn room_of(&self, window: window::Id) -> (f32, f32) {
        if self.is_main(window) {
            return (self.width, self.height);
        }
        for (held, wide, tall) in self.rooms.iter() {
            if *held == window {
                return (*wide, *tall);
            }
        }
        if self.model_window == Some(window) {
            return MODEL_ROOM;
        }
        PROGRAM_ROOM
    }

    // Where the pointer stands in one window, when it stands there.
    pub fn pointer_at(&self, window: window::Id) -> Option<(f32, f32)> {
        self.pointer.filter(|(held, _, _)| *held == window).map(|(_, x, y)| (x, y))
    }

    // The colours of the window.
    fn palette(&self) -> Pal {
        Pal::new(self.snap.dark_mode)
    }

    // Read the engine, and rebuild what moved. A list is built again only when
    // its own count moved, so an answer that streams does not rebuild the
    // carousel.
    fn pull(&mut self) {
        let fresh = self.engine.snapshot();
        let moved = fresh.rev != self.rev || fresh.structure_rev != self.structure;
        // Another chat in use means a chat was opened, so the card is grown
        // again from the top down.
        if fresh.chat_id != self.card_chat {
            self.card_chat = fresh.chat_id.clone();
            self.turn = 0.0;
        }
        self.rev = fresh.rev;
        self.snap = fresh;
        if self.snap.structure_rev != self.structure || self.cards.is_empty() {
            self.cards = cards_of(&self.snap);
            self.structure = self.snap.structure_rev;
        }
        let key = chat_key(&self.snap);
        if key != self.chats {
            self.chats = key;
            self.cards = cards_of(&self.snap);
        }
        if moved || self.rows.is_empty() {
            self.rows = rows_of(&self.snap);
        }
        self.greet = self.snap.home;
    }

    // Let the card of the chat in use grow a step further. It takes about as
    // long as the row of previews takes to settle, and it stops at whole.
    fn grow(&mut self) {
        if self.snap.home {
            self.turn = 0.0;
            return;
        }
        if self.turn < 1.0 {
            self.turn = (self.turn + 0.2).min(1.0);
        }
    }

    // Move the row of previews on by what a flick left behind, then let it come
    // to rest on the nearest chat. The row is moved and never built again, so a
    // card that was built again has no earlier place to slide from.
    fn glide(&mut self) {
        if self.grab.is_some() || !self.snap.overview {
            return;
        }
        if self.flick.abs() > 0.5 {
            self.spin += self.flick;
            self.flick *= 0.82;
            return;
        }
        self.flick = 0.0;
        let shape = self.shape();
        let rest = shape.align(self.spin);
        let step = (rest - self.spin) * 0.35;
        if step.abs() < 0.4 {
            self.spin = rest;
            return;
        }
        self.spin += step;
    }

    // The words of the box, kept beside it so a half written ask is never lost.
    fn draft_now(&mut self) {
        self.draft_text = self.draft.text();
    }

    // Send what the box holds, and leave the box empty.
    fn send(&mut self) {
        let words = self.draft_text.trim().to_string();
        if words.is_empty() || self.snap.generating {
            return;
        }
        self.engine.prompt(words);
        self.draft = text_editor::Content::new();
        self.draft_text = String::new();
        self.pull();
    }

    // The list of settings of the program, filled from what the engine holds.
    fn settings_form(&self) -> SettingsForm {
        let cfg = self.engine.config();
        SettingsForm {
            llama_dir: cfg.llama_dir.clone(),
            models_dir: cfg.models_dir.clone(),
            repositories_dir: cfg.repositories_dir.clone(),
            port: if cfg.remote_port == 0 {
                String::new()
            } else {
                cfg.remote_port.to_string()
            },
            upnp: cfg.upnp_enabled,
            dark: cfg.dark_mode,
            new_repo: String::new(),
        }
    }

    // The list of settings of one model, filled from what the engine holds.
    fn model_form(&self, id: &str, name: &str) -> ModelForm {
        let (held, devices) = self.engine.model_settings(id);
        let kind = self
            .snap
            .models
            .iter()
            .find(|one| one.id() == id)
            .map(|one| one.kind.to_string())
            .unwrap_or_default();
        let mut form = ModelForm {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            template: text_editor::Content::with_text(&held.chat_template),
            extra: text_editor::Content::with_text(&held.extra_args),
            held,
            kinds: parts::choices(llama::KV_TYPES),
            loads: parts::choices(llama::LOAD_MODES),
            specs: parts::choices(llama::SPEC_TYPES),
            gpus: devices,
            preview: String::new(),
        };
        form.preview = preview_of(self, &form);
        form
    }

    // The command line of the form, read again after a field moved.
    fn refresh_model(&mut self) {
        let Some(form) = self.model.as_ref() else {
            return;
        };
        let made = preview_of(self, form);
        if let Some(form) = self.model.as_mut() {
            form.preview = made;
        }
    }
}

impl Drop for App {
    // The window is going away, so the server must go with it. A llama-server
    // left behind would hold the model in the memory of the machine.
    fn drop(&mut self) {
        self.engine.shutdown();
    }
}

// The command line the next answer will use, as the window of one model shows
// it. It helps to see it here, because a wrong extra argument is then easy to
// find. Nothing is started: only the arguments are built.
fn preview_of(app: &App, form: &ModelForm) -> String {
    let Some(model) = app.snap.models.iter().find(|one| one.id() == form.id) else {
        return String::new();
    };
    // The switches of the form win over the switches the model was saved with.
    let mut devices = form.gpus.clone();
    for (place, flag) in form.held.gpu_enabled.iter().enumerate() {
        if let Some(card) = devices.get_mut(place) {
            card.enabled = *flag;
        }
    }
    let mut held = form.held.clone();
    held.chat_template = form.template.text();
    held.extra_args = form.extra.text();
    let mut notes = Vec::new();
    let args = llama::build_args(
        std::path::Path::new(""),
        model,
        &held,
        &devices,
        0,
        app.snap.code_enabled,
        app.snap.thinking_mode.as_str(),
        &mut notes,
    );
    let mut line = String::from("llama-server ");
    for arg in args {
        if arg.contains(' ') {
            line.push_str(&format!("\"{arg}\""));
        } else {
            line.push_str(&arg);
        }
        line.push(' ');
    }
    for note in notes {
        line.push('\n');
        line.push_str(&note);
    }
    line.trim_end().to_string()
}

// The chats of this place, as the carousel and the row of previews want them.
fn cards_of(snap: &Snap) -> Vec<Card> {
    snap.chats
        .iter()
        .map(|chat| Card {
            id: chat.meta.id.clone(),
            title: chat.meta.title.clone(),
            when: store::format_when(chat.meta.updated),
            preview: store::preview(&chat.messages),
            repo: repo_label(snap, &chat.meta.repo_id),
        })
        .collect()
}

// One text that says which chats a place holds, and in which order.
fn chat_key(snap: &Snap) -> String {
    let mut key = String::new();
    for chat in &snap.chats {
        key.push_str(&chat.meta.id);
        key.push('|');
        key.push_str(&chat.meta.title);
        key.push('|');
    }
    key
}

// The name of a repository, written out. A chat with no repository says so.
fn repo_label(snap: &Snap, id: &str) -> String {
    if id.is_empty() {
        return String::from("loose");
    }
    for repo in &snap.repositories {
        if repo.id == id {
            return repo.name.clone();
        }
    }
    String::from("loose")
}

// The messages of the open chat, as the window shows them. While an answer comes
// in, the words the model wrote just now get a line of their own.
fn rows_of(snap: &Snap) -> Vec<message_view::Row> {
    let mut rows: Vec<message_view::Row> = Vec::with_capacity(snap.messages.len() + 1);
    for message in &snap.messages {
        rows.push(row_of(message));
    }
    if snap.generating && !(snap.live_text.is_empty() && snap.live_reasoning.is_empty()) {
        rows.push(message_view::Row {
            id: String::from("live"),
            role: String::from("Assistant"),
            body: snap.live_text.clone(),
            cmd: String::new(),
            tool: String::new(),
            mono: false,
            failed: false,
            when: String::new(),
            blocks: crate::markdown::parse(&snap.live_text),
            reasoning: snap.live_reasoning.clone(),
        });
    }
    rows
}

// One message as the window shows it.
fn row_of(message: &store::Message) -> message_view::Row {
    // The first line of a tool command stands in the head of the tool panel.
    let cmd = if message.role == "Tool" {
        message.body.lines().next().unwrap_or("").to_string()
    } else {
        String::new()
    };
    message_view::Row {
        id: message.id.clone(),
        role: message.role.clone(),
        body: message.body.clone(),
        cmd,
        tool: message.tool.clone(),
        mono: message.mono,
        failed: message.failed,
        when: store::format_clock(message.when),
        blocks: crate::markdown::parse(&message.body),
        reasoning: message.reasoning.clone(),
    }
}

// The one place that changes the state of the window. Nothing here waits for the
// model: a command goes to the engine and the next tick brings the answer.
fn update(app: &mut App, message: Msg) -> Task<Msg> {
    // True when a field of the window of one model moved. The command line at
    // the foot of that window is read again after such a move.
    let mut touched = matches!(
        &message,
        Msg::Ctx(_)
            | Msg::KvKind(_)
            | Msg::DraftModel(_)
            | Msg::ReasonMessage(_)
            | Msg::SpecKind(_)
            | Msg::DraftTokens(_)
            | Msg::Parallel(_)
            | Msg::Batch(_)
            | Msg::Ubatch(_)
            | Msg::Vision(_)
            | Msg::Budget(_)
            | Msg::LoadKind(_)
            | Msg::Memory(_)
            | Msg::CardOn(_)
            | Msg::Template(_)
            | Msg::Extra(_)
    );
    match message {
        Msg::Tick => {
            app.pulse = (app.pulse + 1) % 3;
            if let Some(since) = app.opened {
                let gone = since.elapsed().as_secs_f32();
                app.open_t = (gone / (SETTLE.as_secs_f32())).clamp(0.0, 1.0);
            }
            app.grow();
            app.glide();
            app.pull();
        }
        Msg::Sized(which, wide, tall) => {
            if wide <= 1.0 || tall <= 1.0 {
                return Task::none();
            }
            if app.is_main(which) {
                app.width = wide;
                app.height = tall;
                return Task::none();
            }
            // A window of settings keeps its own room, so a help bubble of that
            // window stays inside it.
            let held = app.rooms.iter_mut().find(|(kept, _, _)| *kept == which);
            if let Some(kept) = held {
                *kept = (which, wide, tall);
            } else {
                app.rooms.push((which, wide, tall));
            }
        }
        Msg::Pointer(which, x, y) => {
            app.pointer = Some((which, x, y));
            if app.is_main(which) && app.snap.overview {
                app.at = x;
            }
        }
        Msg::WindowClose(which) => {
            // The main window asked to be put away. The engine is told to stop,
            // so the model leaves the memory before the window goes.
            if app.is_main(which) {
                app.engine.shutdown();
                return iced::exit();
            }
            // A window of settings asked to be put away. It is closed here, and
            // its form is forgotten when the close comes back as a message.
            return window::close(which);
        }
        Msg::WindowClosed(which) => {
            if app.is_main(which) {
                // The main window is gone. The windows of settings go with it,
                // and the engine is told to stop.
                app.engine.shutdown();
                let mut jobs = vec![iced::exit()];
                for held in [app.settings_window, app.model_window].into_iter().flatten() {
                    jobs.push(window::close(held));
                }
                app.settings = None;
                app.model = None;
                app.settings_window = None;
                app.model_window = None;
                return Task::batch(jobs);
            }
            // One window of settings is gone, so its form is forgotten. The form
            // of the other one stays, because that window is still open.
            if app.settings_window == Some(which) {
                app.settings = None;
                app.settings_window = None;
            }
            if app.model_window == Some(which) {
                app.model = None;
                app.model_window = None;
            }
        }
        Msg::KeyUp(name) => {
            if name == "Escape" {
                // Escape puts the first thing that is in the way away. An answer
                // that comes in is stopped, because it is the loudest thing the
                // window does.
                if app.menu != Menu::None {
                    app.menu = Menu::None;
                } else if app.snap.generating {
                    app.engine.cancel();
                } else if app.snap.overview {
                    app.engine.set_overview(false);
                    app.spin = 0.0;
                }
                return Task::none();
            }
            if name == "Alt" {
                // The Alt key puts the row of previews up, and puts it away
                // again. It is read when it is let go, so the row does not jump
                // while another key is held with Alt.
                let on = !app.snap.overview;
                app.engine.set_overview(on);
                app.spin = 0.0;
                if on {
                    app.opened = Some(Instant::now());
                }
                return Task::none();
            }
            if name == "Enter" {
                if app.snap.overview {
                    return pick_middle(app);
                }
                return Task::none();
            }
            if !app.snap.overview {
                return Task::none();
            }
            match name.as_str() {
                "ArrowLeft" | "a" | "A" => app.engine.roll(-1),
                "ArrowRight" | "d" | "D" => app.engine.roll(1),
                _ => {}
            }
            app.opened = Some(Instant::now());
        }
        // The action of the box is performed on the words it holds, so a
        // letter is written into them and kept for the next frame. The words
        // are read again after an action that changed them, so send knows
        // whether there is something to send.
        Msg::Draft(edit) => {
            let edited = edit.is_edit();
            app.draft.perform(edit);
            if edited {
                app.draft_now();
            }
        }
        Msg::Send => app.send(),
        Msg::NewChat => {
            app.engine.new_chat();
            app.opened = Some(Instant::now());
            // A new chat is written in at once, so the box takes the keys.
            return focus_draft(app);
        }
        Msg::RowAt(where_) => {
            app.at = where_;
            if let Some(before) = app.grab {
                let step = where_ - before;
                if step.abs() > 2.0 {
                    app.dragged = true;
                }
                app.spin += step;
                // The last move of the hand is the speed the row keeps when the
                // finger leaves it.
                app.flick = step;
                app.grab = Some(where_);
            }
        }
        Msg::RowGrab => {
            app.grab = Some(app.at);
            app.dragged = false;
            app.flick = 0.0;
        }
        Msg::RowWheel(step) => {
            app.spin += step;
        }
        Msg::RowRelease => {
            let stood = app.grab.take().unwrap_or(0.0);
            if !app.dragged && app.snap.overview {
                // A press without a move opens the preview the pointer stood on.
                let shape = app.shape();
                let first = app.snap.chat_index as i64 - 3;
                for place in 0..6i64 {
                    let index = first + place;
                    if index < 0 || index as usize >= app.snap.chats.len() {
                        continue;
                    }
                    let x = shape.prev_x(index as f32, app.snap.chat_index, app.spin);
                    if stood >= x && stood <= x + shape.prev_width {
                        let id = app.cards[index as usize].id.clone();
                        app.engine.open_chat(id);
                        app.engine.set_overview(false);
                        app.spin = 0.0;
                        app.opened = Some(Instant::now());
                        return focus_draft(app);
                    }
                }
            }
            app.dragged = false;
        }
        Msg::Code(on) => app.engine.set_code(on),
        Msg::Sandbox(on) => app.engine.set_sandbox(on),
        Msg::Thinking => app.engine.cycle_thinking(),
        Msg::MenuOpen(which) => {
            app.menu = if app.menu == which { Menu::None } else { which };
        }
        Msg::PickModel(which) => {
            if let Some(model) = app.snap.models.get(which) {
                app.engine.select_model(model.id());
            }
            app.menu = Menu::None;
        }
        Msg::PickRepository(which) => {
            // The first place in the list means "no repository". A window of
            // settings lists every repository on disk, so the list is read from
            // the engine: the snapshot stops at the five most recent ones.
            if which == 0 {
                app.engine.select_repository(String::new());
            } else {
                let places = app.engine.repositories();
                if let Some(repo) = places.get(which - 1) {
                    let id = repo.id.clone();
                    app.engine.select_repository(id);
                }
            }
            app.menu = Menu::None;
        }
        Msg::RowLit(id) => app.hovered = Some(id),
        Msg::RowDim => app.hovered = None,
        Msg::TipOn(note) => {
            // The bubble stands where the pointer stands, and comes half a
            // second later. The window puts a new bubble up in the place of the
            // old one, so the ask that came last is the one that shows.
            app.tip = Some(Tip { note, asked: Instant::now() });
        }
        Msg::TipOff => app.tip = None,
        Msg::Settings => {
            // One window of the program at a time. A second window for the same
            // form would leave the first one behind with no record of it, and
            // putting that one away would then look like the main window going.
            if app.settings_window.is_some() {
                return Task::none();
            }
            if app.settings.is_none() {
                app.settings = Some(app.settings_form());
            }
            let card = window::Settings {
                size: iced::Size::new(PROGRAM_ROOM.0, PROGRAM_ROOM.1),
                min_size: Some(iced::Size::new(520.0, 420.0)),
                // Only the main window may end the program. A form that is put
                // away must leave the chats alone.
                exit_on_close_request: false,
                ..window::Settings::default()
            };
            let (id, task) = window::open(card);
            app.settings_window = Some(id);
            return task.map(|_| Msg::TipOff);
        }
        Msg::ModelSettings(place) => {
            // One window of one model at a time, for the same reason as above.
            if app.model_window.is_some() {
                app.menu = Menu::None;
                return Task::none();
            }
            let Some(model) = app.snap.models.get(place) else {
                return Task::none();
            };
            let id = model.id();
            let name = model.name.clone();
            app.model = Some(app.model_form(&id, &name));
            app.menu = Menu::None;
            let card = window::Settings {
                size: iced::Size::new(MODEL_ROOM.0, MODEL_ROOM.1),
                min_size: Some(iced::Size::new(560.0, 420.0)),
                exit_on_close_request: false,
                ..window::Settings::default()
            };
            let (id, task) = window::open(card);
            app.model_window = Some(id);
            return task.map(|_| Msg::TipOff);
        }
        // The window of the program.
        Msg::LlamaDir(words) => {
            if let Some(form) = &mut app.settings {
                form.llama_dir = words;
            }
        }
        Msg::ModelsDir(words) => {
            if let Some(form) = &mut app.settings {
                form.models_dir = words;
            }
        }
        Msg::ReposDir(words) => {
            if let Some(form) = &mut app.settings {
                form.repositories_dir = words;
            }
        }
        Msg::Port(words) => {
            if let Some(form) = &mut app.settings {
                form.port = words;
            }
        }
        Msg::Upnp(on) => {
            if let Some(form) = &mut app.settings {
                form.upnp = on;
            }
        }
        Msg::NewRepoText(words) => {
            if let Some(form) = &mut app.settings {
                form.new_repo = words;
            }
        }
        Msg::NewRepository(words) => {
            app.engine.create_repository(words);
            // The name is used up, so the field is empty again and a second
            // press of the mark makes a second place, not a copy of the first.
            if let Some(form) = &mut app.settings {
                form.new_repo.clear();
            }
        }
        Msg::Dark(on) => {
            if let Some(form) = &mut app.settings {
                form.dark = on;
            }
            app.engine.set_dark_mode(on);
        }
        // The window of one model.
        Msg::Ctx(value) => {
            if let Some(form) = &mut app.model {
                form.held.ctx_size = value.max(1) as u32;
            }
        }
        Msg::KvKind(which) => {
            if let Some(form) = &mut app.model {
                form.held.kv_index = which;
            }
        }
        Msg::DraftModel(words) => {
            if let Some(form) = &mut app.model {
                form.held.draft_model = words;
            }
        }
        Msg::ReasonMessage(words) => {
            if let Some(form) = &mut app.model {
                form.held.reasoning_message = words;
            }
        }
        Msg::SpecKind(which) => {
            if let Some(form) = &mut app.model {
                form.held.spec_index = which;
            }
        }
        Msg::DraftTokens(value) => {
            if let Some(form) = &mut app.model {
                form.held.draft_tokens = value.max(1) as u32;
            }
        }
        Msg::Parallel(value) => {
            if let Some(form) = &mut app.model {
                form.held.parallel = value.max(1) as u32;
            }
        }
        Msg::Batch(value) => {
            if let Some(form) = &mut app.model {
                form.held.batch_size = value.max(1) as u32;
            }
        }
        Msg::Ubatch(value) => {
            if let Some(form) = &mut app.model {
                form.held.ubatch_size = value.max(1) as u32;
            }
        }
        Msg::Vision(on) => {
            if let Some(form) = &mut app.model {
                form.held.vision = on;
            }
        }
        Msg::Budget(value) => {
            if let Some(form) = &mut app.model {
                form.held.reasoning_budget = value;
            }
        }
        // The two boxes of many lines keep their own words until the form is
        // saved. The action is performed on the words, so a letter is kept
        // for the next frame, and the command line at the foot of the window
        // is read again after an action that changed the words.
        Msg::Template(edit) => {
            let edited = edit.is_edit();
            if let Some(form) = &mut app.model {
                form.template.perform(edit);
            }
            touched = edited;
        }
        Msg::Extra(edit) => {
            let edited = edit.is_edit();
            if let Some(form) = &mut app.model {
                form.extra.perform(edit);
            }
            touched = edited;
        }
        Msg::LoadKind(which) => {
            if let Some(form) = &mut app.model {
                form.held.load_index = which;
            }
        }
        Msg::Memory(value) => {
            if let Some(form) = &mut app.model {
                form.held.vram_budget = value.max(0.0);
            }
        }
        Msg::CardOn(which) => {
            if let Some(form) = &mut app.model {
                if which < form.held.gpu_enabled.len() {
                    form.held.gpu_enabled[which] = !form.held.gpu_enabled[which];
                }
            }
        }
        Msg::SaveSettings => {
            if let Some(form) = &app.settings {
                let mut cfg = app.engine.config();
                cfg.llama_dir = form.llama_dir.clone();
                cfg.models_dir = form.models_dir.clone();
                cfg.repositories_dir = form.repositories_dir.clone();
                cfg.dark_mode = form.dark;
                cfg.upnp_enabled = form.upnp;
                cfg.remote_port = form.port.parse::<u16>().unwrap_or(0);
                app.engine.save_settings(cfg);
            }
            app.settings = None;
            return close_window(app.settings_window.take());
        }
        Msg::SaveModel => {
            if let Some(form) = &app.model {
                let mut held = form.held.clone();
                held.chat_template = form.template.text();
                held.extra_args = form.extra.text();
                app.engine.save_model_settings(form.id.clone(), held);
            }
            app.model = None;
            return close_window(app.model_window.take());
        }
        // The form is left behind, and the window with it. What was written in
        // the other window stays in its own window.
        Msg::CloseSettings => {
            app.settings = None;
            return close_window(app.settings_window.take());
        }
        Msg::CloseModel => {
            app.model = None;
            return close_window(app.model_window.take());
        }
    }
    if touched {
        app.refresh_model();
    }
    Task::none()
}

// Open the chat that stands in the middle of the row of previews, and put the
// row away. The keys of the row answer for it, and so does Return.
fn pick_middle(app: &mut App) -> Task<Msg> {
    let Some(card) = app.cards.get(app.snap.chat_index) else {
        return Task::none();
    };
    let id = card.id.clone();
    app.engine.open_chat(id);
    app.engine.set_overview(false);
    app.spin = 0.0;
    app.opened = Some(Instant::now());
    focus_draft(app)
}

// Give the box for a new message the keys, so the first word can be written at
// once. The window was made with the box empty and the caret waiting.
fn focus_draft(app: &mut App) -> Task<Msg> {
    iced::widget::operation::focus(app.draft_id.clone())
}

// Put one window of settings away. Nothing is asked for when the window was
// never opened, because the form that went with it is already gone.
fn close_window(which: Option<window::Id>) -> Task<Msg> {
    match which {
        Some(id) => window::close(id),
        None => Task::none(),
    }
}

// What a key was let go, as a short word. The row of previews answers to these.
// The written form of a named key cannot be used here, because it reads
// "Named(Escape)". The words of one_word below are what the window compares.
fn key_name(key: &iced::keyboard::Key) -> String {
    use iced::keyboard::Key;

    match key {
        Key::Character(words) => words.to_string(),
        Key::Named(named) => String::from(one_word(*named)),
        // A key the window does not know is "Other", and does nothing.
        _ => String::from("Other"),
    }
}

// One named key, as one word. Only the keys the window answers to are named.
// The rest are "Other", so they do nothing.
fn one_word(named: iced::keyboard::key::Named) -> &'static str {
    use iced::keyboard::key::Named;

    match named {
        Named::Escape => "Escape",
        Named::Enter => "Enter",
        Named::Alt => "Alt",
        Named::ArrowLeft => "ArrowLeft",
        Named::ArrowRight => "ArrowRight",
        Named::ArrowUp => "ArrowUp",
        Named::ArrowDown => "ArrowDown",
        // The names a keyboard may give are not all known ahead of time, so the
        // rest are "Other" and do nothing.
        _ => "Other",
    }
}

// The runner joins the state of the window to the windows themselves. It holds
// the engine until the window is ready for it.
pub struct Runner {
    engine: std::cell::RefCell<Option<Engine>>,
}

impl iced_winit::program::Program for Runner {
    type State = App;
    type Message = Msg;
    type Theme = Theme;
    type Renderer = iced::Renderer;
    type Executor = iced::executor::Default;

    fn name() -> &'static str {
        "autumn natter"
    }

    fn settings(&self) -> iced::Settings {
        iced::Settings::default()
    }

    fn window(&self) -> Option<window::Settings> {
        Some(window::Settings {
            size: iced::Size::new(1180.0, 760.0),
            // A window narrower than this cannot hold the top bar in order, so
            // the desktop is not asked to make one.
            min_size: Some(iced::Size::new(620.0, 420.0)),
            ..window::Settings::default()
        })
    }

    fn boot(&self) -> (App, Task<Msg>) {
        let engine = self.engine.borrow_mut().take().expect("the engine is started once");
        let mut app = App::new(engine);
        // The box for a new message has the keys as soon as the window is up,
        // so the first word can be written without a click first.
        let keys = focus_draft(&mut app);
        (app, keys)
    }

    fn update(&self, state: &mut App, message: Msg) -> Task<Msg> {
        update(state, message)
    }

    fn view<'a>(&self, state: &'a App, window: window::Id) -> Element<'a, Msg> {
        let pal = state.palette();
        if state.settings_window == Some(window) {
            return match &state.settings {
                Some(form) => dialog_view::program(state, window, form, pal),
                None => dialog_view::blank(pal),
            };
        }
        if state.model_window == Some(window) {
            return match &state.model {
                Some(form) => dialog_view::model(state, window, form, pal),
                None => dialog_view::blank(pal),
            };
        }
        main_view::draw(state, window, pal)
    }

    fn title(&self, state: &App, window: window::Id) -> String {
        if state.settings_window == Some(window) {
            return String::from("Autumn Natter - Settings");
        }
        if state.model_window == Some(window) {
            let name = state
                .model
                .as_ref()
                .map(|form| form.name.clone())
                .unwrap_or_else(|| state.snap.model_name.clone());
            return format!("Autumn Natter - {name}");
        }
        if state.snap.chat_title.is_empty() {
            return String::from("Autumn Natter");
        }
        format!("{} - Autumn Natter", state.snap.chat_title)
    }

    fn theme(&self, state: &App, _window: window::Id) -> Option<Theme> {
        Some(if state.snap.dark_mode {
            Theme::CatppuccinMocha
        } else {
            Theme::SolarizedLight
        })
    }

    fn subscription(&self, _state: &App) -> Subscription<Msg> {
        use iced::event::{self, Event};
        use iced::keyboard::Event as Key;
        use iced::mouse::Event as Mouse;

        let ticks = iced::time::every(TICK).map(|_| Msg::Tick);
        // The keys are read from every event, and not only from the ones no
        // widget took. The box for a new message keeps the keys it is given,
        // and the row of previews must still answer to Escape, to Alt and to
        // Return while that box has the keys.
        let keys = event::listen_with(|event, _status, which| match event {
            Event::Keyboard(Key::KeyReleased { key, .. }) => Some(Msg::KeyUp(key_name(&key))),
            Event::Mouse(Mouse::CursorMoved { position }) => {
                Some(Msg::Pointer(which, position.x, position.y))
            }
            _ => None,
        });
        let sizes = iced::window::resize_events()
            .map(|(which, size)| Msg::Sized(which, size.width, size.height));
        // A window that asks to be put away, and a window that was put away.
        // The server must not be left running behind an empty desk.
        let asked = iced::window::close_requests().map(Msg::WindowClose);
        let gone = iced::window::close_events().map(Msg::WindowClosed);
        Subscription::batch([ticks, keys, sizes, asked, gone])
    }
}

// The window of the program. The engine was started before this, so the first
// picture of the window already holds the chats on disk.
pub fn run(cfg: Config) -> Result<(), String> {
    let runner = Runner {
        engine: std::cell::RefCell::new(Some(Engine::start(cfg))),
    };
    iced_winit::run(runner).map_err(|why| why.to_string())
}

// The pictures of the window, made without a display. They are only built for
// the tests, and they write under `uidata/`.
#[cfg(test)]
mod shot;

// The path of a key press: the action the box publishes, and the letter it
// leaves behind in its words.
#[cfg(test)]
mod typing {
    use super::*;
    use crate::config;
    use std::path::PathBuf;

    // The engines of the tests take turns, and they share one data folder.
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn app() -> App {
        config::set_data_dir(PathBuf::from("uidata/typing-tests"));
        let _ = config::ensure_dirs();
        App::new(Engine::start(Config::default()))
    }

    // The box writes what it is told to write, so a letter that is pressed is
    // left in the words of the box, and read again before it is sent.
    #[test]
    fn a_pressed_key_is_written_into_the_box() {
        let _guard = TURN.lock().unwrap();
        let mut app = app();

        let _ = update(&mut app, Msg::Draft(Edit::Edit(text_editor::Edit::Insert('a'))));
        assert_eq!(app.draft_text, "a");
        assert_eq!(app.draft.text(), "a");

        let _ = update(&mut app, Msg::Draft(Edit::Edit(text_editor::Edit::Insert('b'))));
        assert_eq!(app.draft_text, "ab");

        let _ = update(&mut app, Msg::Draft(Edit::Edit(text_editor::Edit::Backspace)));
        assert_eq!(app.draft_text, "a");

        // Sending takes what the box holds, and the box is left empty. The
        // first word of a run grows a chat of its own.
        let _ = update(&mut app, Msg::Send);
        assert!(app.draft_text.is_empty());
        assert!(app.draft.text().is_empty());
        assert_eq!(app.snap.chat_count, 1);
    }

    // A key the box does not answer for itself is given the binding of the
    // box, so a plain letter is a letter that is written.
    #[test]
    fn a_plain_key_is_the_binding_of_the_box() {
        use iced::keyboard::{key, Key, Modifiers};
        use iced::widget::text_editor::{Binding, KeyPress, Status};

        let press = KeyPress {
            key: Key::Character("a".into()),
            modified_key: Key::Character("a".into()),
            physical_key: key::Physical::Code(key::Code::KeyA),
            modifiers: Modifiers::NONE,
            text: Some("a".into()),
            // The box only gets a key while it holds the focus.
            status: Status::Focused { is_hovered: false },
        };

        match Binding::<Msg>::from_key_press(press) {
            Some(Binding::Insert(letter)) => assert_eq!(letter, 'a'),
            other => panic!("a plain letter should write itself: {other:?}"),
        }
    }
}
