// The engine. It owns the settings, the chats, the model list and the
// llama-server process. The Slint window and the browser page both drive this
// one engine, so both show the same thing.
//
// One worker thread does all slow work: it starts the server, asks the model
// and runs the terminal tool. A window only reads a snapshot of the state, so
// a slow answer never stops the window from drawing.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::config::{self, Config, ModelSettings};
use crate::gpu::{self, Gpu};
use crate::llama::{self, ModelInfo, Server, ToolCall};
use crate::store::{self, Chat, Message, Repo};
use crate::tools;

// Longest wait for one terminal command.
const TOOL_TIMEOUT: Duration = Duration::from_secs(120);
// How many tool steps one answer may take.
const MAX_TOOL_STEPS: usize = 6;

// Work for the worker thread.
enum Job {
    // The user asked something.
    Prompt(String),
    // Another model is wanted, so the running server must go.
    ModelChanged,
    // Settings changed, so a server must not stay with the old values.
    SettingsChanged,
    // Leave the process, after the model is unloaded.
    Shutdown,
}

// A copy of everything a window needs to draw. It is made while the state lock
// is held, and then used without the lock.
pub struct Snap {
    pub model_name: String,
    pub model_kind: String,
    pub in_repository: bool,
    pub repository_id: String,
    pub repository_name: String,
    pub chat_title: String,
    pub context_used: u64,
    pub context_total: u64,
    pub generating: bool,
    pub busy: bool,
    pub code_enabled: bool,
    pub sandbox_enabled: bool,
    pub sandbox_available: bool,
    pub status: String,
    pub dark_mode: bool,
    pub data_dir: String,
    // What the sandbox can do on this machine, for the settings window.
    pub sandbox_note: String,
    pub models: Vec<ModelInfo>,
    pub model_id: String,
    pub repositories: Vec<Repo>,
    pub chats: Vec<Chat>,
    pub chat_id: String,
    // Where the open chat sits in the carousel. The window moves the carousel
    // to this place when the number changes.
    pub chat_index: usize,
    // How many chats this place has, the blank one counted too.
    pub chat_count: usize,
    // True when the open chat holds no message yet.
    pub blank_chat: bool,
    pub messages: Vec<Message>,
    // Text the model is writing right now, while an answer comes in.
    pub live_text: String,
    // Goes up when a message changes, so the window redraws them.
    pub rev: u64,
    // Goes up when a list changes, so the window rebuilds it.
    pub structure_rev: u64,
}

// The state behind the engine.
pub struct State {
    pub cfg: Config,
    pub store: store::Store,
    pub models: Vec<ModelInfo>,
    pub gpus: Vec<Gpu>,
    pub repositories: Vec<Repo>,
    // Repository the chat list shows, and the one new chats belong to.
    pub scope_repo: String,
    pub chats: Vec<Chat>,
    pub current: Option<String>,
    // A blank chat that the user turned away from. It is dropped once the
    // carousel has finished moving, so the move is still smooth. The text is
    // the id of that chat.
    pub pending_blank: Option<(String, Instant)>,
    // Text the model is writing right now.
    pub live_text: String,
    pub generating: bool,
    // The model is loading, so the window shows a spinner.
    pub busy: bool,
    pub status: String,
    pub ctx_total: u64,
    pub ctx_used: u64,
    pub sandbox_available: bool,
    pub sandbox_note: String,
    pub rev: u64,
    pub structure_rev: u64,
}

impl State {
    // Say that the messages changed.
    fn touch(&mut self) {
        self.rev += 1;
    }

    // Say that a list changed too.
    fn touch_structure(&mut self) {
        self.structure_rev += 1;
        self.rev += 1;
    }

    fn set_status(&mut self, text: &str) {
        self.status = text.to_string();
        self.touch();
    }

    // The chat that is open.
    fn chat_mut(&mut self) -> Option<&mut Chat> {
        let id = self.current.clone()?;
        self.chats.iter_mut().find(|c| c.meta.id == id)
    }

    fn chat(&self) -> Option<&Chat> {
        let id = self.current.as_deref()?;
        self.chats.iter().find(|c| c.meta.id == id)
    }

    // Write the open chat to disk. Cloning first keeps the borrow short.
    // A chat that holds something is a real chat from then on, even when the
    // carousel made it as a blank one.
    fn persist(&mut self) {
        if let Some(id) = self.current.clone() {
            if let Some(chat) = self.chats.iter_mut().find(|c| c.meta.id == id) {
                if !chat.messages.is_empty() {
                    chat.transient = false;
                }
            }
        }
        if let Some(chat) = self.chat() {
            let snapshot = chat.clone();
            self.store.save_chat(&snapshot);
        }
    }

    // Add one message of a normal kind to the open chat.
    fn push_message(&mut self, role: &str, body: &str, mono: bool) {
        if let Some(chat) = self.chat_mut() {
            chat.messages.push(Message {
                id: store::new_id(),
                role: role.to_string(),
                body: body.to_string(),
                mono,
                when: store::now_secs(),
                ..Default::default()
            });
        }
        self.persist();
        self.touch();
    }

