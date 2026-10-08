// The desktop window. It shows a snapshot of the engine and sends the actions
// of the user back to the engine.
//
// The window never waits for the model. A timer reads the state about twenty
// times a second, so the answer appears while it is written.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use slint::{
    CloseRequestResponse, ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel,
};

use crate::config::{self, Config, ModelSettings};
use crate::engine::{Engine, Snap};
use crate::llama;
use crate::store;
use crate::util;

// The generated Rust code for ui/app.slint. It brings App, the two dialogs, the
// row structs and the Theme global.
slint::include_modules!();

// How often the window looks at the engine.
const TICK: Duration = Duration::from_millis(50);

// Handles the window keeps between ticks.
struct Ui {
    messages: Rc<VecModel<MessageRow>>,
    models: Rc<VecModel<ModelRow>>,
    repositories: Rc<VecModel<RepoRow>>,
    chats: Rc<VecModel<ChatRow>>,
    // Last seen counters, so the window only rebuilds what changed.
    last_structure: Cell<u64>,
    last_rev: Cell<u64>,
    // Last seen list of chat ids. The cards of the carousel are moved and never
    // built again, so the list is only set up anew when a chat came or went
    // away. A turn of the carousel writes the rows in place.
    last_chat_key: RefCell<String>,
    // Last greeting line, so the window is not redrawn for the same words.
    last_greeting: RefCell<String>,
    // The two dialogs are made once and then shown again.
    settings_window: RefCell<Option<Rc<SettingsWindow>>>,
    config_window: RefCell<Option<Rc<ModelConfigWindow>>>,
    // The model the config window is about.
    config_model: Rc<RefCell<String>>,
    // Device switches of the open config window.
    gpu_state: Rc<RefCell<Vec<bool>>>,
}

// Open the window and run the event loop.
pub fn run(cfg: Config) -> Result<(), String> {
    let engine = Rc::new(Engine::start(cfg));
    let app = App::new().map_err(text_of)?;

    // The Slint file gives the window a smallest size only. A window with a set
    // size cannot be made bigger or smaller by hand, so the start size lives
    // here instead.
    app.window()
        .set_size(slint::LogicalSize::new(1020.0, 720.0));

    let ui = Rc::new(Ui {
        messages: Rc::new(VecModel::default()),
        models: Rc::new(VecModel::default()),
        repositories: Rc::new(VecModel::default()),
        chats: Rc::new(VecModel::default()),
        last_structure: Cell::new(0),
        last_rev: Cell::new(0),
        last_chat_key: RefCell::new(String::new()),
        last_greeting: RefCell::new(String::new()),
        settings_window: RefCell::new(None),
        config_window: RefCell::new(None),
        config_model: Rc::new(RefCell::new(String::new())),
        gpu_state: Rc::new(RefCell::new(Vec::new())),
    });

    app.set_messages(ModelRc::from(ui.messages.clone()));
    app.set_models(ModelRc::from(ui.models.clone()));
    app.set_repositories(ModelRc::from(ui.repositories.clone()));
    app.set_chats(ModelRc::from(ui.chats.clone()));
    // The desktop build has no send button, because Enter sends the message.
    // The browser build shows the button on a touch screen only.
    app.set_send_visible(false);

    // ---- user actions ----
    let handle = engine.clone();
    let weak_for_send = app.as_weak();
    app.on_send_message(move |text: SharedString| {
        let text = text.to_string();
        if text.trim().is_empty() {
            return;
        }
        // The box is empty again, so the user can write the next question.
        if let Some(app) = weak_for_send.upgrade() {
            app.set_draft_text(SharedString::default());
        }
        handle.prompt(text);
    });

    let handle = engine.clone();
    app.on_cancel_generation(move || handle.cancel());

    let handle = engine.clone();
    app.on_toggle_code(move |enabled: bool| handle.set_code(enabled));

    let handle = engine.clone();
    app.on_toggle_sandbox(move |enabled: bool| handle.set_sandbox(enabled));

    let handle = engine.clone();
    app.on_select_model(move |id: SharedString| handle.select_model(id.to_string()));

    let handle = engine.clone();
    app.on_select_repository(move |id: SharedString| handle.select_repository(id.to_string()));

    let handle = engine.clone();
    app.on_new_chat(move || handle.new_chat());

    // The two round marks turn the carousel.
    let handle = engine.clone();
    app.on_roll_chat(move |step: i32| handle.roll(step));

    // The row of small previews. The engine only remembers whether the row is
    // up, and the window drives the motion of the row itself.
    let handle = engine.clone();
    app.on_show_overview(move || handle.set_overview(true));

    let handle = engine.clone();
    app.on_hide_overview(move || handle.set_overview(false));

    // A click on a preview opens that chat, and closes the row.
    let handle = engine.clone();
    app.on_pick_chat(move |id: SharedString| handle.open_chat(id.to_string()));

    // A full list of repositories is not built yet. The settings window holds
    // every repository on disk, so this opens that window.
    let handle = engine.clone();
    let ui_for_repositories = ui.clone();
    app.on_view_all_repositories(move || {
        show_settings(&handle, &ui_for_repositories);
    });

    let handle = engine.clone();
    let ui_for_settings = ui.clone();
    app.on_open_settings(move || {
        show_settings(&handle, &ui_for_settings);
    });

    let handle_for_config = engine.clone();
    let ui_for_config = ui.clone();
    app.on_configure_model(move |id: SharedString| {
        show_model_config(&handle_for_config, &ui_for_config, id.to_string());
    });

    // Closing the main window ends the program, even when a dialog is open.
    let handle_for_close = engine.clone();
    app.window().on_close_requested(move || {
        handle_for_close.shutdown();
        CloseRequestResponse::HideWindow
    });

    // ---- the tick that paints the state ----
    let timer = Timer::default();
    let weak_for_tick = app.as_weak();
    let handle_for_tick = engine.clone();
    let ui_for_tick = ui.clone();
    timer.start(TimerMode::Repeated, TICK, move || {
        let Some(app) = weak_for_tick.upgrade() else { return };
        render(&app, &handle_for_tick, &ui_for_tick);
    });

    render(&app, &engine, &ui);

    app.run().map_err(text_of)?;
    // The window can also be closed by the platform, so make sure the model is
    // unloaded here too.
    engine.shutdown();
    Ok(())
}

