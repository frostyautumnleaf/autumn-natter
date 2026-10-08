// Repositories and chats on disk.
//
// Every repository has its own directory. The directory name follows the rule
// "name_id", where the name uses only letters, digits and dashes, and the id is
// ten lowercase hexadecimal characters.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config;

// Seconds since 1970-01-01, in the local time zone of the machine.
pub fn now_secs() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(_) => 0,
    }
}

// Turn seconds into a short text for the chat list.
// A time from today shows as hours and minutes. An older time shows the date.
pub fn format_when(secs: i64) -> String {
    if secs <= 0 {
        return String::new();
    }
    #[cfg(unix)]
    {
        let t = secs as libc::time_t;
        let mut local: libc::tm = unsafe { std::mem::zeroed() };
        // SAFETY: localtime_r fills the struct it is given.
        let filled = unsafe { libc::localtime_r(&t, &mut local) };
        if !filled.is_null() {
            let mut today: libc::tm = unsafe { std::mem::zeroed() };
            let now = now_secs() as libc::time_t;
            let _ = unsafe { libc::localtime_r(&now, &mut today) };
            let same_day = local.tm_yday == today.tm_yday && local.tm_year == today.tm_year;
            if same_day {
                return format!("{:02}:{:02}", local.tm_hour, local.tm_min);
            }
            return format!(
                "{:04}-{:02}-{:02}",
                local.tm_year + 1900,
                local.tm_mon + 1,
                local.tm_mday
            );
        }
    }
    let (y, m, d) = civil_from_days(secs.div_euclid(86400));
    format!("{:04}-{:02}-{:02}", y, m, d)
}

// Turn seconds into a clock time such as 14:32.
pub fn format_clock(secs: i64) -> String {
    if secs <= 0 {
        return String::new();
    }
    #[cfg(unix)]
    {
        let t = secs as libc::time_t;
        let mut local: libc::tm = unsafe { std::mem::zeroed() };
        // SAFETY: localtime_r fills the struct it is given.
        let filled = unsafe { libc::localtime_r(&t, &mut local) };
        if !filled.is_null() {
            return format!("{:02}:{:02}", local.tm_hour, local.tm_min);
        }
    }
    let secs_of_day = secs.rem_euclid(86400);
    format!(
        "{:02}:{:02}",
        secs_of_day / 3600,
        (secs_of_day / 60) % 60
    )
}

// Days since the epoch to a year, month and day. This is the civil from days
// algorithm. It avoids a date crate.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097).max(0);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// Exactly ten lowercase hexadecimal characters from the system random source.
pub fn new_id() -> String {
    let mut bytes = [0u8; 5];
    if getrandom::fill(&mut bytes).is_err() {
        // A machine with no random source still needs an id.
        let t = now_secs() as u64;
        for i in 0..5 {
            bytes[i] = ((t >> (i * 8)) ^ (i as u64 * 37)) as u8;
        }
    }
    let mut out = String::with_capacity(10);
    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

// Keep letters, digits and dashes. Spaces become dashes. Other characters go
// away. The result never starts or ends with a dash.
pub fn sanitize_name(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == ' ' || ch == '-' || ch == '_' {
            out.push('-');
        }
        // A dash is added for an underscore too, so names stay uniform.
    }
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').to_string()
}

// Directory name for a repository. A name with no usable character gives
// "repository-<id>".
pub fn repo_dir_name(raw: &str, id: &str) -> String {
    let clean = sanitize_name(raw);
    if clean.is_empty() {
        format!("repository-{}", id)
    } else {
        format!("{}_{}", clean, id)
    }
}

// Parse a directory name back to the id part.
fn id_from_dir_name(name: &str) -> Option<String> {
    let tail = name.rsplit('_').next()?;
    if tail.len() != 10 {
        return None;
    }
    if tail.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c.to_ascii_lowercase())) {
        Some(tail.to_ascii_lowercase())
    } else {
        None
    }
}

// One repository on disk.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Repo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub last_used: i64,
    // Full path of the repository directory. It is not stored in repo.json.
    #[serde(skip, default)]
    pub path: PathBuf,
}

// The stored facts about one chat.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMeta {
    pub id: String,
    pub title: String,
    // Empty means the chat belongs to no repository.
    pub repo_id: String,
    pub model_id: String,
    pub created: i64,
    pub updated: i64,
    // The terminal tool may be used when this is true.
    pub code_enabled: bool,
    // Run the tool inside bubblewrap when this is true.
    pub sandbox_enabled: bool,
    // Tokens used by the last answer.
    pub context_used: u64,
}