    // Add one note. A failed note is drawn in the warning colour. Notes are for
    // the window and are never sent to the model.
    fn push_note(&mut self, text: &str, failed: bool) {
        if let Some(chat) = self.chat_mut() {
            chat.messages.push(Message {
                id: store::new_id(),
                role: "Note".to_string(),
                body: text.to_string(),
                failed,
                when: store::now_secs(),
                ..Default::default()
            });
        }
        self.persist();
        self.touch();
    }

    // Put a tool run in the chat before it starts. The id is given back so the
    // output can be written into the same message.
    fn begin_tool(&mut self, call: &ToolCall, shown: &str) -> String {
        let id = store::new_id();
        if let Some(chat) = self.chat_mut() {
            chat.messages.push(Message {
                id: id.clone(),
                role: "Tool".to_string(),
                body: shown.to_string(),
                tool: String::from("running ..."),
                mono: true,
                call_id: call.id.clone(),
                tool_name: call.name.clone(),
                tool_args: call.arguments.clone(),
                when: store::now_secs(),
                ..Default::default()
            });
        }
        self.persist();
        self.touch();
        id
    }

    // Write the output of a tool run into its message.
    fn finish_tool(&mut self, id: &str, output: &str, ok: bool) {
        if let Some(chat) = self.chat_mut() {
            if let Some(message) = chat.messages.iter_mut().find(|m| m.id == id) {
                message.tool = output.to_string();
                message.failed = !ok;
            }
        }
        self.persist();
        self.touch();
    }

    // Turn the open chat into the message list the API expects.
    fn build_history(&self) -> Value {
        let mut out: Vec<Value> = Vec::new();
        let Some(chat) = self.chat() else {
            return json!([]);
        };
        for message in &chat.messages {
            match message.role.as_str() {
                "You" => out.push(json!({ "role": "user", "content": message.body })),
                "Assistant" => out.push(json!({
                    "role": "assistant",
                    "content": message.body,
                })),
                "Tool" => {
                    if message.call_id.is_empty() {
                        continue;
                    }
                    let name = if message.tool_name.is_empty() {
                        tools::TOOL_NAME
                    } else {
                        message.tool_name.as_str()
                    };
                    // One stored message stands for the call and for its
                    // result, so it becomes two entries here.
                    out.push(json!({
                        "role": "assistant",
                        "content": Value::Null,
                        "tool_calls": [{
                            "id": message.call_id,
                            "type": "function",
                            "function": { "name": name, "arguments": message.tool_args }
                        }]
                    }));
                    out.push(json!({
                        "role": "tool",
                        "tool_call_id": message.call_id,
                        "content": message.tool,
                    }));
                }
                // A note is only for the window.
                _ => {}
            }
        }
        json!(out)
    }

    // Read the chats of the current scope from disk.
    fn reload_chats(&mut self) {
        self.chats = self.store.list_chats(&self.scope_repo);
        // Keep the open chat when it is still there.
        if let Some(id) = self.current.clone() {
            if !self.chats.iter().any(|c| c.meta.id == id) {
                self.current = None;
            }
        }
        self.pending_blank = None;
        // A place with no chat at all still gets a blank one, so the carousel
        // is never empty. When the place has chats, the newest one is opened.
        if self.chats.is_empty() {
            self.open_blank_chat();
        } else if self.current.is_none() {
            self.current = Some(self.chats[0].meta.id.clone());
            self.ctx_used = self.chats[0].meta.context_used;
        }
        self.touch_structure();
    }

    // Put a blank chat at the front of the carousel and open it. The blank
    // chat lives only in the memory. When one is there already, it is used
    // again, so asking for a new chat twice does not make two of them.
    fn open_blank_chat(&mut self) {
        if let Some(place) = self
            .chats
            .iter()
            .position(|c| c.transient && c.messages.is_empty())
        {
            // A new chat belongs at the front, so move the blank one there.
            if place != 0 {
                let chat = self.chats.remove(place);
                self.chats.insert(0, chat);
            }
            self.use_blank_front();
            return;
        }
        let model_id = self.current_model_id();
        let mut chat = Chat::blank(&self.scope_repo, &model_id);
        // A blank chat keeps the sandbox choice of the chat that was at the
        // front. The terminal tool always starts off.
        if let Some(previous) = self.chats.first() {
            chat.meta.sandbox_enabled = previous.meta.sandbox_enabled;
        }
        self.chats.insert(0, chat);
        self.use_blank_front();
    }

    // Open the chat that sits at the front of the carousel, and say what a
    // blank chat is.
    fn use_blank_front(&mut self) {
        let front = self.chats.first().map(|c| c.meta.id.clone());
        self.current = front;
        self.ctx_used = 0;
        self.pending_blank = None;
        self.set_status("a blank chat. Write in it, or turn the carousel and it goes away");
        self.touch_structure();
    }