// Draw the current state into the window.
fn render(app: &App, engine: &Engine, ui: &Rc<Ui>) {
    let snap = engine.snapshot();

    // Palette. Light mode is the default.
    app.global::<Theme>().set_dark(snap.dark_mode);

    // The two flags that move the whole screen go first. A chat grows from the
    // top down when it opens, and that growth starts at nothing, so the window
    // must learn about the change before the chat list arrives.
    app.set_home(snap.home);
    app.set_overview(snap.overview);

    // The line above the chat box on the first screen. It follows the clock, so
    // it is written again only when the words really changed.
    if snap.greeting != *ui.last_greeting.borrow() {
        *ui.last_greeting.borrow_mut() = snap.greeting.clone();
        app.set_greeting(text(snap.greeting.as_str()));
    }

    app.set_model_name(text(snap.model_name.as_str()));
    app.set_model_kind(text(snap.model_kind.as_str()));
    app.set_in_repository(snap.in_repository);
    app.set_repository_name(text(snap.repository_name.as_str()));
    app.set_chat_title(text(snap.chat_title.as_str()));
    app.set_context_used(text(&count(snap.context_used)));
    app.set_context_total(text(&count(snap.context_total)));
    let ratio = if snap.context_total > 0 {
        snap.context_used as f32 / snap.context_total as f32
    } else {
        0.0
    };
    app.set_context_ratio(ratio.min(1.0));
    app.set_generating(snap.generating);
    app.set_server_busy(snap.busy);
    app.set_code_enabled(snap.code_enabled);
    app.set_sandbox_enabled(snap.sandbox_enabled);
    app.set_sandbox_available(snap.sandbox_available);
    app.set_status_text(text(snap.status.as_str()));

    // The carousel turns to the open chat, and says when that chat is blank.
    app.set_chat_index(snap.chat_index as i32);
    app.set_blank_chat(snap.blank_chat);

    if snap.structure_rev != ui.last_structure.get() {
        ui.last_structure.set(snap.structure_rev);
        ui.models.set_vec(
            snap.models
                .iter()
                .map(|m| ModelRow {
                    id: text(&m.id()),
                    name: text(&m.name),
                    kind: text(m.kind),
                    active: m.id() == snap.model_id,
                })
                .collect::<Vec<ModelRow>>(),
        );
        ui.repositories.set_vec(
            snap.repositories
                .iter()
                .map(|r| RepoRow {
                    id: text(&r.id),
                    name: text(&r.name),
                    path: text(&r.path.display().to_string()),
                })
                .collect::<Vec<RepoRow>>(),
        );
        // The two boxes in the top bar take the width of the widest name of
        // their list, so a name is never cut down to a few letters.
        app.set_longest_model(text(&longest_model(&snap)));
        app.set_longest_repository(text(&longest_repository(&snap)));
    }

    // The chat list is set up again only when a chat came or went away. A turn
    // of the carousel marks another chat as the open one, and those rows are
    // written in place, so the window slides the cards instead of building them
    // again. A card that is built again has no earlier place to slide from.
    let key = chat_key(&snap);
    if key != *ui.last_chat_key.borrow() {
        *ui.last_chat_key.borrow_mut() = key;
        ui.chats.set_vec(chat_rows(&snap));
    } else {
        for (index, row) in chat_rows(&snap).into_iter().enumerate() {
            let same = ui.chats.row_data(index).is_some_and(|current| current == row);
            if !same {
                ui.chats.set_row_data(index, row);
            }
        }
    }

    if snap.rev != ui.last_rev.get() || ui.messages.row_count() == 0 {
        ui.last_rev.set(snap.rev);
        draw_messages(ui, &snap);
        // Tell the window to keep the newest message in view.
        let next = app.get_message_rev().saturating_add(1);
        app.set_message_rev(next);
    }
}

