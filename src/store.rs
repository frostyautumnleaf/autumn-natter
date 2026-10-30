// Repositories and chats on disk.
//
// Every repository has its own directory. The directory name follows the rule
// "name_id", where the name uses only letters, digits and dashes, and the id is
// ten lowercase hexadecimal characters.
//
// The chats of a repository do NOT stand inside that directory. A terminal
// command of a chat works in the folder of its repository, so a chat kept there
// could be read, and written over, by the very model that is talking in it. The
// talks of a repository therefore stand under the data directory, at
// `repo_chats/<repository id>/`, and only the work of the chat stands in the
// repository.
//
// The record of a repository - its name, when it was made and used, whether it
// is pinned - does not stand inside that directory either, for the same reason:
// a model that can read the folder it works in can read the list of the other
// repositories, and write over its own. The records stand under the data
// directory, at `repo_records/<repository id>.json`.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
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

// The week of this program, counted from Monday.
const WEEK: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

// Turn seconds into the day of the week, the day and the clock time, all of them
// in the local time of the machine, such as Sunday, 2026-10-25, 19:40. The line
// the settings put before the model is read by the model, and the day of the
// week costs nothing and says something the number of the day does not. The
// three parts are taken from one reading of the clock, so the day of the week
// always belongs to the day that stands beside it.
pub fn format_date_clock(secs: i64) -> String {
    if secs <= 0 {
        return String::new();
    }
    #[cfg(unix)]
    {
        let when = secs as libc::time_t;
        let mut local: libc::tm = unsafe { std::mem::zeroed() };
        // SAFETY: localtime_r fills the struct it is given.
        let filled = unsafe { libc::localtime_r(&when, &mut local) };
        if !filled.is_null() {
            // tm_wday counts from Sunday, which stands last in WEEK.
            let weekday = WEEK[(local.tm_wday + 6).rem_euclid(7) as usize];
            return format!(
                "{}, {:04}-{:02}-{:02}, {:02}:{:02}",
                weekday,
                local.tm_year + 1900,
                local.tm_mon + 1,
                local.tm_mday,
                local.tm_hour,
                local.tm_min
            );
        }
    }
    let days = secs.div_euclid(86400);
    // 1970-01-01, the zero of the count, was a Thursday: the fourth of WEEK.
    let weekday = WEEK[(days + 3).rem_euclid(7) as usize];
    let (y, m, d) = civil_from_days(days);
    format!(
        "{}, {:04}-{:02}-{:02}, {}",
        weekday,
        y,
        m,
        d,
        format_clock(secs)
    )
}

// Turn seconds into the day and the clock time of it, such as
// 2026-10-20, 14:32. The heading of a summarisation is looked at days after it
// stood, so the hour of it alone would not say which day it was.
pub fn format_day_clock(secs: i64) -> String {
    if secs <= 0 {
        return String::new();
    }
    let days = secs.div_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}, {}", y, m, d, format_clock(secs))
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
    format!("{:02}:{:02}", secs_of_day / 3600, (secs_of_day / 60) % 60)
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
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = ((t >> (i * 8)) ^ (i as u64 * 37)) as u8;
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
    if tail
        .chars()
        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c.to_ascii_lowercase()))
    {
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
    // A repository the reader pinned stands in the `Pinned` group of the side
    // bar, above the other groups of the list. A chat of a pinned repository is
    // not pinned with it: the reader pins a chat on its own.
    #[serde(default)]
    pub pinned: bool,
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
    // The web search tool may be used when this is true. It stands on a switch of
    // its own, for it reaches the web out of the program and never the machine.
    // A chat written before the switch existed starts with it off.
    #[serde(default)]
    pub search_enabled: bool,
    // Run the tool inside bubblewrap when this is true.
    pub sandbox_enabled: bool,
    // The three settings of the sandbox. All of them stand off, for a chat and
    // its commands reach nothing beyond the working directory until the reader
    // opens one of them in the menu of the sandbox badge. A chat written before
    // they existed carries none of them and reads with all three off.
    // A command may reach the network.
    #[serde(default)]
    pub sandbox_network: bool,
    // `pip install` works, into a Python environment of the chat's own.
    #[serde(default)]
    pub sandbox_pip: bool,
    // The commands that fetch a page of the web, `wget`, `curl` and `fetch`, may
    // run.
    #[serde(default)]
    pub sandbox_fetch: bool,
    // Thinking mode: "auto", "off", "on", "low", "medium", "high", "xhigh".
    #[serde(default)]
    pub thinking_mode: String,
    // Keep the thinking of the turns before the last answer in the words the
    // model is sent. llama.cpp keeps it by default, so a chat written before
    // the switch existed reads as kept.
    #[serde(default = "preserve_thinking_on")]
    pub preserve_thinking: bool,
    // A chat the reader pinned stands in the `Pinned` group of the side bar,
    // above the groups of the repositories.
    #[serde(default)]
    pub pinned: bool,
    // A chat the reader put away. It stands in no group of the side bar, and is
    // read again from the list of the archived chats, where it can be brought
    // back.
    #[serde(default)]
    pub archived: bool,
    // The moment the chat was last opened. The side bar puts the chat that was
    // opened last at the head of its group, and a chat that was never opened
    // stands where the time of its last message puts it.
    #[serde(default)]
    pub opened: i64,
    // The MCP servers the reader switched on for this chat, by their id. A
    // chat written before the badge existed carries none of them, and stands
    // with no server of the reader's own.
    #[serde(default)]
    pub mcp: Vec<String>,
    // Tokens used by the last answer.
    pub context_used: u64,
}

