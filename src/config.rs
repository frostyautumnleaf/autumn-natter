// Program settings and the place where Autumn Natter keeps its data.
//
// The settings file is a small JSON file. It lives in the program data
// directory. The directory name is always autumn_natter.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

// Name of the data directory. Do not change it without a move step.
pub const APP_DIR_NAME: &str = "autumn_natter";

// Default port for the remote (browser) mode.
pub const DEFAULT_REMOTE_PORT: u16 = 8723;

fn default_remote_port() -> u16 {
    DEFAULT_REMOTE_PORT
}

// A switch of the Chatting section that stands on unless the user turns it off.
fn on_by_default() -> bool {
    true
}

// ------------------------------------------------------------ compaction
//
// A chat goes on for ever once the room of the talk is made again on the way.
// The compaction says how that is done: either nothing at all, or a handoff
// summary. The summary is written by the model itself when the talk nears the
// end of its room, and the words before it are then left out of what goes back
// to the model. Only the first message of the user and the newest summary go
// on, beside the words that came after the summary.

// No compaction. The chat is sent whole until the room is gone.
pub const COMPACTION_OFF: &str = "off";
// The handoff summary. This is what a new set of settings stands on.
pub const COMPACTION_HANDOFF: &str = "handoff";

// The summarisation is set going by a share of the room of the talk.
pub const COMPACT_BY_PERCENT: &str = "percent";
// The summarisation is set going by the least room that must be left over.
pub const COMPACT_BY_TOKENS: &str = "tokens";

// The most characters the handoff prompt may hold. The prompt is sent with the
// whole of the talk, so a prompt of a novel would cost room of its own.
pub const GREATEST_HANDOFF_PROMPT: usize = 4096;

// The prompt that is given to the model when it is asked for the handoff
// summary. The user may write another one in the settings.
pub const DEFAULT_HANDOFF_PROMPT: &str = "Write a detailed but concise handoff summary for AI consumption (no styling) that can be given to an AI to continue working from the current state of the session. Include the context of the session that an AI needs to carry on. Say what has been done so far and what needs to be done next. The AI will only receive the first initial prompt that the user sent, along with this handoff summary. Name the file paths that matter, one short line each, with what each file is used for. Keep the summary to about three hundred words: it must come to an end, for a summary that runs on for ever leaves the room it was asked to make unmade.";

// The second ask for a handoff summary. The first is given all the room that
// stands over after the words of the talk, and a model with a great deal to say
// can fill that room to the end and stop in the middle of a sentence. The
// second ask carries the same talk and a prompt that leans hard the other way,
// so the words come to an end inside the room that is left.
pub const DEFAULT_HANDOFF_PROMPT_SHORT: &str = "Write a very short handoff summary for AI consumption (no styling). The context window of the model is nearly full, so the whole summary must end within a few hundred words. Keep only what an AI needs to continue the work from this point: what is done, what is half done, and what to do next. Name the files that matter, one short line each. Do not repeat the conversation, do not review finished work, and do not add detail that is not needed to continue.";

// The share of the room of the talk at which the handoff summary is asked for.
fn default_compact_percent() -> u32 {
    85
}

// The least room that must be left over, in tokens. This is the number the
// second way of setting the summarisation going works from.
fn default_compact_reserve() -> u32 {
    10240
}

// The handoff prompt of a new set of settings.
fn default_handoff_prompt() -> String {
    String::from(DEFAULT_HANDOFF_PROMPT)
}

// The handoff prompt of the second ask, of a new set of settings.
fn default_handoff_prompt_short() -> String {
    String::from(DEFAULT_HANDOFF_PROMPT_SHORT)
}

// The room for steps of one answer, as the engine wants it. The setting is -1
// for no limit, so the answer is None. A number below -1 is read as no limit
// too, for a ceiling of minus five means nothing. The engine crate keeps this
// line, so the setting of the window and the ceiling of the loop cannot drift
// apart.
pub fn engine_steps(setting: i64) -> Option<usize> {
    if setting < 0 {
        return None;
    }
    Some(setting as usize)
}