// Build the message rows. While an answer comes in, the text the model wrote
// just now gets its own row.
fn draw_messages(ui: &Rc<Ui>, snap: &Snap) {
    let mut rows: Vec<MessageRow> = Vec::with_capacity(snap.messages.len() + 1);
    let mut previous_tool = false;
    for message in &snap.messages {
        rows.push(row_for(message, previous_tool));
        previous_tool = message.role == "Tool";
    }
    if snap.generating {
        rows.push(MessageRow {
            id: text("live"),
            role: text("Assistant"),
            body: text(&snap.live_text),
            tool: SharedString::default(),
            mono: false,
            failed: false,
            when: text(&store::format_clock(store::now_secs())),
            blocks: blocks_model(&snap.live_text),
            reasoning: text(&snap.live_reasoning),
        });
    }
    let same_count = rows.len() == ui.messages.row_count();
    if same_count {
        // Only rows that changed are written, so the list does not flicker.
        for (index, row) in rows.into_iter().enumerate() {
            let current = ui.messages.row_data(index);
            if let Some(current) = current {
                if current != row {
                    ui.messages.set_row_data(index, row);
                }
            }
        }
    } else {
        ui.messages.set_vec(rows);
    }
}

// Convert parsed markdown blocks into the Slint model type.
fn blocks_model(body: &str) -> ModelRc<MarkdownBlock> {
    let vec: Vec<MarkdownBlock> = crate::markdown::parse(body)
        .into_iter()
        .map(|b| MarkdownBlock {
            kind: text(&b.kind),
            level: b.level,
            content: text(&b.content),
        })
        .collect();
    ModelRc::from(Rc::new(VecModel::from(vec)))
}

// One message as the window shows it.
fn row_for(message: &store::Message, previous_tool: bool) -> MessageRow {
    let _ = previous_tool;
    MessageRow {
        id: text(&message.id),
        role: text(&message.role),
        body: text(&message.body),
        tool: text(&message.tool),
        mono: message.mono,
        failed: message.failed,
        when: text(&store::format_clock(message.when)),
        blocks: blocks_model(&message.body),
        reasoning: text(&message.reasoning),
    }
}