    // Forget the blank chat with this id, when it is still blank and it is not
    // the chat that is open.
    fn drop_blank(&mut self, id: &str) {
        if self.current.as_deref() == Some(id) {
            return;
        }
        let place = self.chats.iter().position(|c| c.meta.id == id);
        let Some(place) = place else { return };
        if !(self.chats[place].transient && self.chats[place].messages.is_empty()) {
            return;
        }
        let dropped = self.chats.remove(place);
        // A blank chat never reaches the disk. The delete covers a file that an
        // older run may have left, so the chat cannot come back by itself.
        self.store.delete_chat(&dropped.meta.repo_id, &dropped.meta.id);
        self.touch_structure();
    }

    // Take away a blank chat that the user turned away from. This waits until
    // the carousel had the time to move, so the move stays smooth.
    fn prune_blank(&mut self) {
        let Some((id, deadline)) = self.pending_blank.clone() else { return };
        if Instant::now() < deadline {
            return;
        }
        self.pending_blank = None;
        self.drop_blank(&id);
    }

    // Note that the open chat is a blank one, in case the user leaves it now.
    fn leave_blank(&mut self) {
        let Some(id) = self.current.clone() else { return };
        let blank = self
            .chats
            .iter()
            .any(|c| c.meta.id == id && c.transient && c.messages.is_empty());
        if blank {
            self.pending_blank = Some((id, Instant::now() + Duration::from_millis(320)));
        }
    }

    // Refresh the model list, the device list and the repositories.
    fn refresh_lists(&mut self) {
        self.models = llama::list_models(Path::new(&self.cfg.models_dir));
        self.gpus = match self.server_binary() {
            Some(binary) => gpu::detect(Some(binary.as_path())),
            None => gpu::detect(None),
        };
        self.repositories = self.store.scan_repos();
        self.touch_structure();
    }

    // Full path of the llama-server binary.
    pub fn server_binary(&self) -> Option<PathBuf> {
        if self.cfg.llama_dir.trim().is_empty() {
            return None;
        }
        config::server_in(Path::new(&self.cfg.llama_dir))
    }

    // The selected model.
    pub fn model(&self) -> Option<ModelInfo> {
        let id = self.current_model_id();
        self.models.iter().find(|m| m.id() == id).cloned()
    }

    pub fn current_model_id(&self) -> String {
        if let Some(chat) = self.chat() {
            if !chat.meta.model_id.is_empty() {
                return chat.meta.model_id.clone();
            }
        }
        self.cfg.model_id.clone()
    }

    // Settings for one model, with the device list fitted to the machine.
    fn settings_for(&self, model: &ModelInfo) -> ModelSettings {
        let mut settings = self.cfg.model(&model.id());
        settings.resize_gpus(self.gpus.len());
        settings
    }

