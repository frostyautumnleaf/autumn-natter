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
    // Thinking mode: "auto", "off", "on", "low", "medium", "high", "xhigh".
    #[serde(default)]
    pub thinking_mode: String,
    // Keep the thinking of the turns before the last answer in the words the
    // model is sent. llama.cpp keeps it by default, so a chat written before
    // the switch existed reads as kept.
    #[serde(default = "preserve_thinking_on")]
    pub preserve_thinking: bool,
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
            thinking_mode: String::from("auto"),
            preserve_thinking: true,
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
    // True while a chat is only in the memory. The carousel shows such a chat
    // as a blank one, and drops it when the user turns away without writing.
    // It is never written to disk, so the files hold only real chats.
    #[serde(default, skip_serializing)]
    pub transient: bool,
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
        out.sort_by_key(|r| std::cmp::Reverse(r.last_used));
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
        fs::create_dir_all(&path)
            .map_err(|e| format!("cannot create {}: {}", path.display(), e))?;
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
        out.sort_by_key(|c| std::cmp::Reverse(c.meta.updated));
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

#[cfg(test)]
mod tests {
    use super::*;

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
                // One day back, so the name of the week goes back one too, and the
                // day of the month moves with it.
                assert_eq!(weekday, (was + 6) % 7, "the week of {}", line);
                assert_ne!(parts[1], was_day, "the day moved: {}", line);
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
            transient: false,
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
            transient: false,
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
            transient: false,
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