// Model settings. One entry per model file. The model path is the key.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelSettings {
    pub ctx_size: u32,
    // Index inside the KV cache type list in llama.rs.
    pub kv_index: usize,
    // Index inside the speculative decoding list in llama.rs.
    pub spec_index: usize,
    pub draft_model: String,
    pub draft_tokens: u32,
    pub parallel: u32,
    pub batch_size: u32,
    pub ubatch_size: u32,
    pub tensor_parallel: bool,
    pub vision: bool,
    // Sampling of one answer. The defaults are the values of this program, and
    // they are sent to the server on every start. The values are f64, so the
    // settings file and the window show 0.7, not the full form of an f32.
    pub temperature: f64,
    pub top_p: f64,
    pub top_k: u32,
    pub min_p: f64,
    // 1.0 turns the penalty off.
    pub repeat_penalty: f64,
    // 0.0 turns the penalty off.
    pub presence_penalty: f64,
    // The most tokens one answer may give. -1 leaves no limit.
    pub max_tokens: i64,
    // The seed of the random draw. -1 picks a new one on every start.
    pub seed: i64,
    // The most context checkpoints the server keeps per slot. 0 turns them off.
    pub ctx_checkpoints: i64,
    // The ceiling of the talk memory in mebibytes. 0 turns the cache off, and
    // -1 lifts the limit.
    pub cache_ram: i64,
    // The server keeps the work of the prompt and reuses it on the next turn.
    pub cache_prompt: bool,
    // The room one image may take, for a model that can look at pictures.
    // 0 keeps the value inside the model file.
    pub image_min_tokens: i64,
    pub image_max_tokens: i64,
    // Stop the shift of the context when the talk overflows the room. Good to
    // use when coding, where a shift cuts the words the model works on.
    pub no_context_shift: bool,
    // The projector that reads the pictures works on the card, by default.
    // With the switch on, it works on the CPU.
    pub mmproj_cpu: bool,
    // -1 means no limit. 0 stops thinking at once.
    pub reasoning_budget: i64,
    pub reasoning_message: String,
    // Custom jinja chat template. Empty uses the model's own embedded template.
    #[serde(default)]
    pub chat_template: String,
    // Index inside the model load mode list in llama.rs.
    pub load_index: usize,
    pub extra_args: String,
    // VRAM budget in mebibytes. 0 means use every device fully.
    pub vram_budget: f64,
    // One flag per detected device, in detection order.
    pub gpu_enabled: Vec<bool>,
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            ctx_size: 8192,
            kv_index: 1,
            spec_index: 0,
            draft_model: String::new(),
            draft_tokens: 4,
            parallel: 1,
            batch_size: 512,
            ubatch_size: 128,
            tensor_parallel: false,
            vision: false,
            temperature: 0.7,
            top_p: 0.95,
            top_k: 20,
            min_p: 0.0,
            repeat_penalty: 1.0,
            presence_penalty: 0.0,
            max_tokens: -1,
            seed: -1,
            ctx_checkpoints: 32,
            cache_ram: 8192,
            cache_prompt: true,
            image_min_tokens: 0,
            image_max_tokens: 0,
            no_context_shift: false,
            mmproj_cpu: false,
            reasoning_budget: -1,
            reasoning_message: String::new(),
            chat_template: String::new(),
            // mmap keeps the model in the file cache.
            load_index: 1,
            extra_args: String::new(),
            vram_budget: 0.0,
            gpu_enabled: Vec::new(),
        }
    }
}

impl ModelSettings {
    // Merge a GPU list of a different length into the stored one.
    pub fn resize_gpus(&mut self, count: usize) {
        self.gpu_enabled.resize(count, true);
    }
}