// The switch for keeping the thinking of the earlier turns stands on.
fn preserve_thinking_on() -> bool {
    true
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
            search_enabled: false,
            // The sandbox is on unless the user turns it off.
            sandbox_enabled: true,
            // What a command may reach beyond its own folder stands off, whatever
            // the sandbox itself does.
            sandbox_network: false,
            sandbox_pip: false,
            sandbox_fetch: false,
            // No MCP server of the reader's own stands in a new chat.
            mcp: Vec::new(),
            thinking_mode: String::from("auto"),
            preserve_thinking: true,
            pinned: false,
            archived: false,
            opened: 0,
            context_used: 0,
        }
    }
}

// One attachment hanging on a message. The bytes of the attachment stand in
// a record of their own under the attachments folder of the data directory.
// The message keeps the way to it, the whole file name with its ending, and
// what kind of file it is, so a window can draw a block for the attachment
// without carrying the file about.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    // The whole file name, ending included, as the reader knew it.
    pub name: String,
    // The kind of the file, as the page named it. Empty when the page could
    // not tell.
    #[serde(default)]
    pub mime: String,
    // True when the attachment is a picture. The window draws a picture
    // block, and the program sends it to the model as a picture.
    #[serde(default)]
    pub image: bool,
}

// One message. The role is "You", "Assistant", "Tool", "Note", "Chapter" or
// "Summary". A chapter is a mark the model made of where a part of the talk
// begins: it holds its place among the messages below, and is never drawn as a
// message. A summary is the handoff summary of a compaction: it holds its place
// too, it is drawn as the heading of the summarisation, and it is the mark past
// which the words of the talk are left out of what goes back to the model.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub body: String,
    // The files the reader hung on the message, in the order the reader hung
    // them. A message of the reader carries the files it sent. A message of a
    // tool run carries the pictures that run opened, which is how a picture is
    // shown to the model again when the talk is asked again.
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    // The model's reasoning, streamed before the answer. Empty for most models.
    #[serde(default)]
    pub reasoning: String,
    // How long the reasoning above took, in milliseconds. Zero when there was
    // none, or when the chat was written before this was measured.
    #[serde(default)]
    pub reasoning_ms: u64,
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
    // True of a tool call the room gave out under, before the model had written
    // the whole of it. Such a call never ran, and its words are not whole, so
    // they go back to the model as the words of a turn and never as a call: a
    // server that read the broken words as arguments would fault over them.
    #[serde(default)]
    pub cancelled: bool,
    // True of a tool call the reader stopped with their own hand while the model
    // was still writing it. It never ran either, and its words go back the same
    // way. The two are told apart, for a call the room gave out under had its
    // room made after it by a handoff summary, and a call the reader stopped had
    // nothing after it at all.
    #[serde(default)]
    pub reader_stopped: bool,
    // True of an answer of the model that never came to an end of its own,
    // because the reader stopped it. It is not the same thing as
    // `reader_stopped`, which stands for a call that was cancelled: this says
    // that the words of the model stopped in the middle. The mark stands on the
    // newest answer only, so the page may put the mark of going on with it under
    // that answer and nowhere else.
    #[serde(default)]
    pub stopped: bool,
    // A message of the role "Summary" only. The room the talk took when the
    // handoff summary was asked for, in tokens.
    #[serde(default)]
    pub context_before: u64,
    // A message of the role "Summary" only. The room the handoff summary itself
    // takes, in tokens.
    #[serde(default)]
    pub context_after: u64,
}

// A chat with its messages.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Chat {
    pub meta: ChatMeta,
    #[serde(default)]
    pub messages: Vec<Message>,
}