    // Directory a tool run starts in. A repository chat works inside its
    // repository, and a loose chat gets its own folder.
    fn workdir_for(&self, chat: &Chat) -> PathBuf {
        if !chat.meta.repo_id.is_empty() {
            if let Some(path) = self.store.repo_path(&chat.meta.repo_id) {
                return path;
            }
        }
        let dir = config::paths().data.join("work").join(&chat.meta.id);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    // Make the snapshot a window draws from.
    pub fn snap(&self) -> Snap {
        let model = self.model();
        let chat = self.chat();
        let repository_name = self
            .repositories
            .iter()
            .find(|r| r.id == self.scope_repo)
            .map(|r| r.name.clone())
            .unwrap_or_default();
        let context_total = if self.ctx_total > 0 {
            self.ctx_total
        } else if let Some(model) = &model {
            self.cfg.model(&model.id()).ctx_size as u64
        } else {
            0
        };
        Snap {
            model_name: model.as_ref().map(|m| m.name.clone()).unwrap_or_default(),
            model_kind: model.as_ref().map(|m| m.kind.to_string()).unwrap_or_default(),
            in_repository: !self.scope_repo.is_empty(),
            repository_id: self.scope_repo.clone(),
            repository_name,
            chat_title: chat
                .map(|c| c.meta.title.clone())
                .unwrap_or_else(|| String::from("No chat")),
            context_used: self.ctx_used,
            context_total,
            generating: self.generating,
            busy: self.busy,
            code_enabled: chat.map(|c| c.meta.code_enabled).unwrap_or(false),
            sandbox_enabled: chat.map(|c| c.meta.sandbox_enabled).unwrap_or(true),
            sandbox_available: self.sandbox_available,
            status: self.status.clone(),
            dark_mode: self.cfg.dark_mode,
            data_dir: config::paths().data.display().to_string(),
            sandbox_note: self.sandbox_note.clone(),
            models: self.models.clone(),
            model_id: self.current_model_id(),
            // The top bar shows five recent repositories at most.
            repositories: self.repositories.iter().take(5).cloned().collect(),
            chats: self.chats.clone(),
            chat_id: self.current.clone().unwrap_or_default(),
            chat_index: self
                .chats
                .iter()
                .position(|c| Some(&c.meta.id) == self.current.as_ref())
                .unwrap_or(0),
            chat_count: self.chats.len(),
            blank_chat: chat.map(|c| c.messages.is_empty()).unwrap_or(true),
            messages: chat.map(|c| c.messages.clone()).unwrap_or_default(),
            live_text: self.live_text.clone(),
            rev: self.rev,
            structure_rev: self.structure_rev,
        }
    }
}

// The handle a window keeps.
pub struct Engine {
    state: Arc<Mutex<State>>,
    jobs: Sender<Job>,
    cancel: Arc<AtomicBool>,
}

impl Engine {
    // Start the engine and its worker thread.
    pub fn start(cfg: Config) -> Engine {
        let store = store::Store::new(config::repositories_dir(&cfg));
        let sandbox_available = tools::available();
        let sandbox_note = match tools::probe() {
            Ok(()) => String::from("bubblewrap is present, so tool runs are sandboxed"),
            Err(problem) => problem,
        };
        let mut state = State {
            cfg,
            store,
            models: Vec::new(),
            gpus: Vec::new(),
            repositories: Vec::new(),
            scope_repo: String::new(),
            chats: Vec::new(),
            current: None,
            live_text: String::new(),
            generating: false,
            busy: false,
            status: String::from("ready"),
            ctx_total: 0,
            ctx_used: 0,
            sandbox_available,
            sandbox_note,
            rev: 0,
            structure_rev: 0,
            pending_blank: None,
        };
        state.refresh_lists();
        state.reload_chats();
        // The first thing the user sees is a blank chat at the front of the
        // carousel. It goes away again when it is not used.
        state.open_blank_chat();
        let state = Arc::new(Mutex::new(state));

        let (sender, receiver) = channel::<Job>();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_state = state.clone();
        let worker_cancel = cancel.clone();
        std::thread::spawn(move || worker(worker_state, receiver, worker_cancel));

        Engine {
            state,
            jobs: sender,
            cancel,
        }
    }

    // The settings, for the settings window.
    pub fn config(&self) -> Config {
        match self.state.lock() {
            Ok(state) => state.cfg.clone(),
            Err(_) => Config::default(),
        }
    }

    // Read the state without changing it. A blank chat that the user turned
    // away from is taken away here, because every window reads the state
    // several times a second.
    pub fn snapshot(&self) -> Snap {
        match self.state.lock() {
            Ok(mut state) => {
                state.prune_blank();
                state.snap()
            }
            Err(_) => Snap::empty(),
        }
    }

    // Ask the model something. The worker thread does the work.
    pub fn prompt(&self, text: String) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.generating {
            state.set_status("an answer is still coming");
            return;
        }
        if state.current.is_none() {
            state.set_status("start a new chat first");
            return;
        }
        if state.model().is_none() {
            state.set_status("pick a model in the top bar first");
            return;
        }
        if state.server_binary().is_none() {
            state.set_status("llama-server was not found, set its folder in settings");
            return;
        }
        drop(state);
        self.cancel.store(false, Ordering::SeqCst);
        let _ = self.jobs.send(Job::Prompt(text));
    }

    // Stop reading the answer.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    // The terminal tool for the open chat.
    pub fn set_code(&self, enabled: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if let Some(chat) = state.chat_mut() {
            chat.meta.code_enabled = enabled;
        }
        state.persist();
        state.set_status(if enabled {
            "the model may use the terminal tool"
        } else {
            "the terminal tool is off"
        });
    }

    // The sandbox for the open chat.
    pub fn set_sandbox(&self, enabled: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if enabled && !state.sandbox_available {
            let note = state.sandbox_note.clone();
            state.set_status(&note);
            return;
        }
        if let Some(chat) = state.chat_mut() {
            chat.meta.sandbox_enabled = enabled;
        }
        state.persist();
        state.set_status(if enabled {
            "tool runs are sandboxed"
        } else {
            "tool runs are not sandboxed"
        });
    }