// All program settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    // Directory that holds the llama-server binary.
    pub llama_dir: String,
    // Directory that holds the model files.
    pub models_dir: String,
    // Directory that holds the repositories. Empty means the default place.
    pub repositories_dir: String,
    pub dark_mode: bool,
    #[serde(default = "default_remote_port")]
    pub remote_port: u16,
    pub upnp_enabled: bool,
    // Model the user selected last.
    pub model_id: String,
    // Per model settings. The key is the full model path.
    pub model_settings: BTreeMap<String, ModelSettings>,
    // Chatting. How the chat of the window shows the parts of an answer.
    // Fold a thinking panel up while the thinking of the model comes in.
    #[serde(default = "on_by_default")]
    pub chat_collapse_thinking: bool,
    // Let a thinking panel grow with its words instead of stopping at a height
    // the user moves in. Off, every thinking panel keeps the fixed height it
    // always had.
    #[serde(default = "on_by_default")]
    pub chat_grow_thinking: bool,
    // Let a tool block that the model calls while thinking stand inside the
    // thinking panel of that turn, instead of under it.
    #[serde(default = "on_by_default")]
    pub chat_tools_in_thinking: bool,
    // Fold a tool panel up while its command runs.
    #[serde(default = "on_by_default")]
    pub chat_collapse_tools: bool,
    // Tell the model the day and the clock time of this machine once, in a
    // message of the role `system` at the start of the talk. The line is not
    // shown in the chat.
    #[serde(default = "on_by_default")]
    pub chat_date_new_chat: bool,
    // Give the words of a block of code the colours of their kind.
    #[serde(default = "on_by_default")]
    pub chat_highlight: bool,
    // Let the marks of boldness stand for boldness: a word held between two marks
    // is written in the heavier shape of the letters. Turned off, the marks are
    // read as part of the words of the answer, as they were before.
    #[serde(default = "on_by_default")]
    pub chat_bold_markdown: bool,
    // How many times an answer is asked over when the model came to its own end
    // without a word, a call or a fault. A model that thought a while and then
    // stopped has said nothing of the work it was given, and the same ask a second
    // time very nearly always brings the words back. 0 leaves a silence alone.
    #[serde(default = "silence_retries")]
    pub chat_silent_retries: i64,
    // Let the model mark the chapters of a chat, and show them beside it.
    #[serde(default = "on_by_default")]
    pub chat_chapters: bool,
    // Keep the talk of a chat that the model has worked its way through in a
    // file of the program, so the chat does not have to be read through again
    // when it comes back: after another chat took the model, or after the program
    // was shut down and started again. What stands in the file is what the model
    // understood of the talk, and not the talk itself -- that stands where it
    // always stood -- so the file is only ever a saving of the reading, and it
    // may be thrown away whenever the user likes.
    #[serde(default = "on_by_default")]
    pub chat_context_files: bool,
    // How many terminal commands one answer may run. -1 leaves no limit, so a
    // model may run a command as often as it wants inside one answer.
    #[serde(default = "no_limit")]
    pub chat_max_tool_steps: i64,
    // How many round trips one answer may take in all: a round trip is one turn
    // of the model, and one more follows every command it asks for. -1 leaves
    // no limit, so an answer ends only when the model stops by itself.
    #[serde(default = "no_limit")]
    pub chat_max_answer_steps: i64,
    // A paste of more than this many characters into the chat box becomes a
    // file attachment instead of words in the box. 100 is the least, so a
    // paste of a word or two always stays a paste.
    #[serde(default = "default_pasted_text_char_limit")]
    pub pasted_text_char_limit: u32,
    // How wide the chat may be, in pixels. The width the chat always had is the
    // least it can be set to. The page keeps the chat within 80 per cent of the
    // width of the window, and within the room that stands over after the line
    // of the chapters, the line of the summarisations and the arrows of the chat
    // have taken their room.
    #[serde(default = "default_chat_width")]
    pub chat_width: u32,
    // How long the model waits for an answer to a question it put to the user.
    // When the time is gone the user is asked no more, the model is told that
    // the user did not answer, and it is left to go on as well as it can.
    #[serde(default = "default_ask_seconds")]
    pub ask_seconds: u32,
    // The same for a question the model marked as one it cannot go on without.
    // Nothing at all, which is the default, waits until the user answers: a
    // model that was told to ask only when it must should not have its question
    // taken away for a user who stepped away from the machine.
    #[serde(default)]
    pub ask_critical_seconds: u32,
    // Compaction. Either "off", which leaves the chat alone until the room is
    // gone, or "handoff", which asks the model for a handoff summary before
    // the room is gone. A new set of settings stands on the handoff summary.
    #[serde(default = "default_compaction")]
    pub chat_compaction: String,
    // Which way the summarisation is set going: "percent" for a share of the
    // room of the talk, "tokens" for the least room that must be left over.
    #[serde(default = "default_compact_by")]
    pub chat_compact_by: String,
    // The share of the room of the talk at which the summary is asked for.
    #[serde(default = "default_compact_percent")]
    pub chat_compact_percent: u32,
    // The least room that must be left over, in tokens. When the room left is
    // this small or smaller, the summary is asked for.
    #[serde(default = "default_compact_reserve")]
    pub chat_compact_reserve: u32,
    // The prompt that is sent to the model when it is asked for the handoff
    // summary. No longer than GREATEST_HANDOFF_PROMPT characters.
    #[serde(default = "default_handoff_prompt")]
    pub chat_compact_prompt: String,
    // The prompt of the second ask: the one that is made when the first ran to
    // the end of the room. It leans towards a short summary, so the words come
    // to an end. No longer than GREATEST_HANDOFF_PROMPT characters.
    #[serde(default = "default_handoff_prompt_short")]
    pub chat_compact_prompt_short: String,
    // The vertical timeline of the summarisations, at the right of the chat.
    #[serde(default = "on_by_default")]
    pub chat_timeline: bool,    // The servers of the user's own, spoken to over MCP. They stand here and
    // not in a folder of their own, for the sheet writes the whole list again
    // whenever one of them moves. A set of settings written before this stood
    // reads with no server at all.
    #[serde(default)]
    pub mcp_servers: Vec<crate::mcp::Server>,
}

// The compaction of a new set of settings.
fn default_compaction() -> String {
    String::from(COMPACTION_HANDOFF)
}

// Which way a new set of settings sets the summarisation going.
fn default_compact_by() -> String {
    String::from(COMPACT_BY_PERCENT)
}

// The compaction of a word the window sent. A word that is neither of the two
// gives the handoff summary, which is what a new set of settings stands on.
pub fn sanitize_compaction(value: &str) -> String {
    match value.trim() {
        COMPACTION_OFF => String::from(COMPACTION_OFF),
        _ => String::from(COMPACTION_HANDOFF),
    }
}

// The way of setting the summarisation going. A word that is neither of the
// two gives the share of the room, which is what a new set of settings stands
// on.
pub fn sanitize_compact_by(value: &str) -> String {
    match value.trim() {
        COMPACT_BY_TOKENS => String::from(COMPACT_BY_TOKENS),
        _ => String::from(COMPACT_BY_PERCENT),
    }
}

// The share of the room of the talk. One per cent is the least, so a setting
// of nought does not ask for a summary on every message; a hundred per cent
// asks for one when the room is full.
pub fn sanitize_compact_percent(value: i64) -> u32 {
    value.clamp(1, 100) as u32
}