// The first line of the newest message, in a short form. The carousel uses it
// for the soft picture of a chat at the side of the window.
pub fn preview(messages: &[Message]) -> String {
    // A mark of a chapter is not a word of the talk, so it says nothing about
    // what the chat is about either. A handoff summary is a whole standing text
    // of its own, and its first line would fill the soft picture on its own.
    // The newest message that was really said is the one to show.
    let last = match messages
        .iter()
        .rev()
        .find(|m| m.role != "Chapter" && m.role != "Summary")
    {
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
        }
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

    // True when the newest summarisation of this chat was asked for and never
    // came back. The mark of it stands in the talk with nothing in it, which is
    // what a summary the server never finished leaves behind. The reader is
    // asked to start it again before the chat goes on.
    //
    // Only the newest mark is looked at. An older mark that was written later
    // must not hold the chat: a chat whose last summarisation came off has room
    // to be used again, and a chat that waits on a mark of long ago could never
    // be sent to at all.
    pub fn handoff_owed(&self) -> bool {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == "Summary")
            .map(|m| m.body.trim().is_empty())
            .unwrap_or(false)
    }

    // The marks of every attachment the messages of this chat carry. The
    // records of them stand in the folder of the chat.
    pub fn attachment_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        for message in &self.messages {
            for held in &message.attachments {
                if !ids.contains(&held.id) {
                    ids.push(held.id.clone());
                }
            }
        }
        ids
    }
}


// Reads and writes the data on disk.
pub struct Store {
    repos_dir: PathBuf,
    // The folder that holds the chats of the repositories. It stands away from
    // them, so a command working inside a repository cannot reach a talk.
    chats_root: PathBuf,
    // The folder that holds the record of every repository. It stands away from
    // them for the same reason the talks do: the record of a repository is not
    // the business of the model that works inside it.
    records_root: PathBuf,
}

impl Store {
    pub fn new(repos_dir: PathBuf) -> Store {
        let places = config::paths();
        Store::with_rooms(repos_dir, places.repo_chats, places.repo_records)
    }

    // A store that keeps the talks of its repositories, and their records, in
    // folders of its own. Only the tests and `new` need this: the two folders
    // come from the places of the program.
    fn with_rooms(repos_dir: PathBuf, chats_root: PathBuf, records_root: PathBuf) -> Store {
        let _ = fs::create_dir_all(&repos_dir);
        Store { repos_dir, chats_root, records_root }
    }

    // Move the chats of every repository out of the directory of that
    // repository, into the folder of the talks under the data directory. This
    // runs once, when the program starts, and again when the reader points the
    // program at another folder of repositories. See the head of this file.
    pub fn migrate_chats_out(&self) {
        migrate_repo_chats(&self.repos_dir, &self.chats_root);
    }

    // Move the record of every repository out of the directory of that
    // repository, into the folder of the records under the data directory. The
    // record used to stand in the directory, where a command of the terminal
    // tool could read it and write over it. This runs once, when the program
    // starts, and again when the reader points the program at another folder of
    // repositories.
    pub fn migrate_records_out(&self) {
        migrate_repo_records(&self.repos_dir, &self.records_root);
    }

    // Where the record of one repository stands. A repository with no usable id
    // has no record of its own.
    fn record_of(&self, repo: &Repo) -> Option<PathBuf> {
        repo_record_path(&self.records_root, &repo.id)
    }