    // Show another repository, and load its chats.
    pub fn select_repository(&self, id: String) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.generating {
            state.set_status("wait for the answer to finish");
            return;
        }
        if id == state.scope_repo {
            return;
        }
        state.scope_repo = id.clone();
        state.current = None;
        if let Some(mut repo) = state.repositories.iter().find(|r| r.id == id).cloned() {
            state.store.touch_repo(&mut repo);
            state.status = format!("repository {}", repo.name);
        }
        state.repositories = state.store.scan_repos();
        state.reload_chats();
    }

    // Make a new repository, then show it. The name on disk follows the rule
    // "name_id", where the name keeps only letters, digits and dashes.
    pub fn create_repository(&self, name: String) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.generating {
            state.set_status("wait for the answer to finish");
            return;
        }
        let name = name.trim().to_string();
        if name.is_empty() {
            state.set_status("a repository needs a name");
            return;
        }
        let made = state.store.create_repo(&name);
        match made {
            Ok(repo) => {
                state.scope_repo = repo.id.clone();
                state.current = None;
                state.repositories = state.store.scan_repos();
                state.reload_chats();
                let text = format!("repository {} is ready", repo.name);
                state.set_status(&text);
            }
            Err(problem) => {
                let text = problem;
                state.set_status(&text);
            }
        }
    }

    // Every repository on disk, most recently used first. The settings window
    // shows all of them, while the top bar shows five.
    pub fn repositories(&self) -> Vec<Repo> {
        match self.state.lock() {
            Ok(state) => state.repositories.clone(),
            Err(_) => Vec::new(),
        }
    }

    // Start a chat in the place that is open now. The chat is blank, so it
    // goes away again when the user turns the carousel away without writing.
    pub fn new_chat(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.generating {
            state.set_status("wait for the answer to finish");
            return;
        }
        state.open_blank_chat();
    }

    // Turn the carousel by one chat. Minus one shows the chat at the left, and
    // plus one shows the chat at the right. A blank chat that is left behind
    // goes away after the move.
    pub fn roll(&self, step: i32) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.generating {
            state.set_status("wait for the answer to finish");
            return;
        }
        if state.chats.is_empty() {
            state.open_blank_chat();
            return;
        }
        let here = state
            .chats
            .iter()
            .position(|c| Some(&c.meta.id) == state.current.as_ref())
            .unwrap_or(0);
        let target = if step < 0 {
            here.saturating_sub((-step) as usize)
        } else {
            (here + step as usize).min(state.chats.len() - 1)
        };
        if target == here {
            state.set_status(if step < 0 {
                "this is the first chat of this place"
            } else {
                "this is the last chat of this place"
            });
            return;
        }
        // A blank chat is dropped, but only after the carousel moved.
        state.leave_blank();
        state.current = Some(state.chats[target].meta.id.clone());
        state.ctx_used = state.chats[target].meta.context_used;
        state.set_status("ready");
        state.touch_structure();
    }

    // Open one chat of the carousel.
    pub fn open_chat(&self, id: String) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.generating {
            state.set_status("wait for the answer to finish");
            return;
        }
        if !state.chats.iter().any(|c| c.meta.id == id) {
            return;
        }
        state.leave_blank();
        state.current = Some(id.clone());
        state.ctx_used = state
            .chats
            .iter()
            .find(|c| c.meta.id == id)
            .map(|c| c.meta.context_used)
            .unwrap_or(0);
        state.set_status("ready");
        state.touch_structure();
    }

    // Pick another model. The worker stops the running server.
    pub fn select_model(&self, id: String) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.generating {
            state.set_status("wait for the answer to finish");
            return;
        }
        state.cfg.model_id = id.clone();
        if let Some(chat) = state.chat_mut() {
            chat.meta.model_id = id.clone();
            state.persist();
        }
        let _ = config::save(&state.cfg);
        state.touch_structure();
        let _ = self.jobs.send(Job::ModelChanged);
    }

    // Store new settings, and refresh what depends on them.
    pub fn save_settings(&self, cfg: Config) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.generating {
            state.set_status("wait for the answer to finish");
            return;
        }
        state.cfg = cfg;
        let _ = config::save(&state.cfg);
        state.store = store::Store::new(config::repositories_dir(&state.cfg));
        state.refresh_lists();
        state.reload_chats();
        state.set_status("settings saved");
        let _ = self.jobs.send(Job::SettingsChanged);
    }

    // The colour palette. The browser page has this switch too.
    pub fn set_dark_mode(&self, dark: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.cfg.dark_mode == dark {
            return;
        }
        state.cfg.dark_mode = dark;
        let _ = config::save(&state.cfg);
        state.touch_structure();
    }

    // Settings for one model, with the devices of the machine.
    pub fn model_settings(&self, id: &str) -> (ModelSettings, Vec<Gpu>) {
        match self.state.lock() {
            Ok(state) => {
                let mut settings = state.cfg.model(id);
                settings.resize_gpus(state.gpus.len());
                (settings, state.gpus.clone())
            }
            Err(_) => (ModelSettings::default(), Vec::new()),
        }
    }

    // Save the settings of one model. A running server gets stopped, because it
    // was started with the old values.
    pub fn save_model_settings(&self, id: String, settings: ModelSettings) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.cfg.model_settings.insert(id.clone(), settings);
        let _ = config::save(&state.cfg);
        state.touch_structure();
        drop(state);
        let _ = self.jobs.send(Job::SettingsChanged);
    }

    // Ask the worker to stop, so the model is unloaded.
    pub fn shutdown(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        let _ = self.jobs.send(Job::Shutdown);
    }
}

