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
    // they are sent to the server on every start.
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    pub min_p: f32,
    // 1.0 turns the penalty off.
    pub repeat_penalty: f32,
    // 0.0 turns the penalty off.
    pub presence_penalty: f32,
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
    // Fold a tool panel up while its command runs.
    #[serde(default = "on_by_default")]
    pub chat_collapse_tools: bool,
    // Give the words of a block of code the colours of their kind.
    #[serde(default = "on_by_default")]
    pub chat_highlight: bool,
    // Let the model mark the chapters of a chat, and show them beside it.
    #[serde(default = "on_by_default")]
    pub chat_chapters: bool,
    // How many terminal commands one answer may run. -1 leaves no limit, so a
    // model may run a command as often as it wants inside one answer.
    #[serde(default = "no_limit")]
    pub chat_max_tool_steps: i64,
    // How many round trips one answer may take in all: a round trip is one turn
    // of the model, and one more follows every command it asks for. -1 leaves
    // no limit, so an answer ends only when the model stops by itself.
    #[serde(default = "no_limit")]
    pub chat_max_answer_steps: i64,
}

// A number of steps with no ceiling. The Chatting settings hold -1 for it, and
// llama.cpp uses the same number for "as much as the model can think of".
fn no_limit() -> i64 {
    -1
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
            chat_collapse_tools: true,
            chat_highlight: true,
            chat_chapters: true,
            chat_max_tool_steps: -1,
            chat_max_answer_steps: -1,
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
    fs::create_dir_all(&p.models)?;
    Ok(())
}

// Read settings.json. A missing file gives the default settings.
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
        assert!(cfg.chat_collapse_tools);
        assert!(cfg.chat_highlight);
        assert!(cfg.chat_chapters);
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
        assert!((s.temperature - 0.7).abs() < f32::EPSILON);
        assert!((s.top_p - 0.95).abs() < f32::EPSILON);
        assert_eq!(s.top_k, 20);
        assert!((s.min_p - 0.0).abs() < f32::EPSILON);
        assert!((s.repeat_penalty - 1.0).abs() < f32::EPSILON);
        assert!((s.presence_penalty - 0.0).abs() < f32::EPSILON);
        assert_eq!(s.max_tokens, -1);
        assert_eq!(s.seed, -1);
        // A file written before the section existed still reads, with the
        // defaults of the section in place.
        let old = "{\"ctx_size\":8192,\"kv_index\":1,\"spec_index\":0,\"draft_model\":\"\",\
                    \"draft_tokens\":4,\"parallel\":1,\"batch_size\":512,\"ubatch_size\":128,\
                    \"tensor_parallel\":false,\"vision\":false,\"reasoning_budget\":-1,\
                    \"reasoning_message\":\"\",\"chat_template\":\"\",\"load_index\":1,\
                    \"extra_args\":\"\",\"vram_budget\":0,\"gpu_enabled\":[]}";
        let read: ModelSettings =
            serde_json::from_str(old).expect("an old model settings entry reads");
        assert!((read.temperature - 0.7).abs() < f32::EPSILON);
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
        assert!(read.chat_collapse_tools);
        assert!(read.chat_highlight);
        assert!(read.chat_chapters);
        // The two rooms for steps were not in the file either, so they read as
        // unlimited rather than as a ceiling of nothing.
        assert_eq!(read.chat_max_tool_steps, -1);
        assert_eq!(read.chat_max_answer_steps, -1);
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