// The chat rows as the window and the previews show them. The name of a
// repository is written out, never the id of one.
fn chat_rows(snap: &Snap) -> Vec<ChatRow> {
    snap.chats
        .iter()
        .map(|c| ChatRow {
            id: text(&c.meta.id),
            title: text(&c.meta.title),
            repo: text(&repo_label(snap, &c.meta.repo_id)),
            when: text(&store::format_when(c.meta.updated)),
            active: c.meta.id == snap.chat_id,
            preview: text(&store::preview(&c.messages)),
        })
        .collect()
}

// One text that says which chats a place holds, and in which order. The window
// sets the chat list up again only when this text changes.
fn chat_key(snap: &Snap) -> String {
    snap.chats
        .iter()
        .map(|c| c.meta.id.as_str())
        .collect::<Vec<&str>>()
        .join("|")
}

// The widest model name of the list, with the kind of the model after it, as
// the top bar shows it. The model box takes this width.
fn longest_model(snap: &Snap) -> String {
    let mut best = String::new();
    for model in &snap.models {
        let line = format!("{} ({})", model.name, model.kind);
        if line.chars().count() > best.chars().count() {
            best = line;
        }
    }
    best
}

// The widest repository name of the list. The repository box takes this width.
fn longest_repository(snap: &Snap) -> String {
    let mut best = String::new();
    for repo in &snap.repositories {
        if repo.name.chars().count() > best.chars().count() {
            best = repo.name.clone();
        }
    }
    best
}

// The settings dialog. It writes settings.json through the engine.
fn show_settings(engine: &Rc<Engine>, ui: &Rc<Ui>) {
    let Some(window) = take_settings(&ui.settings_window) else {
        return;
    };

    let snap = engine.snapshot();
    let cfg = engine.config();
    window.global::<Theme>().set_dark(snap.dark_mode);
    window.set_data_dir(text(&snap.data_dir));
    window.set_bwrap_text(text(&snap.sandbox_note));
    window.set_mode_text(text("This run shows the desktop window."));
    window.set_llama_dir(text(&cfg.llama_dir));
    window.set_models_dir(text(&cfg.models_dir));
    window.set_repositories_dir(text(&cfg.repositories_dir));
    window.set_dark_mode(cfg.dark_mode);
    window.set_remote_port(cfg.remote_port as i32);
    window.set_upnp_enabled(cfg.upnp_enabled);
    fill_repositories(&window, engine);

    // Save reads every field, so a half written field cannot be missed.
    let handle = engine.clone();
    let weak = window.as_weak();
    window.on_save(move || {
        let Some(window) = weak.upgrade() else { return };
        let mut cfg = handle.config();
        cfg.llama_dir = window.get_llama_dir().trim().to_string();
        cfg.models_dir = window.get_models_dir().trim().to_string();
        cfg.repositories_dir = window.get_repositories_dir().trim().to_string();
        cfg.dark_mode = window.get_dark_mode();
        let port = window.get_remote_port();
        cfg.remote_port = if port < 1 { config::DEFAULT_REMOTE_PORT } else { port as u16 };
        cfg.upnp_enabled = window.get_upnp_enabled();
        handle.save_settings(cfg);
        let _ = window.window().hide();
    });

    // A new repository is made on disk, and then the window works in it.
    let handle = engine.clone();
    let weak = window.as_weak();
    window.on_create_repository(move |name: SharedString| {
        let Some(window) = weak.upgrade() else { return };
        handle.create_repository(name.to_string());
        // The name of the new repository is on disk, so the field is empty
        // again for the next one.
        window.set_new_repository(SharedString::default());
        fill_repositories(&window, &handle);
    });

    let handle = engine.clone();
    let weak = window.as_weak();
    window.on_use_repository(move |id: SharedString| {
        let Some(window) = weak.upgrade() else { return };
        handle.select_repository(id.to_string());
        fill_repositories(&window, &handle);
    });

    let weak = window.as_weak();
    window.on_close_window(move || {
        if let Some(window) = weak.upgrade() {
            let _ = window.window().hide();
        }
    });

    let _ = window.window().show();
}

// Show every repository in the settings dialog, and mark the one that is in
// use. The list is read again after a new repository was made.
fn fill_repositories(window: &SettingsWindow, engine: &Engine) {
    let rows: Vec<RepoRow> = engine
        .repositories()
        .iter()
        .map(|r| RepoRow {
            id: text(&r.id),
            name: text(&r.name),
            path: text(&r.path.display().to_string()),
        })
        .collect();
    window.set_repositories(ModelRc::from(Rc::new(VecModel::from(rows))));
    let active = engine.snapshot().repository_id;
    window.set_active_repository(text(&active));
}