impl Snap {
    // A snapshot for the rare case where the lock is gone.
    fn empty() -> Snap {
        Snap {
            model_name: String::new(),
            model_kind: String::new(),
            in_repository: false,
            repository_id: String::new(),
            repository_name: String::new(),
            chat_title: String::new(),
            context_used: 0,
            context_total: 0,
            generating: false,
            busy: false,
            code_enabled: false,
            sandbox_enabled: true,
            sandbox_available: false,
            status: String::new(),
            dark_mode: false,
            data_dir: String::new(),
            sandbox_note: String::new(),
            models: Vec::new(),
            model_id: String::new(),
            repositories: Vec::new(),
            chats: Vec::new(),
            chat_id: String::new(),
            chat_index: 0,
            chat_count: 0,
            blank_chat: true,
            messages: Vec::new(),
            live_text: String::new(),
            rev: 0,
            structure_rev: 0,
        }
    }
}

// The worker thread. It is the only code that talks to llama-server.
fn worker(state: Arc<Mutex<State>>, jobs: Receiver<Job>, cancel: Arc<AtomicBool>) {
    // The worker owns the server, so no other thread may stop it while a call
    // is in flight.
    let mut server: Option<Server> = None;
    loop {
        let job = match jobs.recv() {
            Ok(job) => job,
            // The engine went away, so take the model with it.
            Err(_) => {
                stop_server(&mut server);
                return;
            }
        };
        match job {
            Job::Shutdown => {
                stop_server(&mut server);
                return;
            }
            Job::ModelChanged | Job::SettingsChanged => {
                stop_server(&mut server);
                if let Ok(mut state) = state.lock() {
                    state.ctx_total = 0;
                    let name = state
                        .model()
                        .map(|m| m.name)
                        .unwrap_or_else(|| String::from("the model"));
                    let text = format!("{} loads with the next message", name);
                    state.set_status(&text);
                }
            }
            Job::Prompt(text) => {
                run_turn(&state, &mut server, &cancel, text);
            }
        }
    }
}

fn stop_server(server: &mut Option<Server>) {
    if let Some(mut old) = server.take() {
        old.stop();
    }
}

