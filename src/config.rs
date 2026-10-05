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
    // -1 means no limit. 0 stops thinking at once.
    pub reasoning_budget: i64,
    pub reasoning_message: String,
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
            reasoning_budget: -1,
            reasoning_message: String::new(),
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