// The model settings dialog. It shows the settings of one model and the
// devices of the machine.
fn show_model_config(engine: &Rc<Engine>, ui: &Rc<Ui>, id: String) {
    let Some(window) = take_config(&ui.config_window) else {
        return;
    };

    let snap = engine.snapshot();
    let (settings, gpus) = engine.model_settings(&id);
    // Device switches start from the stored settings, or all on.
    let flags = if settings.gpu_enabled.len() == gpus.len() && !gpus.is_empty() {
        settings.gpu_enabled.clone()
    } else {
        vec![true; gpus.len()]
    };
    *ui.gpu_state.borrow_mut() = flags.clone();
    *ui.config_model.borrow_mut() = id.clone();

    let total_mib: u64 = gpus.iter().map(|g| g.memory_mb).sum();
    window.global::<Theme>().set_dark(snap.dark_mode);
    window.set_model_name(text(
        snap.models
            .iter()
            .find(|m| m.id() == id)
            .map(|m| m.name.as_str())
            .unwrap_or(""),
    ));
    window.set_model_kind(text(
        snap.models
            .iter()
            .find(|m| m.id() == id)
            .map(|m| m.kind)
            .unwrap_or(""),
    ));
    window.set_model_path(text(&id));
    window.set_server_text(text(&command_preview(engine, &snap, &settings, &gpus, &flags)));
    window.set_kv_options(list(llama::KV_TYPES));
    window.set_spec_options(list(llama::SPEC_TYPES));
    window.set_load_options(list(llama::LOAD_MODES));
    let rows: Vec<GpuRow> = gpus
        .iter()
        .enumerate()
        .map(|(index, gpu)| GpuRow {
            id: index as i32,
            name: text(&gpu.name),
            memory: text(&gpu.memory_text()),
            enabled: *flags.get(index).unwrap_or(&true),
        })
        .collect();
    window.set_gpus(ModelRc::from(Rc::new(VecModel::from(rows))));
    window.set_vram_total(total_mib as f32);
    window.set_vram_budget(settings.vram_budget as f32);
    window.set_ctx_size(settings.ctx_size as i32);
    window.set_kv_index(settings.kv_index as i32);
    window.set_spec_index(settings.spec_index as i32);
    window.set_load_index(settings.load_index as i32);
    window.set_draft_model(text(&settings.draft_model));
    window.set_draft_tokens(settings.draft_tokens as i32);
    window.set_parallel(settings.parallel as i32);
    window.set_batch_size(settings.batch_size as i32);
    window.set_ubatch_size(settings.ubatch_size as i32);
    window.set_tensor_parallel(settings.tensor_parallel);
    window.set_vision(settings.vision);
    window.set_reasoning_budget(settings.reasoning_budget as i32);
    window.set_reasoning_message(text(&settings.reasoning_message));
    window.set_extra_args(text(&settings.extra_args));
    window.set_chat_template(text(&settings.chat_template));

    // A device switch only changes the list in this dialog. Apply writes it.
    let state = ui.gpu_state.clone();
    let weak = window.as_weak();
    window.on_toggle_gpu(move |index: i32, enabled: bool| {
        let index = index.max(0) as usize;
        {
            let mut flags = state.borrow_mut();
            if index < flags.len() {
                flags[index] = enabled;
            }
        }
        let Some(window) = weak.upgrade() else { return };
        let gpus = window.get_gpus();
        if let Some(row) = gpus.row_data(index) {
            let mut fixed = row;
            fixed.enabled = enabled;
            gpus.set_row_data(index, fixed);
        }
    });

    let handle = engine.clone();
    let model = ui.config_model.clone();
    let state = ui.gpu_state.clone();
    let weak = window.as_weak();
    window.on_apply(move || {
        let Some(window) = weak.upgrade() else { return };
        let id = model.borrow().clone();
        let settings = ModelSettings {
            ctx_size: window.get_ctx_size().max(256) as u32,
            kv_index: window.get_kv_index().max(0) as usize,
            spec_index: window.get_spec_index().max(0) as usize,
            draft_model: window.get_draft_model().trim().to_string(),
            draft_tokens: window.get_draft_tokens().max(1) as u32,
            parallel: window.get_parallel().max(1) as u32,
            batch_size: window.get_batch_size().max(1) as u32,
            ubatch_size: window.get_ubatch_size().max(1) as u32,
            tensor_parallel: window.get_tensor_parallel(),
            vision: window.get_vision(),
            reasoning_budget: window.get_reasoning_budget().max(-1) as i64,
            reasoning_message: window.get_reasoning_message().to_string(),
            load_index: window.get_load_index().max(0) as usize,
            extra_args: window.get_extra_args().to_string(),
            chat_template: window.get_chat_template().to_string(),
            vram_budget: window.get_vram_budget().max(0.0) as f64,
            gpu_enabled: state.borrow().clone(),
        };
        handle.save_model_settings(id, settings);
        let _ = window.window().hide();
    });

    let weak = window.as_weak();
    window.on_close_window(move || {
        if let Some(window) = weak.upgrade() {
            let _ = window.window().hide();
        }
    });

    let _ = window.window().show();
}