// Answer one prompt. When the model asks for a terminal command, the command
// runs and the model is asked again with the result.
fn run_turn(
    state: &Arc<Mutex<State>>,
    server: &mut Option<Server>,
    cancel: &Arc<AtomicBool>,
    text: String,
) {
    // Take the work order first, so two answers cannot mix.
    let (model, settings, workdir, sandbox_on) = {
        let Ok(mut state) = state.lock() else { return };
        if state.generating {
            return;
        }
        let Some(model) = state.model() else {
            state.set_status("pick a model first");
            return;
        };
        if state.chat_mut().is_none() {
            state.set_status("start a new chat first");
            return;
        }
        let sandbox_on = state
            .chat()
            .map(|c| c.meta.sandbox_enabled && state.sandbox_available)
            .unwrap_or(false);
        let workdir = state
            .chat()
            .map(|c| state.workdir_for(&c.clone()))
            .unwrap_or_else(|| config::paths().data);
        // The question goes into the chat at once.
        if let Some(chat) = state.chat_mut() {
            chat.messages.push(Message {
                id: store::new_id(),
                role: "You".to_string(),
                body: text.clone(),
                when: store::now_secs(),
                ..Default::default()
            });
            if chat.meta.title == "New chat" {
                chat.meta.title = Chat::title_from_prompt(&text);
            }
            chat.meta.updated = store::now_secs();
        }
        state.persist();
        state.generating = true;
        state.busy = true;
        state.live_text = String::new();
        state.set_status("the model is getting ready");
        let settings = state.settings_for(&model);
        (model, settings, workdir, sandbox_on)
    };

    // Make sure a server runs for this model. This can take a long time, and
    // the state lock is not held while it happens.
    let ready = ensure_server(state, server, &model, &settings, cancel);
    match ready {
        Ok(total) => {
            let Ok(mut state) = state.lock() else { return };
            state.busy = false;
            state.ctx_total = total;
            state.set_status("thinking");
        }
        Err(problem) => {
            let Ok(mut state) = state.lock() else { return };
            state.busy = false;
            state.generating = false;
            state.push_note(&problem, true);
            state.set_status("the model did not start");
            return;
        }
    }

    let base_url = server.as_ref().map(|s| s.url.clone()).unwrap_or_default();
    let mut steps = 0usize;
    loop {
        steps += 1;
        let (history, use_tools) = {
            let Ok(guard) = state.lock() else { return };
            let use_tools = guard
                .chat()
                .map(|c| c.meta.code_enabled)
                .unwrap_or(false);
            (guard.build_history(), use_tools)
        };
        let mut body = json!({
            "messages": history,
            "stream": true,
            // The usage numbers drive the context meter.
            "stream_options": { "include_usage": true },
        });
        if use_tools {
            body["tools"] = json!([tools::definition()]);
            body["tool_choice"] = json!("auto");
        }

        // Every piece of new text goes into the state, so the window can show
        // it while it arrives.
        let stream_state = state.clone();
        let mut last_update = Instant::now();
        let answer = llama::stream_chat(&base_url, &body, cancel, &mut |piece: &str| {
            if let Ok(mut guard) = stream_state.lock() {
                guard.live_text.push_str(piece);
                // A redraw every 60 ms is enough for the eye.
                if last_update.elapsed() > Duration::from_millis(60) {
                    last_update = Instant::now();
                    guard.touch();
                }
            }
        });

        let Ok(mut guard) = state.lock() else { return };
        let live = std::mem::take(&mut guard.live_text);
        // Both hold the answer. The longer one is the safer one.
        let answer_text = if answer.text.chars().count() >= live.chars().count() {
            answer.text.clone()
        } else {
            live
        };

        if let Some(problem) = &answer.error {
            let text = problem.clone();
            guard.push_note(&text, true);
        }
        if answer.stopped {
            let stopped = if answer_text.trim().is_empty() {
                String::from("(stopped)")
            } else {
                format!("{}\n(stopped)", answer_text)
            };
            guard.push_message("Assistant", &stopped, false);
        } else if !answer_text.trim().is_empty() {
            guard.push_message("Assistant", &answer_text, false);
        }

        // Tokens for the context meter.
        let used = answer.prompt_tokens + answer.completion_tokens;
        guard.ctx_used = if used > 0 {
            used
        } else {
            estimate_tokens(&history)
        };
        let used_now = guard.ctx_used;
        if let Some(chat) = guard.chat_mut() {
            chat.meta.context_used = used_now;
            chat.meta.updated = store::now_secs();
        }
        guard.persist();

        let calls: Vec<ToolCall> = answer
            .tool_calls
            .into_iter()
            .filter(|c| !c.name.is_empty())
            .collect();
        let finished = answer.stopped || answer.error.is_some() || calls.is_empty();
        if finished || !use_tools || steps >= MAX_TOOL_STEPS {
            guard.generating = false;
            let text = if answer.stopped {
                "stopped"
            } else if !finished && steps >= MAX_TOOL_STEPS {
                "the model asked for too many tool runs in one answer"
            } else {
                "ready"
            };
            guard.set_status(text);
            break;
        }
        // Keep the lock free while a command runs.
        drop(guard);

        // The model wants a command to run.
        for call in &calls {
            let (command, purpose) = tools::parse_arguments(&call.arguments);
            let shown = if purpose.trim().is_empty() {
                command.clone()
            } else {
                format!("{}\nwhy: {}", command, purpose)
            };
            let message_id = {
                let Ok(mut guard) = state.lock() else { continue };
                let id = guard.begin_tool(call, &shown);
                let text = format!("running a command: {}", first_line(&command));
                guard.set_status(&text);
                id
            };
            let outcome = tools::run(&command, &workdir, sandbox_on, TOOL_TIMEOUT);
            let mut output = outcome.summary();
            // The chat asked for the sandbox, so say when it was not used.
            if sandbox_on && !outcome.sandboxed {
                output = format!("{}\n(bubblewrap did not wrap this run)", output);
            }
            let Ok(mut guard) = state.lock() else { continue };
            guard.finish_tool(&message_id, &output, outcome.ok);
        }
        let Ok(mut guard) = state.lock() else { return };
        guard.set_status("the model reads the tool result");
    }
}

// Start llama-server when none runs yet, or when another model is wanted. The
// answer is the context size a chat gets.
fn ensure_server(
    state: &Arc<Mutex<State>>,
    server: &mut Option<Server>,
    model: &ModelInfo,
    settings: &ModelSettings,
    cancel: &Arc<AtomicBool>,
) -> Result<u64, String> {
    // A server for the same model is kept, so the model loads only once.
    let reusable = server.as_mut().map(|s| s.alive()).unwrap_or(false)
        && server
            .as_ref()
            .map(|s| s.model_id == model.id())
            .unwrap_or(false);
    if reusable {
        let total = server
            .as_ref()
            .map(|s| s.ctx_total)
            .unwrap_or(settings.ctx_size as u64);
        return Ok(total);
    }
    stop_server(server);
    if cancel.load(Ordering::Relaxed) {
        return Err("stopped before the model loaded".to_string());
    }
    let (binary, gpus, code_enabled) = {
        let Ok(state) = state.lock() else {
            return Err("the state is gone".to_string());
        };
        let binary = state
            .server_binary()
            .ok_or("llama-server was not found in the folder from the settings")?;
        (
            binary,
            state.gpus.clone(),
            state.chat().map(|c| c.meta.code_enabled).unwrap_or(false),
        )
    };
    match llama::Server::start(&binary, model, settings, &gpus, code_enabled) {
        Ok(started) => {
            let total = started.ctx_total;
            let notes = started.notes.clone();
            *server = Some(started);
            if let Ok(mut state) = state.lock() {
                for note in notes {
                    let text = note;
                    state.set_status(&text);
                }
            }
            Ok(total)
        }
        Err(problem) => {
            stop_server(server);
            Err(problem)
        }
    }
}