// The least room that must be left over. One token is the least, and a number
// over a million is a mistake rather than a setting.
pub fn sanitize_compact_reserve(value: i64) -> u32 {
    value.clamp(1, 1_000_000) as u32
}

// The handoff prompt, kept inside the ceiling of its length. A field left
// empty gives the prompt of the program again, for a chat without a prompt
// would ask the model for nothing at all.
pub fn sanitize_handoff_prompt(value: &str) -> String {
    let text = value.trim();
    if text.is_empty() {
        return String::from(DEFAULT_HANDOFF_PROMPT);
    }
    if text.chars().count() <= GREATEST_HANDOFF_PROMPT {
        return text.to_string();
    }
    text.chars().take(GREATEST_HANDOFF_PROMPT).collect()
}

// The prompt of the second ask, kept inside the same ceiling. A field left
// empty gives the short prompt of the program again.
pub fn sanitize_handoff_prompt_short(value: &str) -> String {
    let text = value.trim();
    if text.is_empty() {
        return String::from(DEFAULT_HANDOFF_PROMPT_SHORT);
    }
    if text.chars().count() <= GREATEST_HANDOFF_PROMPT {
        return text.to_string();
    }
    text.chars().take(GREATEST_HANDOFF_PROMPT).collect()
}

// Whether the talk of a chat has reached the point the settings set for the
// handoff summary. `used` is the room the talk takes and `total` the room the
// server gives. The rule stands here and nowhere else, so the field of the
// window and the check of the worker cannot drift apart.
pub fn compaction_wanted(cfg: &Config, used: u64, total: u64) -> bool {
    if sanitize_compaction(&cfg.chat_compaction) != COMPACTION_HANDOFF {
        return false;
    }
    // Nothing is known of the room, so nothing can be said about running out.
    if total == 0 {
        return false;
    }
    // A talk that stands over the room is past any threshold there can be.
    if used >= total {
        return true;
    }
    if sanitize_compact_by(&cfg.chat_compact_by) == COMPACT_BY_TOKENS {
        let reserve = cfg.chat_compact_reserve.max(1) as u64;
        return total - used <= reserve;
    }
    let percent = cfg.chat_compact_percent.clamp(1, 100) as u64;
    used * 100 >= total * percent
}

// The least a paste limit may be set to. Smaller numbers are lifted to it.
pub const LEAST_PASTE_LIMIT: u32 = 100;

// The most a paste limit may be set to, so one paste cannot ask for a file of
// more than a million characters.
pub const GREATEST_PASTE_LIMIT: u32 = 1_000_000;

// The place a paste of words turns into a file, in characters.
fn default_pasted_text_char_limit() -> u32 {
    6000
}

// The short wait of a question the model put to the user. Ten seconds is long
// enough to read a line and press a key, and short enough that a user who is
// not there is not waited for.
fn default_ask_seconds() -> u32 {
    10
}

// The longest wait either kind of question may be set to. A day of waiting is
// already the whole of a working day, and a wait longer than that is a fault of
// the setting rather than a wish of the model.
pub const GREATEST_ASK_SECONDS: u32 = 24 * 60 * 60;

// Keep the paste limit inside the two bounds above.
pub fn sanitize_pasted_text_limit(value: u32) -> u32 {
    value.clamp(LEAST_PASTE_LIMIT, GREATEST_PASTE_LIMIT)
}

// The width the chat always had, and the least it can be set to. A chat no
// narrower than this is as readable as the chat always was.
pub const LEAST_CHAT_WIDTH: u32 = 780;

// The most the field of the window may hold, in pixels. The page keeps the chat
// within 80 per cent of the window as well, so a number above the width of the
// window is answered with the room the window has.
pub const GREATEST_CHAT_WIDTH: u32 = 4000;

// The width of the chat of the program, which is the least of it too.
fn default_chat_width() -> u32 {
    LEAST_CHAT_WIDTH
}

// Keep the width of the chat inside the two bounds above.
pub fn sanitize_chat_width(value: u32) -> u32 {
    value.clamp(LEAST_CHAT_WIDTH, GREATEST_CHAT_WIDTH)
}

// A number of steps with no ceiling. The Chatting settings hold -1 for it, and
// llama.cpp uses the same number for "as much as the model can think of".
fn no_limit() -> i64 {
    -1
}

// How many times an answer is asked over when the model came to its own end
// without a word, a call or a fault of its own.
fn silence_retries() -> i64 {
    2
}