// The command line the next message will use. It helps to see it here, because
// a wrong extra argument is then easy to find.
fn command_preview(
    engine: &Rc<Engine>,
    snap: &Snap,
    settings: &ModelSettings,
    gpus: &[crate::gpu::Gpu],
    flags: &[bool],
) -> String {
    let Some(model) = snap.models.iter().find(|m| m.id() == engine_model_id(engine, snap)) else {
        return String::new();
    };
    let mut devices = gpus.to_vec();
    for (index, flag) in flags.iter().enumerate() {
        if let Some(device) = devices.get_mut(index) {
            device.enabled = *flag;
        }
    }
    let mut notes = Vec::new();
    let args = llama::build_args(
        std::path::Path::new(""),
        model,
        settings,
        &devices,
        0,
        snap.code_enabled,
        &mut notes,
    );
    let mut text = String::from("llama-server ");
    for arg in args {
        if arg.contains(' ') {
            text.push_str(&format!("\"{}\"", arg));
        } else {
            text.push_str(&arg);
        }
        text.push(' ');
    }
    for note in notes {
        text.push_str(&format!("\n{}", note));
    }
    text
}

fn engine_model_id(engine: &Rc<Engine>, snap: &Snap) -> String {
    let _ = engine;
    snap.model_id.clone()
}

// Make the settings window once, then reuse it.
fn take_settings(slot: &RefCell<Option<Rc<SettingsWindow>>>) -> Option<Rc<SettingsWindow>> {
    let mut slot = slot.borrow_mut();
    if slot.is_none() {
        *slot = SettingsWindow::new().ok().map(Rc::new);
    }
    slot.clone()
}

// Make the model window once, then reuse it.
fn take_config(slot: &RefCell<Option<Rc<ModelConfigWindow>>>) -> Option<Rc<ModelConfigWindow>> {
    let mut slot = slot.borrow_mut();
    if slot.is_none() {
        *slot = ModelConfigWindow::new().ok().map(Rc::new);
    }
    slot.clone()
}

// Name of a repository, for the chat list. A chat with no repository shows an
// empty string, and the window then writes "no repository".
fn repo_label(snap: &Snap, repo_id: &str) -> String {
    if repo_id.is_empty() {
        return String::new();
    }
    snap.repositories
        .iter()
        .find(|r| r.id == repo_id)
        .map(|r| r.name.clone())
        .unwrap_or_else(|| repo_id.to_string())
}

// Turn a list of names into the string list Slint wants.
fn list(items: &[&'static str]) -> ModelRc<SharedString> {
    let rows: Vec<SharedString> = items.iter().map(|item| text(item)).collect();
    ModelRc::from(Rc::new(VecModel::from(rows)))
}

// Turn a string into the type Slint wants.
fn text(value: &str) -> SharedString {
    SharedString::from(value.to_string())
}

// Tokens with a comma every three digits.
fn count(value: u64) -> String {
    util::count(value)
}

fn text_of(error: slint::PlatformError) -> String {
    format!("the window could not be opened: {}", error)
}