impl Default for ChatMeta {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            repo_id: String::new(),
            model_id: String::new(),
            created: 0,
            updated: 0,
            code_enabled: false,
            // The sandbox is on unless the user turns it off.
            sandbox_enabled: true,
            context_used: 0,
        }
    }
}

// One message. The role is "You", "Assistant", "Tool" or "Note".
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub body: String,
    // The model's reasoning, streamed before the answer. Empty for most models.
    #[serde(default)]
    pub reasoning: String,
    // Tool name plus arguments, for a tool call. Empty for other messages.
    #[serde(default)]
    pub tool: String,
    #[serde(default)]
    pub mono: bool,
    #[serde(default)]
    pub failed: bool,
    #[serde(default)]
    pub when: i64,
    // Id the model gave to this tool call. The result must carry it back.
    #[serde(default)]
    pub call_id: String,
    // Name of the tool the model asked for.
    #[serde(default)]
    pub tool_name: String,
    // Arguments of the tool call, as the model wrote them.
    #[serde(default)]
    pub tool_args: String,
}

// A chat with its messages.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Chat {
    pub meta: ChatMeta,
    #[serde(default)]
    pub messages: Vec<Message>,
    // True while a chat is only in the memory. The carousel shows such a chat
    // as a blank one, and drops it when the user turns away without writing.
    // It is never written to disk, so the files hold only real chats.
    #[serde(default, skip_serializing)]
    pub transient: bool,
}

// The first line of the newest message, in a short form. The carousel uses it
// for the soft picture of a chat at the side of the window.
pub fn preview(messages: &[Message]) -> String {
    let last = match messages.last() {
        Some(message) => message,
        None => return String::from("Nothing written yet"),
    };
    let line = last.body.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for ch in line.chars() {
        out.push(ch);
        if out.chars().count() >= 140 {
            out.push('\u{2026}');
            break;
        }
    }
    out
}

impl Chat {
    pub fn new(repo_id: &str, model_id: &str) -> Chat {
        let now = now_secs();
        Chat {
            meta: ChatMeta {
                id: new_id(),
                title: "New chat".to_string(),
                repo_id: repo_id.to_string(),
                model_id: model_id.to_string(),
                created: now,
                updated: now,
                ..Default::default()
            },
            messages: Vec::new(),
            transient: false,
        }
    }

    // A blank chat for the front of the carousel. It is not on disk, and it
    // goes away again when the user turns the carousel away from it without
    // writing anything.
    pub fn blank(repo_id: &str, model_id: &str) -> Chat {
        let mut chat = Chat::new(repo_id, model_id);
        chat.transient = true;
        chat
    }

    // Short title from the first message of the user.
    pub fn title_from_prompt(text: &str) -> String {
        let line = text.lines().next().unwrap_or("").trim();
        let mut out = String::new();
        for ch in line.chars() {
            if ch == '\n' || ch == '\t' {
                break;
            }
            out.push(ch);
            if out.chars().count() >= 60 {
                break;
            }
        }
        if out.is_empty() {
            return "New chat".to_string();
        }
        if line.chars().count() > 60 {
            out.push('…');
        }
        out
    }
}

// Reads and writes the data on disk.
pub struct Store {
    repos_dir: PathBuf,
}

impl Store {
    pub fn new(repos_dir: PathBuf) -> Store {
        let _ = fs::create_dir_all(&repos_dir);
        Store { repos_dir }
    }

    // Directory that holds the chats of a repository, or of the loose chats.
    fn chats_dir(&self, repo_id: &str) -> Option<PathBuf> {
        if repo_id.is_empty() {
            return Some(config::paths().chats);
        }
        self.repo_path(repo_id).map(|p| p.join("chats"))
    }

    // Find a repository directory by its id.
    pub fn repo_path(&self, id: &str) -> Option<PathBuf> {
        if id.is_empty() {
            return None;
        }
        let entries = fs::read_dir(&self.repos_dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = path.file_name()?.to_string_lossy().to_string();
            // The directory of a repository is "name_id" or "repository-id".
            let found = if let Some(tail) = name.strip_prefix("repository-") {
                tail == id
            } else {
                id_from_dir_name(&name).as_deref() == Some(id)
            };
            if found {
                return Some(path);
            }
        }
        None
    }