// Rough token count, used when the server gives no usage numbers.
fn estimate_tokens(history: &Value) -> u64 {
    let mut chars = 0u64;
    if let Some(items) = history.as_array() {
        for item in items {
            if let Some(text) = item.get("content").and_then(|c| c.as_str()) {
                chars += text.chars().count() as u64;
            }
            // Every message costs a few tokens on its own.
            chars += 8;
        }
    }
    chars / 4
}

fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    if line.chars().count() > 60 {
        let short: String = line.chars().take(60).collect();
        return format!("{}...", short);
    }
    line.to_string()
}

// Tests for the carousel of chats. They drive the engine the way a window
// does, and look at the snapshot that a window draws.
#[cfg(test)]
mod tests {
    use super::*;

    // The program keeps its paths in one place for the whole run, so the tests
    // share one data folder. They take turns, and each one empties the chats
    // folder first.
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn setup() {
        config::set_data_dir(PathBuf::from("uidata/engine-tests"));
        let _ = config::ensure_dirs();
        let chats = config::paths().chats;
        let _ = std::fs::create_dir_all(&chats);
        if let Ok(entries) = std::fs::read_dir(&chats) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    // A chat with one message, written on disk.
    fn write_chat(id: &str, updated: i64) {
        let store = store::Store::new(config::repositories_dir(&Config::default()));
        let mut chat = Chat::new("", "");
        chat.meta.id = id.to_string();
        chat.meta.title = id.to_string();
        chat.meta.updated = updated;
        let mut message = Message::default();
        message.role = String::from("You");
        message.body = String::from("A written question");
        message.when = updated;
        chat.messages.push(message);
        store.save_chat(&chat);
    }

    // A blank chat lives only in the memory, so it never reaches the disk. A
    // chat that was written in is stored at once.
    #[test]
    fn blank_chats_stay_off_disk() {
        let _guard = TURN.lock().unwrap();
        setup();
        let engine = Engine::start(Config::default());
        let chats = config::paths().chats;
        let stored = || std::fs::read_dir(&chats).map(|entries| entries.count()).unwrap_or(0);

        // The blank chat at the front is not a file.
        assert_eq!(engine.snapshot().chat_count, 1);
        assert_eq!(stored(), 0);

        // Even the badges may not write a blank chat to disk.
        engine.new_chat();
        engine.set_code(true);
        engine.set_sandbox(false);
        assert_eq!(stored(), 0);

        // The first message makes it a real chat.
        {
            let mut state = engine.state.lock().unwrap();
            state.push_message("You", "A real question", false);
        }
        assert_eq!(stored(), 1);

        // A fresh blank chat goes away when it is turned away from, and the
        // chat that was written in stays on the disk and in the carousel.
        engine.new_chat();
        assert_eq!(engine.snapshot().chat_count, 2);
        engine.roll(1);
        std::thread::sleep(Duration::from_millis(500));
        let after = engine.snapshot();
        assert_eq!(after.chat_count, 1);
        assert!(!after.blank_chat);
        assert_eq!(stored(), 1);
    }

    #[test]
    fn carousel_rules() {
        let _guard = TURN.lock().unwrap();
        setup();
        write_chat("aaaabbbb01", 100);
        write_chat("aaaabbbb02", 200);

        // The first thing the user sees is a blank chat at the front.
        let engine = Engine::start(Config::default());
        let snap = engine.snapshot();
        assert_eq!(snap.chat_count, 3);
        assert_eq!(snap.chat_index, 0);
        assert!(snap.blank_chat);
        assert!(snap.chats[0].transient);

        // Asking for a new chat twice still gives one blank chat.
        engine.new_chat();
        engine.new_chat();
        assert_eq!(engine.snapshot().chat_count, 3);

        // A blank chat that is turned away from goes away after the move.
        engine.roll(1);
        assert_eq!(engine.snapshot().chat_count, 3);
        std::thread::sleep(Duration::from_millis(500));
        let after = engine.snapshot();
        assert_eq!(after.chat_count, 2);
        assert_eq!(after.chat_index, 0);
        assert_eq!(after.chat_id, "aaaabbbb02");
        assert!(!after.blank_chat);

        // A blank chat that was used stays, like any other chat.
        engine.new_chat();
        {
            let mut state = engine.state.lock().unwrap();
            state.push_message("You", "Hello there", false);
        }
        engine.roll(1);
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(engine.snapshot().chat_count, 3);

        // Every repository has its own carousel.
        engine.create_repository(String::from("Test Repo"));
        let repo = engine.snapshot();
        assert!(repo.in_repository);
        assert_eq!(repo.chat_count, 1);
        assert!(repo.blank_chat);
        engine.select_repository(String::new());
        let back = engine.snapshot();
        assert!(!back.in_repository);
        assert_eq!(back.chat_count, 3);
    }
}