    // Directory that holds the chats of a repository, or of the loose chats.
    // The chats of a repository stand outside it, for the folder of a repository
    // is the room a terminal command works in. See the head of this file.
    fn chats_dir(&self, repo_id: &str) -> Option<PathBuf> {
        if repo_id.is_empty() {
            return Some(config::paths().chats);
        }
        repo_chats_dir(&self.chats_root, repo_id)
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
            // The record of a repository stands in the folder of the records,
            // named for the id the name of the folder carries.
            let id = name
                .strip_prefix("repository-")
                .map(|t| t.to_string())
                .or_else(|| id_from_dir_name(&name));
            let mut repo = match id
                .as_deref()
                .and_then(|i| repo_record_path(&self.records_root, i))
                .and_then(|p| fs::read_to_string(p).ok())
            {
                Some(text) => serde_json::from_str::<Repo>(&text).unwrap_or_default(),
                None => Repo::default(),
            };
            // A directory without a usable record is still shown, so the user
            // sees what is on disk.
            if repo.id.is_empty() {
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
        out.sort_by_key(|r| std::cmp::Reverse(r.last_used));
        out
    }

    // Create a repository directory and its record. The record does not stand in
    // the directory, for that directory is the room a command of the terminal
    // tool works in. See the head of this file.
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
            // A new repository is not pinned. The reader pins it in the side
            // bar when they want it above the others.
            pinned: false,
            path: path.clone(),
        };
        if path.is_dir() {
            return Err("A repository with that name already exists".to_string());
        }
        let record = self
            .record_of(&repo)
            .ok_or_else(|| String::from("The name of that repository cannot be kept"))?;
        if record.is_file() {
            return Err("A repository with that name already exists".to_string());
        }
        fs::create_dir_all(&path)
            .map_err(|e| format!("Cannot create {}: {}", path.display(), e))?;
        // No `chats` folder here. The chats of a repository stand outside it, so
        // a command working in the repository cannot come at them. See the head
        // of this file.
        let text = serde_json::to_string_pretty(&repo).unwrap_or_else(|_| String::from("{}"));
        fs::write(&record, text).map_err(|e| e.to_string())?;
        Ok(repo)
    }

    // Store the time a repository was last used.
    pub fn touch_repo(&self, repo: &mut Repo) {
        repo.last_used = now_secs();
        self.save_repo(repo);
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
        out.sort_by_key(|c| std::cmp::Reverse(c.meta.updated));
        out
    }

    // Write a chat. Parent directories are created when needed.
    pub fn save_chat(&self, chat: &Chat) {
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

    // Take one chat away for good.
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

    // Read one chat of any place by its id: the loose chats first, then the
    // chats of every repository. The side bar works on a chat of a place that
    // is not the place that is open, so the id is all the caller has.
    pub fn load_chat(&self, id: &str) -> Option<Chat> {
        if id.is_empty() {
            return None;
        }
        let wanted = format!("{id}.json");
        let mut places: Vec<PathBuf> = Vec::new();
        if let Ok(entries) = fs::read_dir(config::paths().chats) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.file_name().map(|n| n.to_string_lossy() == wanted) == Some(true) {
                    places.push(path);
                }
            }
        }
        if let Ok(entries) = fs::read_dir(&self.chats_root) {
            for entry in entries.flatten() {
                let path = entry.path().join(&wanted);
                if path.is_file() {
                    places.push(path);
                }
            }
        }
        for path in places {
            if let Ok(text) = fs::read_to_string(&path) {
                if let Ok(chat) = serde_json::from_str::<Chat>(&text) {
                    if chat.meta.id == id {
                        return Some(chat);
                    }
                }
            }
        }
        None
    }

    // Write the record of a repository back to the folder of the records. The
    // path of the directory is not kept in the record, and a record of a
    // repository the program did not make is never written: a folder of the
    // reader's own, put into the folder of repositories, is left as it stands.
    pub fn save_repo(&self, repo: &Repo) {
        let Some(record) = self.record_of(repo) else { return };
        if record.is_file() {
            let text = serde_json::to_string_pretty(repo).unwrap_or_default();
            let _ = fs::write(record, text);
        }
    }

    // Give a repository another name. The folder of a repository is named after
    // it, `name_id`, so the folder is moved as well and keeps the record, the
    // Python packages of the chats and the records of the attachments with it.
    // The id does not move, which is what the talks of the repository and the
    // chats that point at it are keyed on.
    pub fn rename_repo(&self, id: &str, raw_name: &str) -> Result<Repo, String> {
        let name = raw_name.trim().to_string();
        if name.is_empty() {
            return Err(String::from("A repository needs a name"));
        }
        let path = self
            .repo_path(id)
            .ok_or_else(|| String::from("That repository is not there"))?;
        let dir_name = repo_dir_name(&name, id);
        let newer = match path.parent() {
            Some(parent) => parent.join(&dir_name),
            None => return Err(String::from("The folder of that repository cannot be read")),
        };
        // The record stands outside the folder, so it is read from the folder of
        // the records and written back there: only the folder of the work moves.
        let record = repo_record_path(&self.records_root, id);
        let mut repo = match record.as_ref().and_then(|p| fs::read_to_string(p).ok()) {
            Some(text) => serde_json::from_str::<Repo>(&text).unwrap_or_default(),
            None => Repo::default(),
        };
        repo.id = id.to_string();
        repo.name = name.clone();
        let text = serde_json::to_string_pretty(&repo).unwrap_or_default();
        if newer == path {
            // The name came to the same folder name, so nothing moves but the
            // words of the record.
            if let Some(record) = record {
                fs::write(record, text).map_err(|e| e.to_string())?;
            }
            repo.path = path;
            return Ok(repo);
        }
        if newer.exists() {
            return Err(format!("A repository named {} is there already", name));
        }
        fs::rename(&path, &newer).map_err(|e| format!("Cannot move that repository: {}", e))?;
        if let Some(record) = record {
            fs::write(record, text).map_err(|e| e.to_string())?;
        }
        repo.path = newer;
        Ok(repo)
    }
}

// An id of this program, or nothing at all. Only the characters an id is made of
// are let into a path, so an id out of a chat file of another machine cannot name
// a road out of the folder it is looked in.
fn clean_id(repo_id: &str) -> Option<String> {
    let safe: String = repo_id
        .chars()
        .filter(|c| c.is_ascii_digit() || ('a'..='f').contains(&c.to_ascii_lowercase()))
        .collect();
    if safe.is_empty() || safe.as_str() != repo_id {
        return None;
    }
    Some(safe)
}

// The folder of the record of one repository, outside every repository.
fn repo_record_path(root: &Path, repo_id: &str) -> Option<PathBuf> {
    clean_id(repo_id).map(|id| root.join(format!("{id}.json")))
}