    // All repositories, most recently used first.
    pub fn scan_repos(&self) -> Vec<Repo> {
        let mut out: Vec<Repo> = Vec::new();
        let entries = match fs::read_dir(&self.repos_dir) {
            Ok(e) => e,
            Err(_) => return out,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = match path.file_name() {
                Some(n) => n.to_string_lossy().to_string(),
                None => continue,
            };
            let repo_file = path.join("repo.json");
            let mut repo = match fs::read_to_string(&repo_file) {
                Ok(text) => serde_json::from_str::<Repo>(&text).unwrap_or_default(),
                Err(_) => Repo::default(),
            };
            // A directory without a usable record is still shown, so the user
            // sees what is on disk.
            if repo.id.is_empty() {
                let id = if let Some(tail) = name.strip_prefix("repository-") {
                    Some(tail.to_string())
                } else {
                    id_from_dir_name(&name)
                };
                match id {
                    Some(id) => {
                        repo.id = id;
                        repo.name = human_name(&name);
                    }
                    None => continue,
                }
            }
            repo.path = path;
            out.push(repo);
        }
        out.sort_by(|a, b| b.last_used.cmp(&a.last_used));
        out
    }

    // Create a repository directory and its record file.
    pub fn create_repo(&self, raw_name: &str) -> Result<Repo, String> {
        let id = new_id();
        let dir_name = repo_dir_name(raw_name, &id);
        let path = self.repos_dir.join(&dir_name);
        let now = now_secs();
        let repo = Repo {
            id: id.clone(),
            name: raw_name.trim().to_string(),
            created: now,
            last_used: now,
            path: path.clone(),
        };
        if path.join("repo.json").is_file() {
            return Err("a repository with that name already exists".to_string());
        }
        fs::create_dir_all(&path).map_err(|e| format!("cannot create {}: {}", path.display(), e))?;
        fs::create_dir_all(path.join("chats")).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(&repo).unwrap_or_else(|_| String::from("{}"));
        fs::write(path.join("repo.json"), text).map_err(|e| e.to_string())?;
        Ok(repo)
    }

    // Store the time a repository was last used.
    pub fn touch_repo(&self, repo: &mut Repo) {
        repo.last_used = now_secs();
        if repo.path.join("repo.json").is_file() {
            let text = serde_json::to_string_pretty(repo).unwrap_or_default();
            let _ = fs::write(repo.path.join("repo.json"), text);
        }
    }

    // Chats of one scope, newest first. repo_id "" gives the loose chats.
    pub fn list_chats(&self, repo_id: &str) -> Vec<Chat> {
        let dir = match self.chats_dir(repo_id) {
            Some(d) => d,
            None => return Vec::new(),
        };
        let mut out: Vec<Chat> = Vec::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if let Ok(text) = fs::read_to_string(&path) {
                    if let Ok(chat) = serde_json::from_str::<Chat>(&text) {
                        if !chat.meta.id.is_empty() {
                            out.push(chat);
                        }
                    }
                }
            }
        }
        out.sort_by(|a, b| b.meta.updated.cmp(&a.meta.updated));
        out
    }

    // Write a chat. Parent directories are created when needed.
    //
    // A blank chat of the carousel lives only in the memory. Writing one would
    // make it a real chat, and it would come back after the next start.
    pub fn save_chat(&self, chat: &Chat) {
        if chat.transient {
            return;
        }
        let dir = match self.chats_dir(&chat.meta.repo_id) {
            Some(d) => d,
            None => return,
        };
        let _ = fs::create_dir_all(&dir);
        let path = dir.join(format!("{}.json", chat.meta.id));
        if let Ok(text) = serde_json::to_string(&chat) {
            let _ = fs::write(path, text);
        }
    }

    // Take one chat away for good. The carousel calls this when it drops a
    // blank chat, so a file of an older run cannot bring that chat back.
    pub fn delete_chat(&self, repo_id: &str, id: &str) {
        if id.is_empty() {
            return;
        }
        let dir = match self.chats_dir(repo_id) {
            Some(d) => d,
            None => return,
        };
        let _ = fs::remove_file(dir.join(format!("{id}.json")));
    }
}

// A readable name from a directory name, for a folder with no record file.
fn human_name(dir_name: &str) -> String {
    let stem = match dir_name.rsplit_once('_') {
        Some((head, _)) if !head.is_empty() => head,
        _ => dir_name,
    };
    stem.replace('-', " ")
}