impl Default for Config {
    fn default() -> Self {
        Self {
            llama_dir: default_llama_dir(),
            models_dir: paths().models.display().to_string(),
            repositories_dir: String::new(),
            dark_mode: false,
            remote_port: DEFAULT_REMOTE_PORT,
            upnp_enabled: false,
            model_id: String::new(),
            model_settings: BTreeMap::new(),
            chat_collapse_thinking: true,
            chat_grow_thinking: true,
            chat_tools_in_thinking: true,
            chat_collapse_tools: true,
            chat_date_new_chat: true,
            chat_highlight: true,
            chat_bold_markdown: true,
            chat_silent_retries: 2,
            chat_chapters: true,
            chat_context_files: true,
            chat_max_tool_steps: -1,
            chat_max_answer_steps: -1,
            pasted_text_char_limit: default_pasted_text_char_limit(),
            chat_width: default_chat_width(),
            ask_seconds: default_ask_seconds(),
            ask_critical_seconds: 0,
            // The handoff summary stands between the first message of the
            // user and the end of the room, so a chat can go on for ever.
            chat_compaction: default_compaction(),
            chat_compact_by: default_compact_by(),
            chat_compact_percent: default_compact_percent(),
            chat_compact_reserve: default_compact_reserve(),
            chat_compact_prompt: default_handoff_prompt(),
            chat_compact_prompt_short: default_handoff_prompt_short(),
            chat_timeline: true,
            // No server of the user's own stands in a new set of settings.
            mcp_servers: Vec::new(),
        }
    }
}

impl Config {
    // Settings for one model. A missing entry gives the default settings.
    pub fn model(&self, id: &str) -> ModelSettings {
        self.model_settings.get(id).cloned().unwrap_or_default()
    }
}

// Places on disk that belong to this program.
pub struct Paths {
    // autumn_natter directory.
    pub data: PathBuf,
    // settings.json inside the data directory.
    pub settings_file: PathBuf,
    // Chats that belong to no repository.
    pub chats: PathBuf,
    // The chats of the repositories. They stand here and not inside a
    // repository, for a repository is the room a terminal command works in, and
    // it must not hold the talks of the model that works in it.
    pub repo_chats: PathBuf,
    // The record of every repository: its name, when it was made and used,
    // whether it is pinned. These stand here for the same reason the talks do,
    // and not in the folder of the repository beside the work.
    pub repo_records: PathBuf,
    // The logs of llama-server, one file for every start of it, the ten newest
    // of them. The Logs tab of the sheet of settings reads them.
    pub logs: PathBuf,
    // The talks the model has worked its way through, one file for every chat
    // that had the keeping of them switched on. They stand here, and not in the
    // folder of a repository, for a model that works in a repository must not
    // read or write over what another chat left of itself. llama-server is given
    // this folder when it starts, and it writes the files into it by name.
    pub contexts: PathBuf,
    // Default repositories directory.
    pub repositories: PathBuf,
    // Default models directory.
    pub models: PathBuf,
}

// Return the OS default place for program data.
// Linux: $XDG_CONFIG_HOME or ~/.config. Windows: %APPDATA%.
fn base_config_dir() -> PathBuf {
    if let Some(dir) = dirs::config_dir() {
        return dir;
    }
    // Fall back to a hidden directory in the home directory.
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".config")
}

use std::sync::OnceLock;

// Set by --data-dir. The first call wins.
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

fn data_root() -> PathBuf {
    DATA_DIR
        .get()
        .cloned()
        .unwrap_or_else(|| base_config_dir().join(APP_DIR_NAME))
}

// Read the paths of this program.
pub fn paths() -> Paths {
    let data = data_root();
    Paths {
        chats: data.join("chats"),
        repo_chats: data.join("repo_chats"),
        repo_records: data.join("repo_records"),
        logs: data.join("logs"),
        contexts: data.join("contexts"),
        repositories: data.join("repositories"),
        models: data.join("models"),
        settings_file: data.join("settings.json"),
        data,
    }
}

// Point the data directory somewhere else. Only the start-up code calls this,
// and it calls it before any other code reads the paths.
pub fn set_data_dir(dir: PathBuf) {
    let _ = DATA_DIR.set(dir);
}

// The tests that write through the data folder take turns through this one
// lock, so no test empties the folder of another while it works.
#[cfg(test)]
pub fn test_turn() -> std::sync::MutexGuard<'static, ()> {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    TURN.lock().expect("the lock of the tests")
}

// Create the data directory and the folders inside it.
pub fn ensure_dirs() -> std::io::Result<()> {
    let p = paths();
    fs::create_dir_all(&p.data)?;
    fs::create_dir_all(&p.chats)?;
    fs::create_dir_all(&p.repo_chats)?;
    fs::create_dir_all(&p.repo_records)?;
    fs::create_dir_all(&p.logs)?;
    fs::create_dir_all(&p.contexts)?;
    fs::create_dir_all(&p.models)?;
    Ok(())
}

// Read settings.json. A missing file gives the default settings. The file stands
// in the folder of the data of this place, so a run that points the program at
// another folder with `--data-dir` reads the settings of that folder, and the
// folders of the model and of llama.cpp follow it.
pub fn load() -> Config {
    let file = paths().settings_file;
    match fs::read_to_string(&file) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

// Write settings.json. Parent directories are created when needed.
pub fn save(cfg: &Config) -> std::io::Result<()> {
    let p = paths();
    fs::create_dir_all(&p.data)?;
    let text = serde_json::to_string_pretty(cfg).unwrap_or_else(|_| String::from("{}"));
    // Write to a second file first, then rename. A crash cannot leave a short
    // file behind.
    let tmp = p.data.join("settings.json.new");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, &p.settings_file)
}

