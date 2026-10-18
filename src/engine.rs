// The engine. It owns the settings, the chats, the model list and the
// llama-server process. The window and the browser page both drive this one
// engine, so both show the same thing.
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

use crate::attach;
use crate::store::Attachment;
use crate::config::{self, Config, ModelSettings};
use crate::gpu::{self, Gpu};
use crate::llama::{self, ModelInfo, Server, ToolCall};
use crate::markdown;
use crate::store::{self, Chat, Message, Repo};
use crate::tools;

// Longest wait for one terminal command.
const TOOL_TIMEOUT: Duration = Duration::from_secs(120);
// How many terminal commands one answer may run, and how many round trips one
// answer may take in all, both read from the Chatting settings. A mark of a
// chapter is not a command: it runs nothing, so it eats neither room. The two
// settings stand unlimited for a new set of settings, and the answer then ends
// only when the model stops by itself.
//
// A ceiling of nothing is a poor thing to leave in an answer, so the two rooms
// are never smaller than this when the reader sets one: one round trip for the
// turn that asks, and one command.
const LEAST_TOOL_STEPS: usize = 1;
const LEAST_ANSWER_STEPS: usize = 2;

// One send of the chat box: the words of the reader, and the files they hung
// on it, in the order the reader hung them.
struct SentMessage {
    text: String,
    files: Vec<attach::Incoming>,
}

// Work for the worker thread.
enum Job {
    // The user asked something, with the files of the message beside it.
    Prompt(SentMessage),
    // Another model is wanted, so the running server must go.
    ModelChanged,
    // Settings changed, so a server must not stay with the old values.
    SettingsChanged,
    // The thinking mode changed, so the server must go with its old flags.
    ThinkingChanged,
    // Leave the process, after the model is unloaded.
    Shutdown,
}

// One chapter of a chat, as the window needs it. A chapter is kept in the chat
// as a message of its own, so it stands where the model asked for it, and the
// window draws it as a node on the chapter line and never as a message.
#[derive(Clone, Debug, Default)]
pub struct Chapter {
    // The id of the message the chapter stands on.
    pub id: String,
    // The name the model gave the chapter, no longer than the tool allows.
    pub name: String,
    // The id of the message the chapter begins after. Empty when the chapter
    // stands at the very top of the chat.
    pub after: String,
}

// One call of a tool the model is writing right now. The reader sees the words
// of it as they come, so the block stands in the chat before the call is over
// and before the command it asks for has run. The words are those the reader is
// given: the command, and the reason the model gave for it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LiveTool {
    // The name of the tool, as the model wrote it.
    pub name: String,
    // The command so far. It is read out of the words of the call, which are
    // often not finished.
    pub command: String,
    // The reason the model gave for the command, as far as it has come.
    pub purpose: String,
}

// The calls the reader may watch being written. A mark of a chapter is not
// shown, so a call that could still become one stands aside until its name is
// known: the name comes in pieces, and three letters of it are not yet the name
// of a tool. A call with no name of its own is therefore kept out too.
fn watched(calls: &[ToolCall]) -> Vec<LiveTool> {
    let mut out: Vec<LiveTool> = Vec::new();
    for call in calls {
        if tools::CHAPTER_TOOL_NAME.starts_with(call.name.as_str()) {
            continue;
        }
        let (command, purpose) = tools::partial_arguments(&call.arguments);
        out.push(LiveTool {
            name: call.name.clone(),
            command,
            purpose,
        });
    }
    out
}