// The folder of the chats of one repository, outside every repository. Only the
// characters an id of this program is made of are let into a path, so an id out
// of a chat file of another machine cannot name a road out of the folder of the
// talks. An id with nothing usable in it gives nothing at all.
fn repo_chats_dir(root: &Path, repo_id: &str) -> Option<PathBuf> {
    clean_id(repo_id).map(|id| root.join(id))
}

// The chats of a repository used to stand in the directory of that repository,
// at `<repository>/chats`. A command of the terminal tool works in that
// directory, so the model could read every talk of the repository it was working
// in, and write over it. The chats now stand under the data directory. This
// moves them, once, when the program starts: nothing of a chat is thrown away,
// for a chat whose id stands in both places keeps both copies, the newer under
// its own id and the older beside it with `.kept` at the end of its name, where
// no list of chats reads it.
pub fn migrate_repo_chats(repos_dir: &Path, chats_root: &Path) {
    let Ok(entries) = fs::read_dir(repos_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let older = path.join("chats");
        if !older.is_dir() {
            continue;
        }
        let Some(id) = repo_id_of(&path) else {
            continue;
        };
        let Some(newer) = repo_chats_dir(chats_root, &id) else { continue };
        let _ = fs::create_dir_all(&newer);
        let Ok(chats) = fs::read_dir(&older) else { continue };
        for chat in chats.flatten() {
            let from = chat.path();
            if from.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(name) = from.file_name().map(|n| n.to_string_lossy().to_string()) else {
                continue;
            };
            let stood = newer.join(&name);
            if !stood.exists() {
                if fs::rename(&from, &stood).is_err() {
                    continue;
                }
                continue;
            }
            // The same chat stands in both places, so the newer of the two is
            // the one the program reads, and the older is laid beside it out of
            // the reach of the list of chats.
            let moved = newer.join(format!("{}.kept", name));
            if older_of(&stood, &from) {
                // The copy that already stood outside is the older one, so the
                // two change places before the older is laid aside.
                let held = newer.join(format!("{}.held", name));
                if fs::rename(&stood, &held).is_ok()
                    && fs::rename(&from, &stood).is_ok()
                    && fs::rename(&held, &moved).is_err()
                {
                    let _ = fs::rename(&stood, &held);
                }
            } else {
                let _ = fs::rename(&from, &moved);
            }
        }
        // The folder goes when nothing of it is left, so the repository holds no
        // mark of where its talks were kept.
        let _ = fs::remove_dir(&older);
    }
}

// True when the chat that stands at `older` was written before the one that
// stands at `newer`. A file that cannot be read as a chat is the older of the
// two, for a chat the program can read is worth more than one it cannot.
fn older_of(newer: &Path, older: &Path) -> bool {
    let read = |path: &Path| {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Chat>(&text).ok())
            .map(|chat| chat.meta.updated)
            .unwrap_or(i64::MIN)
    };
    read(newer) < read(older)
}

// The id of the repository that stands at this path, out of its record file or
// out of the name of its folder.
// The id of a repository, from the name of its folder. The record is not asked
// for, for the record stands outside the folder now, and the name of the folder
// is what the talks of a repository are keyed on.
fn repo_id_of(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().to_string();
    if let Some(tail) = name.strip_prefix("repository-") {
        return Some(tail.to_string());
    }
    id_from_dir_name(&name)
}