// Directory that holds the repositories. Create it when it does not exist.
pub fn repositories_dir(cfg: &Config) -> PathBuf {
    let dir = if cfg.repositories_dir.trim().is_empty() {
        paths().repositories
    } else {
        PathBuf::from(&cfg.repositories_dir)
    };
    let _ = fs::create_dir_all(&dir);
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    // The switches of the Chatting section stand on for a new set of settings,
    // and the two rooms for steps stand unlimited.
    #[test]
    fn the_chatting_switches_stand_on() {
        let cfg = Config::default();
        assert!(cfg.chat_collapse_thinking);
        assert!(cfg.chat_grow_thinking);
        assert!(cfg.chat_tools_in_thinking);
        assert!(cfg.chat_collapse_tools);
        assert!(cfg.chat_date_new_chat);
        assert!(cfg.chat_highlight);
        assert!(cfg.chat_chapters);
        assert!(cfg.chat_context_files);
        assert_eq!(cfg.chat_max_tool_steps, -1);
        assert_eq!(cfg.chat_max_answer_steps, -1);
    }

    // A number of steps is kept and read back, and a number below -1 is read as
    // no limit rather than as a nonsense ceiling.
    #[test]
    fn the_rooms_for_steps_are_kept() {
        let cfg = Config {
            chat_max_tool_steps: 3,
            chat_max_answer_steps: 40,
            ..Config::default()
        };
        let text = serde_json::to_string(&cfg).expect("the settings are written");
        let back: Config = serde_json::from_str(&text).expect("the settings are read");
        assert_eq!(back.chat_max_tool_steps, 3);
        assert_eq!(back.chat_max_answer_steps, 40);
        // The engine reads a number as a ceiling, and -1 as no ceiling at all.
        assert_eq!(engine_steps(back.chat_max_tool_steps), Some(3));
        assert_eq!(engine_steps(-1), None);
        assert_eq!(engine_steps(-9), None);
        assert_eq!(engine_steps(0), Some(0));
    }

    // The defaults of the Sampling section stand as the program wants them.
    #[test]
    fn the_sampling_defaults_are_kept() {
        let s = ModelSettings::default();
        assert!((s.temperature - 0.7).abs() < f64::EPSILON);
        assert!((s.top_p - 0.95).abs() < f64::EPSILON);
        assert_eq!(s.top_k, 20);
        assert!((s.min_p - 0.0).abs() < f64::EPSILON);
        assert!((s.repeat_penalty - 1.0).abs() < f64::EPSILON);
        assert!((s.presence_penalty - 0.0).abs() < f64::EPSILON);
        assert_eq!(s.max_tokens, -1);
        assert_eq!(s.seed, -1);
        // The values are written in the short form, so the window shows 0.7
        // and not the full form of a small number.
        let text = serde_json::to_string(&s).expect("the settings are written");
        assert!(text.contains("\"temperature\":0.7"));
        assert!(text.contains("\"top_p\":0.95"));
        // A file written before the section existed still reads, with the
        // defaults of the section in place.
        let old = "{\"ctx_size\":8192,\"kv_index\":1,\"spec_index\":0,\"draft_model\":\"\",\
                    \"draft_tokens\":4,\"parallel\":1,\"batch_size\":512,\"ubatch_size\":128,\
                    \"tensor_parallel\":false,\"vision\":false,\"reasoning_budget\":-1,\
                    \"reasoning_message\":\"\",\"chat_template\":\"\",\"load_index\":1,\
                    \"extra_args\":\"\",\"vram_budget\":0,\"gpu_enabled\":[]}";
        let read: ModelSettings =
            serde_json::from_str(old).expect("an old model settings entry reads");
        assert!((read.temperature - 0.7).abs() < f64::EPSILON);
        assert_eq!(read.top_k, 20);
        assert_eq!(read.max_tokens, -1);
        assert_eq!(read.seed, -1);
    }

    // A settings file written before the Chatting section existed still reads,
    // and its switches are on rather than off.
    #[test]
    fn an_old_settings_file_keeps_the_switches_on() {
        let old = "{\"llama_dir\":\"\",\"models_dir\":\"\",\"repositories_dir\":\"\",\
                   \"dark_mode\":false,\"remote_port\":8723,\"upnp_enabled\":false,\
                   \"model_id\":\"\",\"model_settings\":{}}";
        let read: Config = serde_json::from_str(old).expect("an old settings file reads");
        assert!(read.chat_collapse_thinking);
        assert!(read.chat_grow_thinking);
        assert!(read.chat_tools_in_thinking);
        assert!(read.chat_collapse_tools);
        assert!(read.chat_date_new_chat);
        assert!(read.chat_highlight);
        assert!(read.chat_chapters);
        // The keeping of the talks was not in the file either, and it reads on
        // rather than off: a user who never heard of it is not left with the
        // chats read through again on every coming back.
        assert!(read.chat_context_files);
        // The two rooms for steps were not in the file either, so they read as
        // unlimited rather than as a ceiling of nothing.
        assert_eq!(read.chat_max_tool_steps, -1);
        assert_eq!(read.chat_max_answer_steps, -1);
    }

    // A paste of words into the chat box becomes a file at six thousand
    // characters, and a settings file written before this setting stood reads
    // with the same number. The number keeps inside its two bounds.
    #[test]
    fn a_paste_of_words_becomes_a_file_after_six_thousand() {
        assert_eq!(Config::default().pasted_text_char_limit, 6000);
        let old = "{\"llama_dir\":\"\",\"models_dir\":\"\",\"repositories_dir\":\"\",\
                   \"dark_mode\":false,\"remote_port\":8723,\"upnp_enabled\":false,\
                   \"model_id\":\"\",\"model_settings\":{}}";
        let read: Config = serde_json::from_str(old).expect("an old settings file reads");
        assert_eq!(read.pasted_text_char_limit, 6000);
        assert_eq!(sanitize_pasted_text_limit(9), 100);
        assert_eq!(sanitize_pasted_text_limit(5000), 5000);
        assert_eq!(sanitize_pasted_text_limit(2_000_000), 1_000_000);
    }

    // The width of the chat. The width the chat always had is the default and
    // the least, so a settings file written before this setting stood keeps the
    // chat as wide as it always was. The field cannot go below that width, and
    // cannot ask for a chat wider than a wall.
    #[test]
    fn the_width_of_the_chat_keeps_inside_its_bounds() {
        assert_eq!(Config::default().chat_width, LEAST_CHAT_WIDTH);
        let old = "{\"llama_dir\":\"\",\"models_dir\":\"\",\"repositories_dir\":\"\",\
                   \"dark_mode\":false,\"remote_port\":8723,\"upnp_enabled\":false,\
                   \"model_id\":\"\",\"model_settings\":{}}";
        let read: Config = serde_json::from_str(old).expect("an old settings file reads");
        assert_eq!(read.chat_width, LEAST_CHAT_WIDTH);
        assert_eq!(sanitize_chat_width(400), LEAST_CHAT_WIDTH);
        assert_eq!(sanitize_chat_width(1100), 1100);
        assert_eq!(sanitize_chat_width(9000), GREATEST_CHAT_WIDTH);
    }

    // The Compaction section of the settings. A new set of settings asks for
    // the handoff summary, by the share of the room, at 85 per cent, with the
    // prompt of the program in the text box, and with the timeline of the
    // summarisations standing beside the chat.
    #[test]
    fn the_handoff_summary_is_the_default_compaction() {
        let cfg = Config::default();
        assert_eq!(cfg.chat_compaction, COMPACTION_HANDOFF);
        assert_eq!(cfg.chat_compact_by, COMPACT_BY_PERCENT);
        assert_eq!(cfg.chat_compact_percent, 85);
        assert_eq!(cfg.chat_compact_reserve, 10240);
        assert!(cfg.chat_timeline);
        assert_eq!(cfg.chat_compact_prompt, DEFAULT_HANDOFF_PROMPT);
        // The prompt of the program is a text a model can be sent, and it is
        // short enough to fit the ceiling of the field.
        assert!(cfg.chat_compact_prompt.starts_with("Write a detailed"));
        // The prompt of the program names a length, for a summary that never ends
        // leaves the room it was asked to make unmade.
        assert!(cfg.chat_compact_prompt.contains("three hundred words"));
        assert!(cfg.chat_compact_prompt.chars().count() <= GREATEST_HANDOFF_PROMPT);
        // The second ask has a prompt of its own, and it leans towards a short
        // summary rather than a long one.
        assert_eq!(cfg.chat_compact_prompt_short, DEFAULT_HANDOFF_PROMPT_SHORT);
        assert!(cfg.chat_compact_prompt_short.starts_with("Write a very short"));
        assert!(cfg
            .chat_compact_prompt_short
            .chars()
            .count()
            <= GREATEST_HANDOFF_PROMPT);
    }

    // A settings file written before the Compaction section existed still reads,
    // and it reads with the handoff summary asked for rather than with nothing.
    #[test]
    fn an_old_settings_file_asks_for_the_handoff_summary() {
        let old = "{\"llama_dir\":\"\",\"models_dir\":\"\",\"repositories_dir\":\"\",\
                   \"dark_mode\":false,\"remote_port\":8723,\"upnp_enabled\":false,\
                   \"model_id\":\"\",\"model_settings\":{}}";
        let read: Config = serde_json::from_str(old).expect("an old settings file reads");
        assert_eq!(read.chat_compaction, COMPACTION_HANDOFF);
        assert_eq!(read.chat_compact_by, COMPACT_BY_PERCENT);
        assert_eq!(read.chat_compact_percent, 85);
        assert_eq!(read.chat_compact_reserve, 10240);
        assert_eq!(read.chat_compact_prompt, DEFAULT_HANDOFF_PROMPT);
        assert_eq!(
            read.chat_compact_prompt_short,
            DEFAULT_HANDOFF_PROMPT_SHORT
        );
        assert!(read.chat_timeline);
    }

    // The fields of the Compaction section are kept inside their bounds. A word
    // that is neither of the two compactions gives the handoff summary, and a
    // prompt left empty gives the prompt of the program again.
    #[test]
    fn the_compaction_fields_are_kept_inside_their_bounds() {
        assert_eq!(sanitize_compaction("off"), COMPACTION_OFF);
        assert_eq!(sanitize_compaction("handoff"), COMPACTION_HANDOFF);
        assert_eq!(sanitize_compaction("a word of the user"), COMPACTION_HANDOFF);
        assert_eq!(sanitize_compact_by("tokens"), COMPACT_BY_TOKENS);
        assert_eq!(sanitize_compact_by("percent"), COMPACT_BY_PERCENT);
        assert_eq!(sanitize_compact_by(""), COMPACT_BY_PERCENT);
        assert_eq!(sanitize_compact_percent(0), 1);
        assert_eq!(sanitize_compact_percent(72), 72);
        assert_eq!(sanitize_compact_percent(400), 100);
        assert_eq!(sanitize_compact_reserve(0), 1);
        assert_eq!(sanitize_compact_reserve(10240), 10240);
        assert_eq!(sanitize_compact_reserve(9_000_000), 1_000_000);
        // The prompt of the user is kept, and a long one is cut at the ceiling
        // rather than refused.
        assert_eq!(sanitize_handoff_prompt("say something short"), "say something short");
        let long = "x".repeat(GREATEST_HANDOFF_PROMPT + 500);
        assert_eq!(
            sanitize_handoff_prompt(&long).chars().count(),
            GREATEST_HANDOFF_PROMPT
        );
        assert_eq!(sanitize_handoff_prompt("   "), DEFAULT_HANDOFF_PROMPT);
        // The prompt of the second ask is kept by the same rule, and a field
        // left empty gives the short prompt of the program again.
        assert_eq!(
            sanitize_handoff_prompt_short("say very little"),
            "say very little"
        );
        assert_eq!(
            sanitize_handoff_prompt_short(&long).chars().count(),
            GREATEST_HANDOFF_PROMPT
        );
        assert_eq!(
            sanitize_handoff_prompt_short("  "),
            DEFAULT_HANDOFF_PROMPT_SHORT
        );
    }

    // The rule that sets the summarisation going. The share of the room works
    // from the talk in hand, the room left works from what is over, and a talk
    // past the room is past either threshold.
    #[test]
    fn the_summarisation_starts_at_the_share_asked_for_or_at_the_room_left() {
        let by_percent = Config {
            chat_compact_by: String::from(COMPACT_BY_PERCENT),
            chat_compact_percent: 85,
            ..Config::default()
        };
        assert!(!compaction_wanted(&by_percent, 4096, 8192));
        assert!(!compaction_wanted(&by_percent, 6962, 8192));
        assert!(compaction_wanted(&by_percent, 6964, 8192));
        assert!(compaction_wanted(&by_percent, 8192, 8192));
        assert!(compaction_wanted(&by_percent, 9000, 8192));
        // Nothing is known of the room, so nothing is said about running out.
        assert!(!compaction_wanted(&by_percent, 4000, 0));

        let by_tokens = Config {
            chat_compact_by: String::from(COMPACT_BY_TOKENS),
            chat_compact_reserve: 10240,
            ..Config::default()
        };
        // A room of 32 768 with 10 000 left over is past the least left over.
        assert!(compaction_wanted(&by_tokens, 22768, 32768));
        // With 12 000 left over there is still room for the next answer.
        assert!(!compaction_wanted(&by_tokens, 20768, 32768));
        // The number of the share does not matter on this road.
        assert!(compaction_wanted(&by_tokens, 7000, 8192));

        // No compaction at all, whatever the talk.
        let off = Config {
            chat_compaction: String::from(COMPACTION_OFF),
            ..Config::default()
        };
        assert!(!compaction_wanted(&off, 9000, 8192));
        assert!(!compaction_wanted(&off, 8192, 8192));
    }
}