// The chapters of one chat, in the order they were marked. A note is not a
// place a chapter can begin after, for the window puts notes between the turns
// and they come and go with them.
fn chapter_marks(chat: &Chat) -> Vec<Chapter> {
    let mut out: Vec<Chapter> = Vec::new();
    let mut before = String::new();
    for message in &chat.messages {
        // A mark with no name is no chapter: the reader would see a node with
        // nothing to read on it. A mark of either kind is never a place another
        // chapter begins after.
        if message.role == "Chapter" {
            if !message.body.trim().is_empty() {
                out.push(Chapter {
                    id: message.id.clone(),
                    name: message.body.clone(),
                    after: before.clone(),
                });
            }
        } else if message.role != "Note" {
            before = message.id.clone();
        }
    }
    out
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
    // The thinking mode of the open chat.
    pub thinking_mode: String,
    // True when the model's template can switch thinking.
    pub thinking_available: bool,
    // The thinking modes the badge offers, in the order they are listed. An
    // empty list means the model has no thinking mode at all.
    pub thinking_modes: Vec<String>,
    // Whether the open chat keeps the thinking of the turns before the last
    // answer in the words the model is sent.
    pub thinking_preserve: bool,
    // True when the template of the model offers that choice.
    pub thinking_preserve_available: bool,
    pub status: String,
    pub dark_mode: bool,
    pub data_dir: String,
    // What the sandbox can do on this machine, for the settings window.
    pub sandbox_note: String,
    // Chatting. Whether a thinking panel is folded up while it is written.
    pub chat_collapse_thinking: bool,
    // Whether a tool panel is folded up while its command runs.
    pub chat_collapse_tools: bool,
    // Whether a block of code takes the colours of the kinds of word in it.
    pub chat_highlight: bool,
    // Whether the model may mark the chapters of a chat, and whether the
    // chapter line stands beside it.
    pub chat_chapters: bool,
    // The number of characters where a paste of words into the chat box turns
    // into a file of its own.
    pub chat_paste_limit: u32,
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
    // True when no chat is open. The window then shows the chat box in the
    // middle of the screen, with the program mark and a line for this hour.
    pub home: bool,
    // True when the chats are shown as small previews.
    pub overview: bool,
    // The line above the chat box while no chat is open.
    pub greeting: String,
    pub messages: Vec<Message>,
    // The chapters of the open chat, in the order the model marked them.
    pub chapters: Vec<Chapter>,
    // Text the model is writing right now, while an answer comes in.
    pub live_text: String,
    // The model's reasoning, streamed while an answer comes in.
    pub live_reasoning: String,
    // How long the thinking of the answer taking shape lasted, in
    // milliseconds, and whether it has stopped. The page writes these on the
    // head of the panel.
    pub live_thought_ms: u64,
    pub live_thought_done: bool,
    // How far the server is with the words it was sent, from 0 to 100, and
    // whether the gauge above the chat box stands. The gauge stands from the
    // moment a request goes out until the model has a word to give back.
    pub progress: u32,
    pub progress_on: bool,
    // How many pictures the message being worked through carries. The program
    // sends words today, so the number is nothing. The words of the gauge
    // follow it, so a chat that can carry a picture says so by itself.
    pub progress_images: usize,
    // The calls of a tool the model is writing right now, in the order it
    // writes them. A block of a command being written is made of these.
    pub live_tools: Vec<LiveTool>,
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
    // The model's reasoning, streamed while an answer comes in.
    pub live_reasoning: String,
    // Whether the thinking that came with the live answer has stopped. The
    // head of the panel says how long it took once it has.
    pub live_thought_done: bool,
    // How long the thinking of the live answer lasted, in milliseconds.
    pub live_thought_ms: u64,
    // How far the server is with the words it was sent, from 0 to 100, and
    // whether the gauge above the chat box stands.
    pub progress: u32,
    pub progress_on: bool,
    // How many pictures the message being worked through carries.
    pub progress_images: usize,
    // The calls of a tool the model is writing right now, in the order the
    // model writes them. The reader sees a command as it is written.
    pub live_tools: Vec<LiveTool>,
    // The moments the thinking of the live answer began and last stopped. The
    // two together are the length of it.
    thought_start: Option<Instant>,
    thought_end: Option<Instant>,
    pub generating: bool,
    // The model is loading, so the window shows a spinner.
    pub busy: bool,
    pub status: String,
    pub ctx_total: u64,
    pub ctx_used: u64,
    pub sandbox_available: bool,
    pub sandbox_note: String,
    // What the chat template of the current model can do with thinking.
    pub thinking: markdown::ThinkingOptions,
    pub rev: u64,
    pub structure_rev: u64,
    // True while the chats are shown as small previews.
    pub overview: bool,
    // The chat settings a new chat starts with. They matter while no chat is
    // open, where a badge has no chat to change.
    pub new_code: bool,
    pub new_sandbox: bool,
    pub new_thinking: String,
    pub new_preserve: bool,
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

    // The choice to keep the thinking of the turns before the last answer, for
    // the chat in hand. With no chat open, the choice the badge holds for the
    // next chat is used, so the badge and the flags of the server cannot drift
    // apart.
    fn preserve_of_open(&self) -> bool {
        self.chat()
            .map(|c| c.meta.preserve_thinking)
            .unwrap_or(self.new_preserve)
    }

    // Whether the thoughts of the turns before the last answer go back to the
    // model with the words of the chat. The flag of the server only says that
    // the template may keep them; the thoughts themselves live in the words
    // sent, for the template reads them out of each answer and out of nowhere
    // else. So a chat that keeps them is not enough: the thinking of an answer
    // has to travel with it, and a model whose template cannot keep any does
    // not get the words either.
    fn kept_thinking(&self) -> bool {
        self.thinking.has_preserve && self.preserve_of_open()
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
    fn push_message(
        &mut self,
        role: &str,
        body: &str,
        reasoning: &str,
        reasoning_ms: u64,
        mono: bool,
    ) {
        if let Some(chat) = self.chat_mut() {
            chat.messages.push(Message {
                id: store::new_id(),
                role: role.to_string(),
                body: body.to_string(),
                reasoning: reasoning.to_string(),
                reasoning_ms,
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

    // Mark the start of a chapter of the chat. The mark is a message of its
    // own, so it keeps its place in the talk, but it is never drawn: the window
    // shows it as a node on the chapter line beside the chat. The model is
    // answered that the mark was taken, and asked again for a name when it sent
    // none.
    fn add_chapter(&mut self, call: &ToolCall, name: &str) {
        let answer = if name.is_empty() {
            "the chapter needs a name, so it was not marked"
        } else {
            "the chapter was marked"
        };
        let Some(chat) = self.chat_mut() else {
            // The chat was put away between the call and the mark, so there is
            // nowhere for the mark to stand.
            return;
        };
        chat.messages.push(Message {
            id: store::new_id(),
            role: "Chapter".to_string(),
            body: name.to_string(),
            tool: answer.to_string(),
            call_id: call.id.clone(),
            tool_name: call.name.clone(),
            tool_args: call.arguments.clone(),
            when: store::now_secs(),
            ..Default::default()
        });
        self.persist();
        self.touch();
    }

    // Put a tool run in the chat before it starts. The id is given back so the
    // output can be written into the same message. The thinking of a turn with
    // no words of its own stands here too, so it is not lost.
    fn begin_tool(
        &mut self,
        call: &ToolCall,
        shown: &str,
        thinking: &str,
        thinking_ms: u64,
    ) -> String {
        let id = store::new_id();
        if let Some(chat) = self.chat_mut() {
            chat.messages.push(Message {
                id: id.clone(),
                role: "Tool".to_string(),
                body: shown.to_string(),
                reasoning: thinking.to_string(),
                reasoning_ms: thinking_ms,
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

    // True when the model can be shown a picture: the vision switch of the
    // model settings stands on, and a projector file stands next to the model
    // for the server to take. The server gets its `--mmproj` from the same
    // two facts, so the answer here and the flags there cannot drift apart.
    fn pictures_can_go(&self) -> bool {
        match self.model() {
            Some(model) => {
                self.cfg.model(&model.id()).vision && llama::find_mmproj(&model.path).is_some()
            }
            None => false,
        }
    }

    // Turn the open chat into the message list the API expects.
    fn build_history(&self) -> Value {
        let mut out: Vec<Value> = Vec::new();
        let Some(chat) = self.chat() else {
            return json!([]);
        };
        let vision_on = self.pictures_can_go();
        // The records of the attachments of this chat stand in a folder of
        // its own room, so that is where their words and pictures are looked
        // for.
        let shelf = self.attach_place_for(chat);
        // The thoughts that may go back with the answers of the chat.
        let keep_thinking = self.kept_thinking();
        for message in &chat.messages {
            match message.role.as_str() {
                // The words of the reader, and the attachments that hang on
                // them. Without attachments this is the plain words, as it
                // always was.
                "You" => out.push(json!({
                    "role": "user",
                    "content": attach::content_at(&shelf, &message.body, &message.attachments, vision_on),
                })),
                "Assistant" => {
                    let mut entry = json!({
                        "role": "assistant",
                        "content": message.body,
                    });
                    // The thinking of the answer rides beside its words, so the
                    // template may put it back where it stood. An answer that
                    // thought nothing sends nothing, for the empty block of a
                    // turn is not worth the room.
                    if keep_thinking && !message.reasoning.trim().is_empty() {
                        entry["reasoning_content"] = json!(message.reasoning);
                    }
                    out.push(entry);
                }
                // A tool run and a chapter are both a call the model made and
                // an answer it got back. Only the run is shown to the reader.
                "Tool" | "Chapter" => {
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
                    let mut call = json!({
                        "role": "assistant",
                        "content": Value::Null,
                        "tool_calls": [{
                            "id": message.call_id,
                            "type": "function",
                            "function": { "name": name, "arguments": message.tool_args }
                        }]
                    });
                    // The thinking of a turn that asked for a command stands
                    // with the command, so it goes back the same way.
                    if keep_thinking && !message.reasoning.trim().is_empty() {
                        call["reasoning_content"] = json!(message.reasoning);
                    }
                    out.push(call);
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
        // The badge no longer offers "auto": the reader chooses, and thinking
        // is off unless it is asked for. A chat left with the old mode starts
        // with thinking off, which is what the badge then shows.
        for chat in self.chats.iter_mut() {
            if chat.meta.thinking_mode.is_empty() || chat.meta.thinking_mode == "auto" {
                chat.meta.thinking_mode = String::from("off");
            }
        }
        // Keep the open chat when it is still there.
        if let Some(id) = self.current.clone() {
            if !self.chats.iter().any(|c| c.meta.id == id) {
                self.current = None;
            }
        }
        self.pending_blank = None;
        // A new place is looked at as one chat, not as a row of previews.
        self.overview = false;
        // A place with no chat at all is left empty. The window then shows the
        // chat box on its own, and the first message starts the first chat.
        // When the place has chats, the newest one is opened.
        if self.chats.is_empty() {
            self.current = None;
            self.ctx_used = 0;
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
        // front. With nothing at the front, the choice the badges hold for the
        // next chat is used. The terminal tool starts off unless it was asked
        // for while no chat was open.
        match self.chats.first() {
            Some(previous) => chat.meta.sandbox_enabled = previous.meta.sandbox_enabled,
            None => chat.meta.sandbox_enabled = self.new_sandbox,
        }
        chat.meta.code_enabled = self.new_code;
        chat.meta.thinking_mode = self.new_thinking.clone();
        chat.meta.preserve_thinking = self.new_preserve;
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
        self.store
            .delete_chat(&dropped.meta.repo_id, &dropped.meta.id);
        self.touch_structure();
    }

    // Take away a blank chat that the user turned away from. This waits until
    // the carousel had the time to move, so the move stays smooth.
    fn prune_blank(&mut self) {
        let Some((id, deadline)) = self.pending_blank.clone() else {
            return;
        };
        if Instant::now() < deadline {
            return;
        }
        self.pending_blank = None;
        self.drop_blank(&id);
    }

    // Note that the open chat is a blank one, in case the user leaves it now.
    fn leave_blank(&mut self) {
        let Some(id) = self.current.clone() else {
            return;
        };
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
        self.adopt_default_model();
        self.touch_structure();
    }

    // Point at a model when nothing sensible is picked yet. A model that was
    // picked is kept. Without this the top bar shows a model that no chat uses,
    // and a message then fails with "pick a model in the top bar first".
    fn adopt_default_model(&mut self) {
        let wanted = self.cfg.model_id.clone();
        let known = self.models.iter().any(|m| m.id() == wanted);
        if wanted.is_empty() || !known {
            if let Some(first) = self.models.first() {
                let id = first.id();
                self.cfg.model_id = id;
                let _ = config::save(&self.cfg);
            }
        }
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

    // Read the chat template the model will use, and what it can do with
    // thinking. A custom template from the settings wins over the one the
    // model file carries.
    fn refresh_thinking(&mut self) {
        let model_id = self.current_model_id();
        let custom = self.cfg.model(&model_id).chat_template;
        let template: Option<String> = if !custom.trim().is_empty() {
            Some(custom)
        } else {
            self.model()
                .and_then(|m| crate::gguf::read_string(&m.path, "tokenizer.chat_template"))
        };
        self.thinking = match template {
            Some(text) => markdown::analyze_template(&text),
            None => markdown::ThinkingOptions::default(),
        };
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

    // The folder the records of the attachments of a chat stand in: the folder
    // `.attachments` in the room of the chat. A chat and its files are opened
    // again together, and go together when the reader moves the repository,
    // and a command of the tool may read a file the reader hung on a message,
    // for it works in the same room.
    fn attach_place_for(&self, chat: &Chat) -> PathBuf {
        attach::shelf(&self.workdir_for(chat))
    }

    // The records of the attachments used to stand in one flat folder under
    // the data directory, with nothing in it to say which chat held them. When
    // such a folder stands, the records are moved into the room of the chat
    // that carries their marks, and the old folder goes when it holds nothing
    // left. This runs once, when the program starts.
    fn migrate_attachments(&mut self) {
        if !attach::old_dir().is_dir() {
            return;
        }
        let work = config::paths().data.join("work");
        // Every chat of a repository shares the folder of that repository.
        for repo in self.store.scan_repos() {
            let Some(room) = self.store.repo_path(&repo.id) else {
                continue;
            };
            let mut ids: Vec<String> = Vec::new();
            for chat in self.store.list_chats(&repo.id) {
                for id in chat.attachment_ids() {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            }
            attach::move_old(&attach::shelf(&room), &ids);
        }
        // A chat with no repository has a folder of its own.
        for chat in self.store.list_chats("") {
            let ids = chat.attachment_ids();
            if ids.is_empty() {
                continue;
            }
            attach::move_old(&attach::shelf(&work.join(&chat.meta.id)), &ids);
        }
        attach::drop_old_dir();
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
            model_kind: model
                .as_ref()
                .map(|m| m.kind.to_string())
                .unwrap_or_default(),
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
            code_enabled: chat.map(|c| c.meta.code_enabled).unwrap_or(self.new_code),
            sandbox_enabled: chat
                .map(|c| c.meta.sandbox_enabled)
                .unwrap_or(self.new_sandbox),
            sandbox_available: self.sandbox_available,
            thinking_mode: chat
                .map(|c| c.meta.thinking_mode.clone())
                .unwrap_or_else(|| self.new_thinking.clone()),
            thinking_available: self.thinking.has_thinking || self.thinking.has_effort,
            thinking_modes: self.thinking.modes().iter().map(|m| m.to_string()).collect(),
            thinking_preserve: self.preserve_of_open(),
            thinking_preserve_available: self.thinking.has_preserve,
            status: self.status.clone(),
            dark_mode: self.cfg.dark_mode,
            data_dir: config::paths().data.display().to_string(),
            sandbox_note: self.sandbox_note.clone(),
            chat_collapse_thinking: self.cfg.chat_collapse_thinking,
            chat_collapse_tools: self.cfg.chat_collapse_tools,
            chat_highlight: self.cfg.chat_highlight,
            chat_chapters: self.cfg.chat_chapters,
            chat_paste_limit: self.cfg.pasted_text_char_limit,
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
            home: self.current.is_none(),
            overview: self.overview,
            greeting: crate::greet::greeting(store::now_secs()),
            // The marks of the chapters are not words of the talk: they are
            // given on the side, and kept out of the messages the window draws.
            messages: chat
                .map(|c| {
                    c.messages
                        .iter()
                        .filter(|m| m.role != "Chapter")
                        .cloned()
                        .collect()
                })
                .unwrap_or_default(),
            chapters: chat.map(chapter_marks).unwrap_or_default(),
            live_text: self.live_text.clone(),
            live_reasoning: self.live_reasoning.clone(),
            live_thought_ms: self.live_thought_ms,
            live_thought_done: self.live_thought_done,
            progress: self.progress,
            progress_on: self.progress_on,
            progress_images: self.progress_images,
            live_tools: self.live_tools.clone(),
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
        // The badge stands on only when the jail has been tried and kept what
        // it promises, so a machine with bubblewrap installed and user
        // namespaces switched off says so instead of running commands free.
        let sandbox_note = match tools::jail_check() {
            Ok(()) => String::from(
                "bubblewrap works. A command sees only the working directory of the chat, \
                 and it has no network.",
            ),
            Err(problem) => problem,
        };
        let sandbox_available = tools::jail_check().is_ok();
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
            live_reasoning: String::new(),
            live_thought_done: false,
            live_thought_ms: 0,
            progress: 0,
            progress_on: false,
            progress_images: 0,
            live_tools: Vec::new(),
            thought_start: None,
            thought_end: None,
            generating: false,
            busy: false,
            status: String::from("ready"),
            ctx_total: 0,
            ctx_used: 0,
            sandbox_available,
            sandbox_note,
            thinking: markdown::ThinkingOptions::default(),
            rev: 0,
            structure_rev: 0,
            pending_blank: None,
            overview: false,
            new_code: false,
            new_sandbox: true,
            new_thinking: String::from("off"),
            // llama.cpp keeps the thinking of the earlier turns on its own, so
            // the program asks for the same thing and lets the reader turn it
            // off for a model that offers the choice.
            new_preserve: true,
        };
        state.refresh_lists();
        state.migrate_attachments();
        state.reload_chats();
        // The first screen holds no chat at all, even when this place has
        // chats. The window shows the chat box on its own, the first message
        // opens a new chat, and the previews hold the chats that exist.
        state.current = None;
        state.ctx_used = 0;
        state.refresh_thinking();
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

    // Ask the model something, with the files the reader hung on the message
    // beside the words. The worker thread does the work. A message of nothing
    // at all is refused; a message of files and no words is a real message.
    pub fn prompt(&self, text: String, files: Vec<attach::Incoming>) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if text.trim().is_empty() && files.is_empty() {
            state.set_status("nothing was written");
            return;
        }
        if state.generating {
            state.set_status("an answer is still coming");
            return;
        }
        // A message sent from the first screen opens a chat of this place. The
        // window grows the chat box into a chat at the same time.
        if state.current.is_none() {
            state.open_blank_chat();
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
        let _ = self
            .jobs
            .send(Job::Prompt(SentMessage { text, files }));
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
        } else {
            // No chat is open, so the choice is kept for the next one.
            state.new_code = enabled;
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
        } else {
            state.new_sandbox = enabled;
        }
        state.persist();
        state.set_status(if enabled {
            "tool runs are sandboxed"
        } else {
            "tool runs are not sandboxed"
        });
    }

    // Choose the thinking mode of the open chat. A model with no thinking
    // mode does not change, and neither does a mode the model does not offer.
    pub fn set_thinking(&self, mode: &str) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.generating {
            state.set_status("wait for the answer to finish");
            return;
        }
        if !state.thinking.modes().contains(&mode) {
            return;
        }
        if let Some(chat) = state.chat_mut() {
            chat.meta.thinking_mode = mode.to_string();
        } else {
            state.new_thinking = mode.to_string();
        }
        state.persist();
        state.set_status(&format!("thinking is {}", mode));
        state.touch();
        drop(state);
        // The flags are server level, so the running server goes with them.
        let _ = self.jobs.send(Job::ThinkingChanged);
    }

    // Turn the keeping of the thinking of the earlier turns on or off, for the
    // chat in hand. A model whose template cannot keep its thinking does not
    // change, and neither does a chat while an answer comes in. Keeping costs
    // tokens, for the thoughts of the chat go back to the model with every
    // turn.
    pub fn set_preserve(&self, on: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.generating {
            state.set_status("wait for the answer to finish");
            return;
        }
        if !state.thinking.has_preserve {
            state.set_status("this model has no switch for keeping the thinking");
            return;
        }
        let changed = state.preserve_of_open() != on;
        if let Some(chat) = state.chat_mut() {
            chat.meta.preserve_thinking = on;
        } else {
            state.new_preserve = on;
        }
        state.persist();
        state.set_status(if on {
            "the thinking of the earlier turns is kept"
        } else {
            "the thinking of the earlier turns is dropped"
        });
        state.touch();
        drop(state);
        // The flag stands in the command line of the server, so the running
        // server goes with it. A choice that did not move leaves the server be.
        if changed {
            let _ = self.jobs.send(Job::ThinkingChanged);
        }
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

    // The words of one attachment, for the page that shows the block. A
    // window asks by the id the state gave with the message. Nothing is
    // known of the reader who asks: the record is the record.
    pub fn attachment(&self, id: &str) -> Option<attach::Stored> {
        // The record of an attachment stands with the chat that holds it, so
        // the chat that is open is looked in first, and the rooms of the other
        // chats after it.
        if let Ok(keeper) = self.state.lock() {
            if let Some(chat) = keeper.chat() {
                if let Some(record) = attach::read_at(&keeper.attach_place_for(chat), id) {
                    return Some(record);
                }
            }
        }
        let paths = config::paths();
        attach::find_in(&[paths.data.join("work"), paths.repositories], id)
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
        // The new chat is wanted in full, so the previews are put away.
        state.overview = false;
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
        // Only the messages changed. The list of chats keeps its shape, so the
        // window slides the cards instead of building them again.
        state.touch();
    }

    // Show the chats of this place as small previews, or put them away again.
    // While the previews are up, the chat in the middle of them is the chat the
    // user is pointing at, so it is also the chat that is open.
    pub fn set_overview(&self, on: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.generating {
            state.set_status("wait for the answer to finish");
            return;
        }
        if state.overview == on {
            return;
        }
        state.overview = on;
        if on && state.current.is_none() && !state.chats.is_empty() {
            state.current = Some(state.chats[0].meta.id.clone());
            state.ctx_used = state.chats[0].meta.context_used;
        }
        state.touch();
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
        state.overview = false;
        state.set_status("ready");
        state.touch();
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
        let mut cfg = cfg;
        // The chat box turns a long paste of words into a file, and the number
        // it does this at is kept inside the two bounds here.
        cfg.pasted_text_char_limit = config::sanitize_pasted_text_limit(cfg.pasted_text_char_limit);
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
            thinking_mode: String::from("off"),
            thinking_modes: Vec::new(),
            thinking_available: false,
            thinking_preserve: true,
            thinking_preserve_available: false,
            status: String::new(),
            dark_mode: false,
            data_dir: String::new(),
            sandbox_note: String::new(),
            chat_collapse_thinking: true,
            chat_collapse_tools: true,
            chat_highlight: true,
            chat_chapters: true,
            chat_paste_limit: 6000,
            models: Vec::new(),
            model_id: String::new(),
            repositories: Vec::new(),
            chats: Vec::new(),
            chat_id: String::new(),
            chat_index: 0,
            chat_count: 0,
            blank_chat: true,
            home: true,
            overview: false,
            greeting: String::new(),
            messages: Vec::new(),
            chapters: Vec::new(),
            live_text: String::new(),
            live_reasoning: String::new(),
            live_thought_ms: 0,
            live_thought_done: false,
            progress: 0,
            progress_on: false,
            progress_images: 0,
            live_tools: Vec::new(),
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
                    state.refresh_thinking();
                    let name = state
                        .model()
                        .map(|m| m.name)
                        .unwrap_or_else(|| String::from("the model"));
                    let text = format!("{} loads with the next message", name);
                    state.set_status(&text);
                }
            }
            Job::ThinkingChanged => {
                // The flags are server level, so the running server goes.
                // The model and context stay the same, so nothing else changes.
                stop_server(&mut server);
            }
            Job::Prompt(sent) => {
                run_turn(&state, &mut server, &cancel, sent);
            }
        }
    }
}

fn stop_server(server: &mut Option<Server>) {
    if let Some(mut old) = server.take() {
        old.stop();
    }
}

// What one answer of the model leaves in the chat.
#[derive(Debug, PartialEq, Eq)]
enum Kept {
    // The words of the answer.
    Words,
    // No words came in, but the thinking of the turn is kept.
    Thinking,
    // A quiet line for a turn that left nothing to read.
    Note,
    // Nothing at all. A command, or a stop with nothing in it, needs no line
    // about itself.
    Nothing,
}

// Split the calls of one step into the marks of chapters and the commands the
// model wants run. A mark costs no room for commands, so a talk with many
// chapters is not cut short by the command ceiling.
fn split_calls(calls: Vec<ToolCall>) -> (Vec<ToolCall>, Vec<ToolCall>) {
    let mut marks: Vec<ToolCall> = Vec::new();
    let mut runs: Vec<ToolCall> = Vec::new();
    for call in calls {
        if call.name == tools::CHAPTER_TOOL_NAME {
            marks.push(call);
        } else {
            runs.push(call);
        }
    }
    (marks, runs)
}

// The model has a word to give back, so the words it was sent are worked
// through and the gauge above the chat box has nothing left to say. True when
// the gauge stood, so the caller says that the messages changed.
fn finish_progress(state: &mut State) -> bool {
    if !state.progress_on {
        return false;
    }
    state.progress_on = false;
    // The number is put at the end on purpose: a gauge that goes away has
    // finished its work, and the reader may still see it for a moment.
    state.progress = 100;
    true
}

// The model wrote something of a call. A call cannot be written before the
// server has the whole of the words it was sent through, so the gauge above the
// chat box has nothing left to say from this moment: it stood for the work
// before the first word of the answer, and a model that asks for a command
// before it has said anything would else leave it standing over the chat until
// the command had run. The words of the call are put into the state as well, so
// the reader sees the command grow. True when the window has something new to
// draw.
fn write_calls(state: &mut State, calls: &[ToolCall]) -> bool {
    let made = watched(calls);
    // The gauge leaves for a mark of a chapter too. Such a call gives the
    // reader no block, but the model wrote it after the words it was sent.
    let gauge = finish_progress(state);
    let grew = made != state.live_tools;
    if grew {
        state.live_tools = made;
    }
    gauge || grew
}

// The meter of the context while an answer comes in. The server says how many
// words the talk takes on every piece of the answer, so the meter moves during
// the answer and not only when the answer is over. The meter only climbs: the
// words of an answer that is still coming are not taken back. The number the
// server gives at the end of the answer is kept for the end, and that number
// may be smaller, for a stop takes words away. True when the number moved, so
// the caller says that the messages changed.
fn climb_context(state: &mut State, made: u64) -> bool {
    if made == 0 || made <= state.ctx_used {
        return false;
    }
    state.ctx_used = made;
    true
}

// How many pictures the message that is being worked through carries. That is
// the last thing that goes out with the request. The program sends words today,
// so the answer is nothing. The count is taken from the words that go out, and
// not from a guess about the number of tokens, so the gauge says the right thing
// on the day a chat can carry a picture.
fn pending_images(history: &Value) -> usize {
    let Some(last) = history.as_array().and_then(|items| items.last()) else {
        return 0;
    };
    let Some(parts) = last.get("content").and_then(|c| c.as_array()) else {
        return 0;
    };
    parts
        .iter()
        .filter(|part| part.get("type").and_then(|t| t.as_str()) == Some("image_url"))
        .count()
}

// Put the marks of one step into the chat, and count them. They are put in the
// order the model asked for them, and each stands where its call came.
fn take_marks(state: &Arc<Mutex<State>>, calls: &[ToolCall], made: &mut usize) {
    for call in calls {
        let name = tools::chapter_name(&call.arguments);
        let Ok(mut guard) = state.lock() else {
            return;
        };
        guard.add_chapter(call, &name);
        *made += 1;
    }
}

// Choose what one answer leaves. `stopped` says the reader stopped the run,
// and `error` says the run failed. A command is an answer of its own, so a
// turn that asks for one writes no line, and a stop is not written down
// either. The thinking of a turn with no words goes to the command it led to.
fn kept_of(
    text: &str,
    thinking: &str,
    stopped: bool,
    asked_for_tool: bool,
    error: bool,
) -> Kept {
    if !text.trim().is_empty() {
        return Kept::Words;
    }
    if asked_for_tool {
        return Kept::Nothing;
    }
    if stopped {
        return if thinking.trim().is_empty() {
            Kept::Nothing
        } else {
            Kept::Thinking
        };
    }
    if error {
        // The reason of the failure already stands as a note of its own.
        return Kept::Nothing;
    }
    Kept::Note
}

// Answer one prompt. When the model asks for a terminal command, the command
// runs and the model is asked again with the result. The files the reader
// hung on the message are written to disk first, and the message of the
// reader carries their marks, in the order the reader hung them.
fn run_turn(
    state: &Arc<Mutex<State>>,
    server: &mut Option<Server>,
    cancel: &Arc<AtomicBool>,
    sent: SentMessage,
) {
    // The files wait in the folder of the chat that carries them, so the
    // model can be asked again about them after a restart of the program, and
    // a command of the tool can read them in the room it works in.
    let shelf = state
        .lock()
        .ok()
        .and_then(|keeper| keeper.chat().map(|chat| keeper.attach_place_for(chat)));
    let kept: Vec<Attachment> = match &shelf {
        Some(place) => attach::save_at(place, &sent.files),
        // No chat to carry them, so there is no room to keep them in.
        None => Vec::new(),
    };
    let lost = sent.files.len().saturating_sub(kept.len());
    let text = sent.text;
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
        // What the chat asks for, and nothing else. Whether the jail really
        // works on this machine is looked at by the tool itself, which refuses
        // the run when the jail cannot be had. A chat that wants the sandbox is
        // never served a plain run because the jail is broken.
        let sandbox_on = state
            .chat()
            .map(|c| c.meta.sandbox_enabled)
            .unwrap_or(false);
        let workdir = state
            .chat()
            .map(|c| state.workdir_for(&c.clone()))
            .unwrap_or_else(|| config::paths().data);
        // The question goes into the chat at once, with its attachments
        // hanging on it in the order the reader hung them.
        if let Some(chat) = state.chat_mut() {
            chat.messages.push(Message {
                id: store::new_id(),
                role: "You".to_string(),
                body: text.clone(),
                attachments: kept.clone(),
                when: store::now_secs(),
                ..Default::default()
            });
            if chat.meta.title == "New chat" {
                // A message of files and no words takes its name from the
                // first file, so the chat is not left nameless.
                let naming = if text.trim().is_empty() {
                    kept.first().map(|a| a.name.clone()).unwrap_or_default()
                } else {
                    text.clone()
                };
                chat.meta.title = Chat::title_from_prompt(&naming);
            }
            chat.meta.updated = store::now_secs();
        }
        if lost > 0 {
            let text = format!(
                "{} of the attachments were too big, or the disk would not have them, so they did not go",
                lost
            );
            state.push_note(&text, true);
        }
        // A picture the model cannot be shown is said out loud, so the reader
        // does not wonder why the model looks blind at it.
        let pictures = kept.iter().filter(|a| a.image).count();
        if pictures > 0 && !state.pictures_can_go() {
            state.push_note(
                "the pictures of this message cannot be shown to this model: turn vision on in the settings of the model, and send the message again",
                true,
            );
        }
        state.persist();
        state.generating = true;
        state.busy = true;
        // Both live buffers start empty, so a new turn never shows the
        // words of an older one, not even from another chat.
        state.live_text = String::new();
        state.live_reasoning = String::new();
        state.live_tools = Vec::new();
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
    // The two rooms of the Chatting settings, read once for this answer. None
    // means no ceiling, which is how both stand for a new set of settings: the
    // answer then ends when the model stops by itself, and a model may run as
    // many commands as it wants. A mark of a chapter eats neither room.
    let (command_room, answer_room) = {
        let Ok(guard) = state.lock() else { return };
        (
            config::engine_steps(guard.cfg.chat_max_tool_steps).map(|n| n.max(LEAST_TOOL_STEPS)),
            config::engine_steps(guard.cfg.chat_max_answer_steps)
                .map(|n| n.max(LEAST_ANSWER_STEPS)),
        )
    };
    let mut steps = 0usize;
    // How many commands have run and how many chapters have been marked in this
    // answer. The two rooms are kept apart on purpose: a mark runs nothing, and
    // a model that marks every part of a long talk needs far more marks than it
    // needs commands.
    let mut commands = 0usize;
    let mut marks = 0usize;
    loop {
        steps += 1;
        let (history, code_on, chapters_on) = {
            let Ok(guard) = state.lock() else { return };
            (
                guard.build_history(),
                guard.chat().map(|c| c.meta.code_enabled).unwrap_or(false),
                guard.cfg.chat_chapters,
            )
        };
        // Either tool is enough to let the model ask for one.
        let use_tools = code_on || chapters_on;
        let mut body = json!({
            "messages": history,
            "stream": true,
            // The usage numbers drive the context meter.
            "stream_options": { "include_usage": true },
            // The server says how far it is with the words it was sent, so the
            // gauge above the chat box has numbers to show before the model has
            // a word to give back.
            "return_progress": true,
            // And the server says on every piece how many words the talk takes,
            // so the meter of the context moves while the model writes, and not
            // only when it has stopped.
            "timings_per_token": true,
        });
        if use_tools {
            let mut offered: Vec<Value> = Vec::new();
            if code_on {
                offered.push(tools::definition());
            }
            if chapters_on {
                offered.push(tools::chapter_definition());
            }
            body["tools"] = Value::Array(offered);
            body["tool_choice"] = json!("auto");
        }

        // Every piece of new text goes into the state, so the window can show
        // it while it arrives.
        let stream_state = state.clone();
        let mut text_update = Instant::now();
        let mut reason_update = Instant::now();
        // The thinking of this step is measured, so that the head of the panel
        // can say how long the model thought for.
        if let Ok(mut guard) = state.lock() {
            guard.thought_start = None;
            guard.thought_end = None;
            guard.live_thought_ms = 0;
            guard.live_thought_done = false;
            // The gauge stands for this step alone, and for every step of an
            // answer. The words under the number follow the pictures the
            // message carries; the server has none of them, so the count is
            // read from the words that go out.
            guard.progress = 0;
            guard.progress_images = pending_images(&history);
            guard.progress_on = true;
            // The calls of the step before this one stand in the chat by now, so
            // the blocks of the words being written start anew.
            guard.live_tools = Vec::new();
        }
        let progress_state = state.clone();
        let context_state = state.clone();
        let tool_state = state.clone();
        let answer = llama::stream_chat(
            &base_url,
            &body,
            cancel,
            &mut |piece: &str| {
                if let Ok(mut guard) = stream_state.lock() {
                    guard.live_text.push_str(piece);
                    // The first word is here, so the words that were sent are
                    // worked through and the gauge has nothing left to say.
                    if finish_progress(&mut guard) {
                        guard.touch();
                    }
                    // Words have come, so the thinking of this moment is over.
                    // It may begin again before the answer ends.
                    if guard.thought_start.is_some() {
                        guard.live_thought_done = true;
                    }
                    if text_update.elapsed() > Duration::from_millis(60) {
                        text_update = Instant::now();
                        guard.touch();
                    }
                }
            },
            &mut |piece: &str| {
                if let Ok(mut guard) = stream_state.lock() {
                    let now = Instant::now();
                    if guard.thought_start.is_none() {
                        guard.thought_start = Some(now);
                    }
                    guard.thought_end = Some(now);
                    if let Some(begun) = guard.thought_start {
                        guard.live_thought_ms = now.duration_since(begun).as_millis() as u64;
                    }
                    guard.live_thought_done = false;
                    guard.live_reasoning.push_str(piece);
                    // A thought is a word of the answer as well: the words that
                    // were sent are worked through by now.
                    if finish_progress(&mut guard) {
                        guard.touch();
                    }
                    if reason_update.elapsed() > Duration::from_millis(60) {
                        reason_update = Instant::now();
                        guard.touch();
                    }
                }
            },
            &mut |made: llama::Progress| {
                // The server speaks of the prompt work in whole numbers, and a
                // batch of words may stand between two of its answers. The
                // gauge is written again only when the number it shows moved.
                if let Ok(mut guard) = progress_state.lock() {
                    let percent = (made.part() * 100.0).round() as u32;
                    if percent != guard.progress {
                        guard.progress = percent;
                        guard.touch();
                    }
                }
            },
            &mut |made: llama::Context| {
                // The room of the talk moves with every piece of the answer, so
                // the meter in the top bar moves with it. The number is put in
                // only when it grew, so a piece that says nothing new costs the
                // window nothing.
                if let Ok(mut guard) = context_state.lock() {
                    if climb_context(&mut guard, made.used()) {
                        guard.touch();
                    }
                }
            },
            &mut |made: &[ToolCall]| {
                // The reader sees the model write a command: the words of it go
                // into the state as they come, and not when the call is over.
                // The same piece takes the gauge away, so a model that asks for
                // a command before it has said anything does not leave it
                // standing over the chat for the whole of the run.
                if let Ok(mut guard) = tool_state.lock() {
                    if write_calls(&mut guard, made) {
                        guard.touch();
                    }
                }
            },
        );

        let Ok(mut guard) = state.lock() else { return };
        // A step that ended without a word leaves no gauge standing behind.
        guard.progress_on = false;
        let live = std::mem::take(&mut guard.live_text);
        let live_thinking = std::mem::take(&mut guard.live_reasoning);
        // The call is over, so the block of the words being written goes. The
        // run of the command stands in the chat in its place, and says what
        // became of it.
        guard.live_tools.clear();
        // How long the model spent thinking in this step. The words of the
        // panel are made from this.
        let thought_ms = match (guard.thought_start, guard.thought_end) {
            (Some(begun), Some(end)) => end.duration_since(begun).as_millis() as u64,
            _ => 0,
        };
        guard.thought_start = None;
        guard.thought_end = None;
        guard.live_thought_ms = 0;
        guard.live_thought_done = false;
        // Both hold the answer. The longer one is the safer one.
        let answer_text = if answer.text.chars().count() >= live.chars().count() {
            answer.text.clone()
        } else {
            live
        };
        // The thinking is kept the same way. Both live buffers are empty from
        // here on, so a stored step is not shown again while the next one is
        // still coming in.
        let answer_thinking = if answer
            .reasoning
            .chars()
            .count()
            >= live_thinking.chars().count()
        {
            answer.reasoning.clone()
        } else {
            live_thinking
        };

        if let Some(problem) = &answer.error {
            let text = problem.clone();
            guard.push_note(&text, true);
        }
        // The calls this step made, split into marks and commands. Only the
        // commands eat the room for commands. A mark is an action of its own
        // kind: the model is told that it was taken, and it goes on with the
        // next part of the talk. A turn that thinks and then asks for a command
        // has words enough, so nothing is written about a missing answer, and
        // its thinking goes to the command that stands for it.
        let step_calls: Vec<ToolCall> = answer
            .tool_calls
            .iter()
            .filter(|call| !call.name.is_empty())
            .cloned()
            .collect();
        let (step_marks, step_runs) = if use_tools {
            split_calls(step_calls)
        } else {
            (Vec::new(), Vec::new())
        };
        let asked_for_tool =
            use_tools && (!step_runs.is_empty() || !step_marks.is_empty());
        let no_words = answer_text.trim().is_empty();
        let turn_thinking = if no_words && asked_for_tool {
            answer_thinking.as_str()
        } else {
            ""
        };
        match kept_of(
            &answer_text,
            &answer_thinking,
            answer.stopped,
            asked_for_tool,
            answer.error.is_some(),
        ) {
            // The words are kept as they came in, also when the reader
            // stopped the run.
            Kept::Words => {
                guard.push_message("Assistant", &answer_text, &answer_thinking, thought_ms, false)
            }
            Kept::Thinking => {
                guard.push_message("Assistant", "", &answer_thinking, thought_ms, false)
            }
            // The model used tokens but left nothing to read, and asked for no
            // command. This happens when the context ran full.
            Kept::Note => {
                let note = if answer.finish == "length" {
                    String::from("(the context ran full before the model could answer)")
                } else {
                    String::from("(the model gave no answer)")
                };
                guard.push_message("Assistant", &note, &answer_thinking, thought_ms, false);
            }
            Kept::Nothing => {}
        }

        // Tokens for the context meter. The number the server gives at the end
        // is the number that stands, and it is kept over the live number of the
        // answer, for the answer is over and the server knows best.
        let used = answer.prompt_tokens + answer.completion_tokens;
        guard.ctx_used = if used > 0 {
            used
        } else {
            // Nothing came back, for the answer was stopped or the server could
            // not be reached. The meter already holds the words the server said
            // it had worked through, so they are kept over the guess.
            guard.ctx_used.max(estimate_tokens(&history))
        };
        let used_now = guard.ctx_used;
        if let Some(chat) = guard.chat_mut() {
            chat.meta.context_used = used_now;
            chat.meta.updated = store::now_secs();
        }
        guard.persist();

        // What is left to do after this step: the marks, and the commands the
        // room still holds. A step that only marked a chapter is not a tool run
        // in the old sense, so the answer goes on to the next part of the talk.
        let finished = answer.stopped || answer.error.is_some();
        let nothing_left = step_marks.is_empty() && step_runs.is_empty();
        // A ceiling of nothing is the same as no ceiling at all, so an answer
        // with an empty room for commands still ends on the turn that asked.
        let no_room_left = answer_room.is_some_and(|room| steps >= room);
        if finished || !use_tools || nothing_left || no_room_left {
            guard.generating = false;
            let text = if answer.stopped {
                "stopped"
            } else if !finished && !nothing_left {
                if no_room_left {
                    "the answer stopped at the room for steps of the Chatting settings"
                } else {
                    "the model asked for more than one answer could hold"
                }
            } else {
                "ready"
            };
            guard.set_status(text);
            // A mark costs nothing, so the marks of a step that ran clean are
            // taken even when the answer ends on the ceiling of the loop. A
            // stopped run or a failed one keeps nothing of a step that never
            // came to an end.
            drop(guard);
            if !finished {
                take_marks(state, &step_marks, &mut marks);
            }
            break;
        }
        // Keep the lock free while a mark is made and while a command runs.
        drop(guard);

        // The model wants a mark, a command, or both. The thinking of a turn
        // with no words of its own stands with the first thing the reader is
        // shown. A turn that only marked chapters shows nothing, so its
        // thoughts are kept on their own rather than lost with a mark nobody
        // sees.
        let mut thoughts = turn_thinking.to_string();
        // The marks first: they cost nothing, and the reader sees the parts of
        // the talk in the order the model asked for them.
        take_marks(state, &step_marks, &mut marks);
        for call in step_runs.iter() {
            // The room for commands is spent. The call is still answered, so
            // the model hears why and can go on with its talk instead of
            // asking again for a command that never runs.
            if command_room.is_some_and(|room| commands >= room) {
                let Ok(mut guard) = state.lock() else {
                    continue;
                };
                let note = "the room for commands in this answer is spent, so the command did not run";
                let id = guard.begin_tool(call, note, "", 0);
                guard.finish_tool(&id, note, false);
                continue;
            }
            commands += 1;
            let (command, purpose) = tools::parse_arguments(&call.arguments);
            let shown = if purpose.trim().is_empty() {
                command.clone()
            } else {
                format!("{}\nwhy: {}", command, purpose)
            };
            // The first command the reader sees takes the thoughts of the turn,
            // and no other one does.
            let thinking = std::mem::take(&mut thoughts);
            let message_id = {
                let Ok(mut guard) = state.lock() else {
                    continue;
                };
                let id = guard.begin_tool(call, &shown, &thinking, thought_ms);
                let text = format!("running a command: {}", first_line(&command));
                guard.set_status(&text);
                id
            };
            let outcome = tools::run(&command, &workdir, sandbox_on, TOOL_TIMEOUT);
            let mut output = outcome.summary();
            // The chat asked for the sandbox, so say when it was not used. The
            // run itself already says why, and it did not run at all then.
            if sandbox_on && !outcome.sandboxed {
                output = format!(
                    "{}\n(the command did not run, for the sandbox was not to be had)",
                    output
                );
            }
            if !sandbox_on {
                output = format!(
                    "{}\n(not sandboxed: this command could reach the whole machine)",
                    output
                );
            }
            let Ok(mut guard) = state.lock() else {
                continue;
            };
            guard.finish_tool(&message_id, &output, outcome.ok);
        }
        if !thoughts.is_empty() {
            // The turn asked for marks and for nothing a reader could look at,
            // so the thoughts stand above the marks as a panel of their own.
            let Ok(mut guard) = state.lock() else { return };
            guard.push_message("Assistant", "", &thoughts, thought_ms, false);
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
    let (binary, gpus, tools_wanted, thinking_mode, preserve) = {
        let Ok(state) = state.lock() else {
            return Err("the state is gone".to_string());
        };
        let binary = state
            .server_binary()
            .ok_or("llama-server was not found in the folder from the settings")?;
        // Either tool is enough to ask the server for tool support: the chapter
        // is wanted even in a chat that never runs a command.
        let tools_wanted = state
            .chat()
            .map(|c| c.meta.code_enabled)
            .unwrap_or(false)
            || state.cfg.chat_chapters;
        (
            binary,
            state.gpus.clone(),
            tools_wanted,
            state
                .chat()
                .map(|c| c.meta.thinking_mode.clone())
                .unwrap_or_else(|| String::from("off")),
            state.thinking.has_preserve.then(|| state.preserve_of_open()),
        )
    };
    // A server for the same model, thinking mode, choice of keeping the
    // thinking, and tool support is kept, so the model loads only once. Each of
    // the three needs flags of its own, and so does a tool that came or went
    // since the model was loaded.
    let reusable = server.as_mut().map(|s| s.alive()).unwrap_or(false)
        && server
            .as_ref()
            .map(|s| {
                s.model_id == model.id()
                    && s.thinking_mode == thinking_mode
                    && s.preserve_thinking == preserve
                    && s.tools_on == tools_wanted
            })
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
    match llama::Server::start(
        &binary,
        model,
        settings,
        &gpus,
        tools_wanted,
        &thinking_mode,
        preserve,
    ) {
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

// Rough token count, used when the server gives no usage numbers. The
// content of a message is words, or a list of parts with the words of the
// reader, the words of the files, and the pictures of the message among
// them. A picture costs a block of tokens of its own, whatever it shows.
fn estimate_tokens(history: &Value) -> u64 {
    let mut chars = 0u64;
    if let Some(items) = history.as_array() {
        for item in items {
            match item.get("content") {
                Some(Value::String(text)) => chars += text.chars().count() as u64,
                Some(Value::Array(parts)) => {
                    for part in parts {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            chars += text.chars().count() as u64;
                        }
                        if part.get("type").and_then(|t| t.as_str()) == Some("image_url") {
                            // A picture stands for a few hundred tokens.
                            chars += 4000;
                        }
                    }
                }
                _ => {}
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
    // share one data folder. They take turns through the lock of the config,
    // and each one empties the chats folder first.

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
        let message = Message {
            role: String::from("You"),
            body: String::from("A written question"),
            when: updated,
            ..Default::default()
        };
        chat.messages.push(message);
        store.save_chat(&chat);
    }

    // A blank chat lives only in the memory, so it never reaches the disk. A
    // chat that was written in is stored at once.
    #[test]
    fn blank_chats_stay_off_disk() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let chats = config::paths().chats;
        let stored = || {
            std::fs::read_dir(&chats)
                .map(|entries| entries.count())
                .unwrap_or(0)
        };

        // A run starts with no chat at all, and nothing on disk.
        assert_eq!(engine.snapshot().chat_count, 0);
        assert!(engine.snapshot().home);
        assert_eq!(stored(), 0);

        // Even the badges may not write a blank chat to disk.
        engine.new_chat();
        engine.set_code(true);
        engine.set_sandbox(false);
        assert_eq!(stored(), 0);

        // The first message makes it a real chat.
        {
            let mut state = engine.state.lock().unwrap();
            state.push_message("You", "A real question", "", 0, false);
        }
        assert_eq!(stored(), 1);

        // A fresh blank chat goes away when it is turned away from, and the
        // chat that was written in stays on the disk and in the carousel.
        engine.new_chat();
        assert_eq!(engine.snapshot().chat_count, 2);
        assert!(engine.snapshot().blank_chat);
        engine.roll(1);
        std::thread::sleep(Duration::from_millis(500));
        let after = engine.snapshot();
        assert_eq!(after.chat_count, 1);
        assert!(!after.blank_chat);
        assert_eq!(stored(), 1);
    }

    #[test]
    fn carousel_rules() {
        let _guard = config::test_turn();
        setup();
        write_chat("aaaabbbb01", 100);
        write_chat("aaaabbbb02", 200);

        // The first thing the user sees is the chat box on its own, with no
        // chat open. The chats on disk wait in the previews.
        let engine = Engine::start(Config::default());
        let snap = engine.snapshot();
        assert_eq!(snap.chat_count, 2);
        assert!(snap.home);
        assert!(!snap.generating);

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
            state.push_message("You", "Hello there", "", 0, false);
        }
        engine.roll(1);
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(engine.snapshot().chat_count, 3);

        // Every repository has its own carousel. A new repository is empty, so
        // the window shows the chat box on its own again.
        engine.create_repository(String::from("Test Repo"));
        let repo = engine.snapshot();
        assert!(repo.in_repository);
        assert_eq!(repo.chat_count, 0);
        assert!(repo.home);
        engine.select_repository(String::new());
        let back = engine.snapshot();
        assert!(!back.in_repository);
        assert_eq!(back.chat_count, 3);
    }

    // The first screen holds no chat. A message opens one, and the previews are
    // a window over the carousel that does not change which chat is open.
    #[test]
    fn the_first_screen_and_the_previews() {
        let _guard = config::test_turn();
        setup();
        write_chat("aaaabbbb03", 300);
        let engine = Engine::start(Config::default());

        // The chat on disk is there, but no chat is open.
        let snap = engine.snapshot();
        assert_eq!(snap.chat_count, 1);
        assert!(snap.home);
        assert!(!snap.overview);

        // The previews show that one chat, and it becomes the chat in use.
        engine.set_overview(true);
        let previews = engine.snapshot();
        assert!(previews.overview);
        assert!(!previews.home);
        assert_eq!(previews.chat_index, 0);

        // Picking it closes the previews and keeps the chat.
        engine.open_chat(String::from("aaaabbbb03"));
        let picked = engine.snapshot();
        assert!(!picked.overview);
        assert_eq!(picked.chat_id, "aaaabbbb03");

        // A message from the first screen opens a chat of this place. No model
        // is picked here, so the message stops before the worker, but the chat
        // is already there.
        engine.select_repository(String::new());
        engine.create_repository(String::from("Second"));
        assert!(engine.snapshot().home);
        engine.prompt(String::from("A first question"), Vec::new());
        let after = engine.snapshot();
        assert!(!after.home);
        assert_eq!(after.chat_count, 1);
        assert!(after.chats[0].transient);
    }

    // The badge offers the modes of the model and the reader chooses one. A
    // mode the model does not offer is refused, and a model with no thinking
    // mode does not change.
    #[test]
    fn the_thinking_mode_is_chosen() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        engine.new_chat();
        {
            let mut state = engine.state.lock().unwrap();
            state.thinking = markdown::ThinkingOptions {
                has_thinking: true,
                has_effort: false,
                efforts: Vec::new(),
                has_preserve: false,
            };
        }
        // The switch model offers off and on, and a new chat starts with the
        // thinking off.
        assert_eq!(engine.snapshot().thinking_mode, "off");
        engine.set_thinking("on");
        assert_eq!(engine.snapshot().thinking_mode, "on");
        // A level is no use to a model with only a switch.
        engine.set_thinking("xhigh");
        assert_eq!(engine.snapshot().thinking_mode, "on");
        engine.set_thinking("off");
        assert_eq!(engine.snapshot().thinking_mode, "off");
        // The effort model offers the levels its template names.
        {
            let mut state = engine.state.lock().unwrap();
            state.thinking = markdown::ThinkingOptions {
                has_thinking: false,
                has_effort: true,
                efforts: vec![String::from("low"), String::from("high")],
                has_preserve: false,
            };
        }
        engine.set_thinking("high");
        assert_eq!(engine.snapshot().thinking_mode, "high");
        engine.set_thinking("medium");
        assert_eq!(engine.snapshot().thinking_mode, "high");
        // A model with no thinking mode keeps the mode it had.
        {
            let mut state = engine.state.lock().unwrap();
            state.thinking = markdown::ThinkingOptions::default();
        }
        engine.set_thinking("off");
        assert_eq!(engine.snapshot().thinking_mode, "high");
    }

    // The choice to keep the thinking of the turns before the last answer
    // stands with the chat, and only a model whose template offers it may
    // change it.
    #[test]
    fn the_thinking_of_the_earlier_turns_may_be_kept() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        engine.new_chat();
        {
            let mut state = engine.state.lock().unwrap();
            state.thinking = markdown::ThinkingOptions {
                has_thinking: true,
                has_effort: false,
                efforts: Vec::new(),
                has_preserve: true,
            };
        }
        // llama.cpp keeps the thinking on its own, so a new chat starts kept.
        assert!(engine.snapshot().thinking_preserve_available);
        assert!(engine.snapshot().thinking_preserve);
        engine.set_preserve(false);
        assert!(!engine.snapshot().thinking_preserve);
        engine.set_preserve(true);
        assert!(engine.snapshot().thinking_preserve);
        // A model whose template cannot keep its thinking does not change, and
        // says so.
        {
            let mut state = engine.state.lock().unwrap();
            state.thinking = markdown::ThinkingOptions::default();
        }
        engine.set_preserve(false);
        assert!(engine.snapshot().thinking_preserve);
        assert!(engine
            .snapshot()
            .status
            .contains("no switch for keeping the thinking"));
    }

    // The choice to keep the thinking is not only a flag of the server. The
    // template of a model reads the thoughts of an answer out of the words it
    // is sent, so the thoughts have to travel with the answer they came from -
    // otherwise there is nothing there to keep, and the model is left with no
    // memory of its own reasoning. A chat that drops them, and a model whose
    // template cannot keep them, send no thoughts at all.
    #[test]
    fn the_kept_thinking_travels_with_the_answers() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        {
            let mut state = engine.state.lock().unwrap();
            state.thinking = markdown::ThinkingOptions {
                has_thinking: true,
                has_effort: false,
                efforts: Vec::new(),
                has_preserve: true,
            };
            state.open_blank_chat();
            if let Some(chat) = state.chat_mut() {
                chat.messages.push(Message {
                    role: String::from("You"),
                    body: String::from("what is two plus two?"),
                    ..Default::default()
                });
                chat.messages.push(Message {
                    role: String::from("Assistant"),
                    body: String::from("four"),
                    reasoning: String::from("two and two make four"),
                    ..Default::default()
                });
                // A turn that thought and then asked for a command leaves its
                // thinking with the command, so it must travel the same road.
                chat.messages.push(Message {
                    role: String::from("Tool"),
                    body: String::from("ls"),
                    reasoning: String::from("let me look first"),
                    tool: String::from("one file"),
                    call_id: String::from("call-one"),
                    tool_name: String::from(tools::TOOL_NAME),
                    tool_args: String::from("{\"command\":\"ls\"}"),
                    ..Default::default()
                });
            }
            let said = state.build_history();
            let list = said.as_array().expect("the history is a list");
            assert_eq!(list.len(), 4);
            // The words of the reader carry no thoughts of their own.
            assert!(list[0].get("reasoning_content").is_none());
            assert_eq!(
                list[1]["reasoning_content"].as_str().unwrap(),
                "two and two make four"
            );
            assert_eq!(
                list[2]["reasoning_content"].as_str().unwrap(),
                "let me look first"
            );
            assert!(list[3].get("reasoning_content").is_none());
        }
        // A chat that drops the thinking sends the answers bare.
        engine.set_preserve(false);
        {
            let state = engine.state.lock().unwrap();
            let said = state.build_history();
            let list = said.as_array().expect("the history is a list");
            assert!(list.iter().all(|m| m.get("reasoning_content").is_none()));
        }
        // A model whose template cannot keep its thinking is sent none either,
        // whatever the chat chooses.
        {
            let mut state = engine.state.lock().unwrap();
            state.thinking = markdown::ThinkingOptions::default();
            state.new_preserve = true;
            if let Some(chat) = state.chat_mut() {
                chat.meta.preserve_thinking = true;
            }
            let said = state.build_history();
            let list = said.as_array().expect("the history is a list");
            assert!(list.iter().all(|m| m.get("reasoning_content").is_none()));
        }
    }

    // A chapter stands where the model asked for it. The marks are given to the
    // window in the order they were made, each tied to the message the chapter
    // begins after, and a mark with no name is not a chapter.
    #[test]
    fn the_chapters_stand_where_they_were_marked() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        engine.new_chat();
        let mark = |name: &str| ToolCall {
            id: format!("call-{name}"),
            name: String::from(tools::CHAPTER_TOOL_NAME),
            arguments: format!(r#"{{"name":"{name}"}}"#),
        };
        let (first, second, spoken) = {
            let mut state = engine.state.lock().unwrap();
            state.push_message("You", "Fix the parser", "", 0, false);
            let first = state.chat().unwrap().messages.last().unwrap().id.clone();
            state.add_chapter(&mark("Fixing the parser"), "Fixing the parser");
            state.push_message("Assistant", "The parser is fixed.", "", 0, false);
            let second = state.chat().unwrap().messages.last().unwrap().id.clone();
            // A mark with no name, and a mark of a chapter of its own. The name
            // of the second mark is longer than the tool allows, and comes back
            // cut.
            state.add_chapter(&mark(""), "");
            let sixty = "n".repeat(62);
            let long = tools::chapter_name(&mark(&sixty).arguments);
            assert_eq!(long.len(), tools::MAX_CHAPTER_NAME);
            state.add_chapter(&mark(&long), &long);
            // Everything the chat holds, the marks of the chapters and all.
            let spoken = state.chat().unwrap().messages.clone();
            (first, second, spoken)
        };
        let chapters = engine.snapshot().chapters;
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].name, "Fixing the parser");
        assert_eq!(chapters[0].after, first);
        // The unnamed mark is skipped, and the chapter after it still begins
        // after the same message.
        assert_eq!(chapters[1].name, "n".repeat(50));
        assert_eq!(chapters[1].after, second);
        // The marks are not words of the talk: the window is given the chapter
        // on the side, and the message list without them.
        let visible = engine.snapshot().messages;
        assert!(!visible.iter().any(|m| m.role == "Chapter"));
        assert_eq!(visible.len(), 2);
        // The soft picture of the chat in the corner is the newest thing that
        // was really said, and never the name of a chapter.
        let preview = store::preview(&spoken);
        assert!(!preview.contains(&"n".repeat(50)));
        assert_eq!(preview, "The parser is fixed.");
    }

    // A mark of a chapter is not a command, so it is split away from the
    // commands of a step and eats none of the room a command has.
    #[test]
    fn a_mark_of_a_chapter_costs_no_command_room() {
        let call = |name: &str, id: &str| ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: String::from("{}"),
        };
        let step = vec![
            call(tools::CHAPTER_TOOL_NAME, "one"),
            call(tools::TOOL_NAME, "two"),
            call(tools::CHAPTER_TOOL_NAME, "three"),
        ];
        let (marks, runs) = split_calls(step);
        assert_eq!(marks.len(), 2);
        assert_eq!(runs.len(), 1);
        assert_eq!(marks[0].id, "one");
        assert_eq!(marks[1].id, "three");
        assert_eq!(runs[0].id, "two");
        // A step of nothing but marks holds no command at all, so an answer can
        // mark chapter after chapter without touching the room for commands.
        let (only, runs) = split_calls(vec![call(tools::CHAPTER_TOOL_NAME, "one")]);
        assert_eq!(only.len(), 1);
        assert!(runs.is_empty());
    }

    // A turn writes no line about itself. A command is an answer, a stop is
    // not a sentence, and a failure is already told by its own note.
    #[test]
    fn a_turn_writes_no_line_about_itself() {
        // Words stand for themselves, also the words of a stopped run.
        assert_eq!(
            kept_of("Here is the plan", "", false, false, false),
            Kept::Words
        );
        assert_eq!(
            kept_of("Half way to the plan", "", true, false, false),
            Kept::Words
        );
        // The model thought and then asked for a command. The command is the
        // answer, so no line is written about a missing one.
        assert_eq!(
            kept_of("", "I will run a command.", false, true, false),
            Kept::Nothing
        );
        // A stopped turn keeps the thinking it had, and nothing else.
        assert_eq!(
            kept_of("", "Half way through the thinking", true, false, false),
            Kept::Thinking
        );
        assert_eq!(kept_of("", "", true, false, false), Kept::Nothing);
        // A failed run is told by the note of the failure alone.
        assert_eq!(kept_of("", "", false, false, true), Kept::Nothing);
        // Nothing came in, and nothing failed. One quiet line is right.
        assert_eq!(kept_of("", "", false, false, false), Kept::Note);
    }

    // The thinking of a turn with no words of its own stands with the command
    // it led to, so it is not lost when the line about the answer is left out.
    #[test]
    fn a_tool_run_keeps_the_thinking_of_its_turn() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        engine.new_chat();
        let call = ToolCall {
            id: String::from("call-one"),
            name: String::from(tools::TOOL_NAME),
            arguments: String::from("{\"command\":\"python -c 'print(5)'\"}"),
        };
        {
            let mut state = engine.state.lock().unwrap();
            let id =
                state.begin_tool(&call, "python -c 'print(5)'", "I will run the command.", 0);
            state.finish_tool(&id, "5", true);
        }
        let snap = engine.snapshot();
        let last = snap.messages.last().expect("the chat holds the run");
        assert_eq!(last.role, "Tool");
        assert_eq!(last.reasoning, "I will run the command.");
        assert_eq!(last.tool, "5");
    }

    // The words under the number of the gauge follow the pictures the message
    // carries. A chat of words gives none, which is how the program stands
    // today, and only the newest message of the user is looked at.
    #[test]
    fn the_pictures_of_a_message_are_counted() {
        let words = json!([{ "role": "user", "content": "one question" }]);
        assert_eq!(pending_images(&words), 0);

        let one = json!([{
            "role": "user",
            "content": [
                { "type": "text", "text": "what is this?" },
                { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAA" } },
            ],
        }]);
        assert_eq!(pending_images(&one), 1);

        let two = json!([{
            "role": "user",
            "content": [
                { "type": "image_url", "image_url": { "url": "AAA" } },
                { "type": "image_url", "image_url": { "url": "BBB" } },
            ],
        }]);
        assert_eq!(pending_images(&two), 2);

        // Only the last thing that goes out is worked through anew. A turn that
        // ends with the words of the model carries no picture of its own.
        let older = json!([
            { "role": "user", "content": [{ "type": "image_url", "image_url": { "url": "AAA" } }] },
            { "role": "assistant", "content": "a cat on a mat" },
        ]);
        assert_eq!(pending_images(&older), 0);

        // A chat with nothing in it is still a chat of words.
        assert_eq!(pending_images(&json!([])), 0);
    }

    // A message that hangs attachments goes out to the model as parts: the
    // words of the reader, the list of the files with their numbers and
    // their endings, and the words of a file of words. Without a model that
    // can look at pictures, no picture part goes out, and the list says why.
    #[test]
    fn the_attachments_of_a_message_stand_in_the_history() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let files = vec![
            attach::Incoming {
                name: String::from("photo.png"),
                mime: String::from("image/png"),
                data: String::from("data:image/png;base64,AAA="),
                text: String::new(),
            },
            attach::Incoming {
                name: String::from("notes.txt"),
                mime: String::from("text/plain"),
                data: String::new(),
                text: String::from("the words of the note"),
            },
        ];
        let mut state = engine.state.lock().unwrap();
        state.open_blank_chat();
        // The records of the attachments stand in a folder of the room of the
        // chat that carries them, so this test saves them where a chat of this
        // place would keep them.
        let shelf = {
            let chat = state.chat().expect("a blank chat stands open");
            state.attach_place_for(chat)
        };
        let kept = attach::save_at(&shelf, &files);
        assert_eq!(kept.len(), 2);
        if let Some(chat) = state.chat_mut() {
            chat.messages.push(Message {
                id: store::new_id(),
                role: String::from("You"),
                body: String::from("What is in these?"),
                attachments: kept.clone(),
                ..Default::default()
            });
        }
        let history = state.build_history();
        let items = history.as_array().expect("the history is a list");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["role"], json!("user"));
        let parts = items[0]["content"]
            .as_array()
            .expect("a message with attachments goes out as parts");
        let lead = parts[0].get("text").unwrap().as_str().unwrap();
        assert!(lead.starts_with("What is in these?"));
        assert!(lead.contains("1. photo.png"));
        assert!(lead.contains("2. notes.txt"));
        // No model stands in this test, so no picture can go, and the list
        // says that plainly.
        assert!(lead.contains("vision is off"));
        assert!(!parts
            .iter()
            .any(|p| p.get("type").and_then(|t| t.as_str()) == Some("image_url")));
        // The file of words goes out under its number and its name.
        let words = parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .find(|t| t.starts_with("The words of attachment 2"))
            .expect("the words of the second attachment");
        assert!(words.contains("notes.txt"));
        assert!(words.contains("the words of the note"));
        // The snapshot hands the blocks to the window: name and kind, in the
        // order the reader hung them, and which of them are pictures.
        let snap = state.snap();
        let message = snap.messages.iter().find(|m| m.role == "You").unwrap();
        assert_eq!(message.attachments.len(), 2);
        assert_eq!(message.attachments[0].name, "photo.png");
        assert!(message.attachments[0].image);
        assert!(!message.attachments[1].image);
        drop(state);
        attach::remove_at(
            &shelf,
            &kept.iter().map(|a| a.id.clone()).collect::<Vec<_>>(),
        );
    }

    // The reader watches a command as the model writes it, and watches nothing
    // of a mark of a chapter. The name of a call comes in pieces, so a name that
    // could still become the mark stands aside until it is known.
    #[test]
    fn only_a_command_the_reader_can_use_is_written_out() {
        let command = ToolCall {
            id: String::from("call-one"),
            name: String::from(tools::TOOL_NAME),
            arguments: String::from("{\"command\":\"ls -la\",\"purpose\":\"see what is here\"}"),
        };
        let made = watched(std::slice::from_ref(&command));
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].name, tools::TOOL_NAME);
        assert_eq!(made[0].command, "ls -la");
        assert_eq!(made[0].purpose, "see what is here");

        // A mark of a chapter runs nothing and is shown to nobody, so there is
        // nothing to watch while it is written.
        let mark = ToolCall {
            id: String::from("call-two"),
            name: String::from(tools::CHAPTER_TOOL_NAME),
            arguments: String::from("{\"name\":\"The plan\"}"),
        };
        assert!(watched(std::slice::from_ref(&mark)).is_empty());

        // Three letters of a name are not yet the name of a tool: the mark could
        // still be coming, so nothing stands in the chat for it yet.
        let half = ToolCall {
            id: String::from("call-three"),
            name: String::from("cha"),
            arguments: String::from("{\"na"),
        };
        assert!(watched(std::slice::from_ref(&half)).is_empty());

        // A call with no name of its own says nothing of what it is.
        let blank = ToolCall {
            id: String::from("call-four"),
            ..Default::default()
        };
        assert!(watched(std::slice::from_ref(&blank)).is_empty());

        // The words of a command that is not finished are kept as they came, so
        // the reader sees the command grow instead of waiting for the end.
        let grown = ToolCall {
            id: String::from("call-five"),
            name: String::from(tools::TOOL_NAME),
            arguments: String::from("{\"command\":\"cargo test --release mark"),
        };
        let made = watched(std::slice::from_ref(&grown));
        assert_eq!(made[0].command, "cargo test --release mark");
        assert!(made[0].purpose.is_empty());
    }

    // The calls being written stand in the snapshot, and go away with the step
    // that wrote them, so a block is never left standing over a chat.
    #[test]
    fn the_calls_being_written_stand_in_the_snapshot() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        engine.new_chat();
        {
            let mut state = engine.state.lock().unwrap();
            let call = ToolCall {
                id: String::from("call-one"),
                name: String::from(tools::TOOL_NAME),
                arguments: String::from("{\"command\":\"uname"),
            };
            state.live_tools = watched(std::slice::from_ref(&call));
        }
        let snap = engine.snapshot();
        assert_eq!(snap.live_tools.len(), 1);
        assert_eq!(snap.live_tools[0].command, "uname");
        {
            let mut state = engine.state.lock().unwrap();
            state.live_tools.clear();
        }
        assert!(engine.snapshot().live_tools.is_empty());
    }

    // The gauge above the chat box stood while the server worked the words it
    // was sent through. A model cannot write a command before that is over, so
    // the first word of a call takes the gauge away. A model that asks for a
    // command before it has said anything must not leave it standing over the
    // chat until the command has run.
    #[test]
    fn the_gauge_leaves_when_a_call_is_begun() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        engine.new_chat();
        let command = ToolCall {
            id: String::from("call-one"),
            name: String::from(tools::TOOL_NAME),
            arguments: String::from("{\"command\":\"uname"),
        };
        {
            let mut state = engine.state.lock().unwrap();
            state.progress_on = true;
            state.progress = 42;
            // The command is only begun, and the gauge goes already.
            assert!(write_calls(&mut state, std::slice::from_ref(&command)));
            assert!(!state.progress_on, "the gauge stood on after the call began");
            // A gauge that goes away has finished its work.
            assert_eq!(state.progress, 100);
            assert_eq!(state.live_tools.len(), 1);
            assert_eq!(state.live_tools[0].command, "uname");
        }
        assert!(!engine.snapshot().progress_on);
        // The mark of a chapter gives the reader no block, but it is written
        // after the words that were sent, so the gauge leaves for it as well.
        let mark = ToolCall {
            id: String::from("call-two"),
            name: String::from(tools::CHAPTER_TOOL_NAME),
            arguments: String::from("{\"name\":\"The plan\"}"),
        };
        {
            let mut state = engine.state.lock().unwrap();
            state.live_tools.clear();
            state.progress_on = true;
            state.progress = 10;
            assert!(write_calls(&mut state, std::slice::from_ref(&mark)));
            assert!(!state.progress_on, "the gauge stood on after a mark began");
            assert!(state.live_tools.is_empty());
        }
        // A piece that says nothing new of a call the state holds already says
        // nothing at all: the gauge went with the first word of it.
        {
            let mut state = engine.state.lock().unwrap();
            state.live_tools.clear();
            write_calls(&mut state, std::slice::from_ref(&command));
            assert!(!write_calls(&mut state, std::slice::from_ref(&command)));
        }
    }

    // The meter of the context moves while an answer comes in, and it only
    // climbs. The number the server gives at the end of the answer is kept for
    // the end, and that one may be smaller.
    #[test]
    fn the_context_meter_moves_while_an_answer_comes() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        engine.new_chat();
        {
            let mut state = engine.state.lock().unwrap();
            state.ctx_used = 0;
            // Nothing is known of the talk yet, so the meter keeps what it
            // stands on.
            assert!(!climb_context(&mut state, 0));
            assert_eq!(state.ctx_used, 0);
            // The words of the talk so far. The meter climbs.
            assert!(climb_context(&mut state, 431));
            // A piece that says the same, or less, moves nothing: the words of
            // an answer that is still coming are not taken back.
            assert!(!climb_context(&mut state, 431));
            assert!(!climb_context(&mut state, 400));
            assert_eq!(state.ctx_used, 431);
            assert!(climb_context(&mut state, 900));
            assert_eq!(state.ctx_used, 900);
        }
        // The window draws the number of the meter from the snapshot, so the
        // number of the live answer reaches the top bar.
        assert_eq!(engine.snapshot().context_used, 900);
    }
}