// The record of a repository used to stand in the directory of that repository,
// at `<repository>/repo.json`. A command of the terminal tool works in that
// directory, so the model could read the record of the repository it was working
// in, see the other repositories from it, and write over its own. The records
// now stand under the data directory. This moves them, once, when the program
// starts. A repository whose record stands in both places keeps the one under
// the data directory, which is the newer of the two, and the copy inside the
// directory is put away with it.
pub fn migrate_repo_records(repos_dir: &Path, records_root: &Path) {
    let Ok(entries) = fs::read_dir(repos_dir) else {
        return;
    };
    let _ = fs::create_dir_all(records_root);
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let older = path.join("repo.json");
        if !older.is_file() {
            continue;
        }
        let text = fs::read_to_string(&older).unwrap_or_default();
        // The id of the record itself comes before the id of the folder, for a
        // folder that was renamed while the record was not keeps the id the
        // talks are keyed on.
        let mut repo: Repo = serde_json::from_str(&text).unwrap_or_default();
        if repo.id.is_empty() {
            repo.id = match repo_id_of(&path) {
                Some(id) => id,
                None => {
                    // Nothing names this folder, so the record cannot be put
                    // anywhere. It is taken away rather than left where a
                    // command could come at it.
                    let _ = fs::remove_file(&older);
                    continue;
                }
            };
        }
        let Some(newer) = repo_record_path(records_root, &repo.id) else {
            let _ = fs::remove_file(&older);
            continue;
        };
        if !newer.is_file() {
            // The path of the directory is not kept in a record, and was never
            // written into one: what stands in the folder is what stands now.
            let written = serde_json::to_string_pretty(&repo).unwrap_or(text.clone());
            if fs::write(&newer, written).is_err() {
                continue;
            }
        }
        let _ = fs::remove_file(&older);
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

#[cfg(test)]
mod tests {
    use super::*;

    // The chats of a repository do not stand inside the directory of that
    // repository. A terminal command of a chat works in the folder of its
    // repository, so a chat kept there could be read and written over by the
    // model that is talking in it.
    #[test]
    fn the_chats_of_a_repository_stand_outside_it() {
        static TURN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let room = std::env::temp_dir().join(format!(
            "autumn-natter-chats-out-{}-{}",
            std::process::id(),
            TURN.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let repos = room.join("repositories");
        let talks = room.join("repo_chats");
        let records = room.join("repo_records");
        // A repository of the old shape: its directory holds a folder of chats.
        let repo = repos.join("notes_ab12cd34ef");
        fs::create_dir_all(repo.join("chats")).unwrap();
        fs::write(repo.join("repo.json"), r#"{"id":"ab12cd34ef","name":"notes"}"#).unwrap();
        let stood = repo.join("chats").join("99.json");
        fs::write(&stood, r#"{"meta":{"id":"99","title":"t","repo_id":"ab12cd34ef","model_id":"m","created":1,"updated":5,"code_enabled":false,"sandbox_enabled":true,"context_used":0},"messages":[]}"#).unwrap();
        // The work of the chat stays where it is: only the talks move.
        fs::write(repo.join("notes.md"), "work").unwrap();
        migrate_repo_chats(&repos, &talks);
        let moved = talks.join("ab12cd34ef").join("99.json");
        assert!(moved.is_file(), "the chat did not move out");
        assert!(!stood.exists(), "the chat is still inside the repository");
        assert!(!repo.join("chats").exists(), "the folder of chats is still there");
        assert!(repo.join("notes.md").is_file(), "the work of the chat stays");
        // The chat is read again from its new place, and nothing of it stands
        // under the repository any more.
        let store = Store::with_rooms(repos.clone(), talks.clone(), records.clone());
        let listed = store.list_chats("ab12cd34ef");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].meta.id, "99");
        let left: Vec<String> = fs::read_dir(&repo)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(left, vec![String::from("notes.md"), String::from("repo.json")], "{:?}", left);
        // A chat written now goes to the new place too.
        let mut chat = listed[0].clone();
        chat.meta.title = String::from("again");
        store.save_chat(&chat);
        assert!(talks.join("ab12cd34ef").join("99.json").is_file());
        assert_eq!(store.list_chats("ab12cd34ef")[0].meta.title, "again");
        let _ = fs::remove_dir_all(&room);
    }

    // The record of a repository does not stand inside the directory of that
    // repository either. The model that works in a repository can read the
    // folder it works in, and the record of it is not its business.
    #[test]
    fn the_record_of_a_repository_stands_outside_it() {
        static TURN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let room = std::env::temp_dir().join(format!(
            "autumn-natter-records-out-{}-{}",
            std::process::id(),
            TURN.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let repos = room.join("repositories");
        let records = room.join("repo_records");
        // A repository of the old shape: its directory holds its own record.
        let repo = repos.join("notes_ab12cd34ef");
        fs::create_dir_all(&repo).unwrap();
        fs::write(
            repo.join("repo.json"),
            r#"{"id":"ab12cd34ef","name":"notes","created":7,"last_used":9,"pinned":true}"#,
        )
        .unwrap();
        fs::write(repo.join("notes.md"), "work").unwrap();
        migrate_repo_records(&repos, &records);
        assert!(!repo.join("repo.json").exists(), "the record is still inside");
        assert!(repo.join("notes.md").is_file(), "the work of the chat stays");
        let kept = records.join("ab12cd34ef.json");
        assert!(kept.is_file(), "the record did not move out");
        // What the record said is not lost in the moving.
        let store = Store::with_rooms(repos.clone(), room.join("repo_chats"), records.clone());
        let listed = store.scan_repos();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "ab12cd34ef");
        assert_eq!(listed[0].name, "notes");
        assert!(listed[0].pinned, "the pin came across");
        assert_eq!(listed[0].last_used, 9);
        // A new repository is made with its record outside its directory, and a
        // rename leaves the record where it stands.
        let made = store.create_repo("The Ledger").unwrap();
        assert!(!made.path.join("repo.json").exists(), "a new record stood inside");
        assert!(records.join(format!("{}.json", made.id)).is_file());
        let renamed = store.rename_repo(&made.id, "The Ladder").unwrap();
        assert_eq!(renamed.path.file_name().unwrap().to_string_lossy(), "the-ladder_".to_string() + &made.id);
        assert!(records.join(format!("{}.json", made.id)).is_file());
        assert_eq!(
            store
                .scan_repos()
                .into_iter()
                .find(|r| r.id == made.id)
                .unwrap()
                .name,
            "The Ladder"
        );
        let _ = fs::remove_dir_all(&room);
    }

    // A chat id of another machine is not allowed to name a road out of the
    // folder of the talks.
    #[test]
    fn a_chat_id_names_no_road_outside_the_folder_of_talks() {
        let root = std::path::Path::new("/data/repo_chats");
        assert_eq!(
            repo_chats_dir(root, "ab12cd34ef"),
            Some(root.join("ab12cd34ef"))
        );
        for bad in ["../../etc", "", "...", "/x", "A B"] {
            assert!(repo_chats_dir(root, bad).is_none(), "{} was let through", bad);
        }
    }

    // The line of the day and the clock time reads as one moment: the day of the
    // week belongs to the day that stands beside it, whatever time zone this
    // machine works in, and the clock time is the clock time of that same day.
    #[test]
    fn the_line_of_the_day_names_the_day_of_the_week_it_stands_on() {
        assert_eq!(format_date_clock(0), "");
        // A few hours back, so the hour of the line stands away from the turn of
        // a day. Eight days are looked at, so a day of the week worked out apart
        // from the day of the month cannot agree with it by chance.
        let base = now_secs() - 5 * 3600;
        let mut earlier: Option<(usize, String)> = None;
        for day in 0..8 {
            let secs = base - day * 86400;
            let line = format_date_clock(secs);
            let parts: Vec<&str> = line.split(", ").collect();
            assert_eq!(parts.len(), 3, "three parts, one line: {}", line);
            let weekday = WEEK
                .iter()
                .position(|name| *name == parts[0])
                .expect("a day of the week the line names");
            assert_eq!(parts[1].split('-').count(), 3, "the day of the month: {}", line);
            assert!(line.ends_with(&format_clock(secs)), "the clock of the day: {}", line);
            if let Some((was, was_day)) = earlier {
                // The day of the month never moves forwards while the clock goes
                // back, and the name of the week goes back one step with it.
                assert!(parts[1] <= was_day.as_str(), "the day ran backwards: {}", line);
                // One day the year holds 25 hours: the day the clocks go back, so
                // a step of 86400 seconds stands twice inside the same day. There
                // the name of the week stands still with the day beside it, and is
                // never allowed to name a day other than the one it stands on.
                if parts[1] == was_day {
                    assert_eq!(weekday, was, "one day named twice over: {}", line);
                } else {
                    assert_eq!(weekday, (was + 6) % 7, "the week of {}", line);
                }
            }
            earlier = Some((weekday, parts[1].to_string()));
        }
    }

    // A chat written before the switch for keeping the thinking of the earlier
    // turns existed reads with the thinking kept, which is what llama.cpp does
    // for a template that can keep it.
    #[test]
    fn an_older_chat_keeps_the_thinking_of_the_earlier_turns() {
        let older = r#"{"id":"a","title":"t","repo_id":"","model_id":"m","created":1,
            "updated":2,"code_enabled":false,"sandbox_enabled":true,
            "thinking_mode":"off","context_used":10}"#;
        let meta: ChatMeta = serde_json::from_str(older).expect("the older chat is read");
        assert!(meta.preserve_thinking);
        assert_eq!(meta.thinking_mode, "off");
        // A chat that says it dropped the thinking keeps saying so.
        let dropped = r#"{"id":"a","title":"t","repo_id":"","model_id":"m","created":1,
            "updated":2,"code_enabled":false,"sandbox_enabled":true,
            "thinking_mode":"off","preserve_thinking":false,"context_used":10}"#;
        let meta: ChatMeta = serde_json::from_str(dropped).expect("the chat is read");
        assert!(!meta.preserve_thinking);
    }

    // A message written before a handoff summary was kept reads with the two
    // numbers of the room at nought, and the marks of the summarisations are
    // never the soft picture of a chat.
    #[test]
    fn an_older_message_keeps_nothing_of_the_summarisation() {
        let older = r#"{"id":"a","role":"Assistant","body":"the words of the answer"}"#;
        let message: Message = serde_json::from_str(older).expect("an older message reads");
        assert_eq!(message.context_before, 0);
        assert_eq!(message.context_after, 0);
        // A call the room gave out under is a thing a chat learned of later, so a
        // message written before it is not one.
        assert!(!message.cancelled);
        // A call that was cut off keeps its mark when it is written and read
        // again, for whether the words of it go back as a call or as the words of
        // a turn rests on the mark alone.
        let cut = r#"{"id":"b","role":"Tool","body":"a file","tool_name":"edit_file","tool_args":"{\"path\": \"a","cancelled":true}"#;
        let cut: Message = serde_json::from_str(cut).expect("a cut-off call reads");
        assert!(cut.cancelled);
        assert_eq!(cut.tool_args, "{\"path\": \"a");
        let again = serde_json::to_string(&cut).expect("the cut-off call is written");
        let back: Message = serde_json::from_str(&again).expect("and read");
        assert!(back.cancelled);
        // Such a call was not stopped by the reader, and a chat written before the
        // two were told apart reads the same way: the room gave out under it.
        assert!(!back.reader_stopped);
        // A call the reader stopped keeps a mark of its own, so the words that go
        // back to the model tell which of the two hands stopped it.
        let stopped = r#"{"id":"c","role":"Tool","body":"a file","tool_name":"edit_file","cancelled":true,"reader_stopped":true}"#;
        let stopped: Message = serde_json::from_str(stopped).expect("a call the reader stopped reads");
        assert!(stopped.cancelled);
        assert!(stopped.reader_stopped);
        let again = serde_json::to_string(&stopped).expect("it is written");
        let back: Message = serde_json::from_str(&again).expect("and read again");
        assert!(back.reader_stopped);
        // A handoff summary holds the two numbers of the room, and is written
        // and read back whole.
        let summary = Message {
            id: "s1".to_string(),
            role: "Summary".to_string(),
            body: "the handoff summary of the talk".to_string(),
            context_before: 6964,
            context_after: 412,
            ..Default::default()
        };
        let chat = Chat {
            meta: ChatMeta {
                id: "c1".to_string(),
                ..Default::default()
            },
            messages: vec![
                Message {
                    id: "m1".to_string(),
                    role: "You".to_string(),
                    body: "the first prompt".to_string(),
                    ..Default::default()
                },
                summary,
            ],
        };
        let text = serde_json::to_string(&chat).expect("the chat is written");
        let back: Chat = serde_json::from_str(&text).expect("the chat is read");
        assert_eq!(back.messages[1].context_before, 6964);
        assert_eq!(back.messages[1].context_after, 412);
        // The soft picture of the chat is the first prompt, and never the first
        // line of a handoff summary.
        assert_eq!(preview(&back.messages), "the first prompt");
    }

    // A handoff summary that was asked for and never came back leaves the chat
    // waiting, and a chat with the summary in it is not waiting.
    #[test]
    fn a_chat_waits_while_its_handoff_summary_is_missing() {
        let mark = |body: &str| Message {
            id: "s1".to_string(),
            role: "Summary".to_string(),
            body: body.to_string(),
            ..Default::default()
        };
        let waiting = Chat {
            meta: ChatMeta {
                id: "c1".to_string(),
                ..Default::default()
            },
            messages: vec![mark("")],
        };
        assert!(waiting.handoff_owed());
        let written = Chat {
            messages: vec![mark("the handoff summary of the talk")],
            ..waiting
        };
        assert!(!written.handoff_owed());
    }

    // A summarisation that was never written holds the chat. One that was
    // written after it lets the chat go, for the newest mark is the mark the
    // chat waits on. A chat with an old mark of long ago behind it must still
    // take a message, or it would be shut for ever.
    #[test]
    fn an_older_unwritten_summarisation_does_not_hold_the_chat() {
        let mark = |id: &str, body: &str| Message {
            id: id.to_string(),
            role: "Summary".to_string(),
            body: body.to_string(),
            ..Default::default()
        };
        let chat = Chat {
            meta: ChatMeta {
                id: "c1".to_string(),
                ..Default::default()
            },
            messages: vec![
                mark("s1", ""),
                mark("s2", "the handoff summary of the talk, written the second time"),
            ],
        };
        assert!(!chat.handoff_owed());
        // The other way round the chat still waits, for the newest mark holds
        // nothing.
        let back = Chat {
            messages: vec![
                mark("s1", "a summary that came off"),
                mark("s2", ""),
            ],
            ..chat
        };
        assert!(back.handoff_owed());
    }

    // A chat written before the switch of the web search existed reads with the
    // switch off, which is what a new chat starts with as well.
    #[test]
    fn an_older_chat_leaves_the_web_search_off() {
        let older = r#"{"id":"a","title":"t","repo_id":"","model_id":"m","created":1,
            "updated":2,"code_enabled":true,"sandbox_enabled":true,
            "thinking_mode":"off","context_used":10}"#;
        let meta: ChatMeta = serde_json::from_str(older).expect("the older chat is read");
        assert!(!meta.search_enabled);
        assert!(meta.code_enabled);
        let turned_on = r#"{"id":"a","title":"t","repo_id":"","model_id":"m","created":1,
            "updated":2,"code_enabled":false,"search_enabled":true,"sandbox_enabled":true,
            "thinking_mode":"off","context_used":10}"#;
        let meta: ChatMeta = serde_json::from_str(turned_on).expect("the chat is read");
        assert!(meta.search_enabled);
    }
}