// Look for the llama-server binary in the usual build places.
fn default_llama_dir() -> String {
    let mut tried: Vec<PathBuf> = Vec::new();
    if let Ok(env) = std::env::var("AUTUMN_NATTER_LLAMA_DIR") {
        if !env.trim().is_empty() {
            tried.push(PathBuf::from(env));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            tried.push(dir.to_path_buf());
            tried.push(dir.join("llama"));
            tried.push(dir.join("../llamacpp/build/bin"));
            tried.push(dir.join("../../llamacpp/build/bin"));
        }
    }
    // A checkout next to the program is common during development.
    if let Ok(cwd) = std::env::current_dir() {
        tried.push(cwd.join("llamacpp/build/bin"));
    }
    tried.push(PathBuf::from("/usr/local/bin"));
    tried.push(PathBuf::from("/usr/bin"));
    for dir in tried {
        if server_in(&dir).is_some() {
            return dir.display().to_string();
        }
    }
    String::new()
}

// Return the llama-server binary inside a directory, if it is there.
pub fn server_in(dir: &Path) -> Option<PathBuf> {
    let names = [
        "llama-server",
        "llama-server.exe",
        "bin/llama/llama-server",
        "bin/llama/llama-server.exe",
        "bin/llama-server",
        "bin/llama-server.exe",
    ];
    for name in names {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
