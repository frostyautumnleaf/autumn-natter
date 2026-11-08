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
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
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
// algorithm. It avoids a date crate. The log of the program reads it when the
// machine has no local time to give, which is the case the clock above gives back
// as a count of seconds.
pub(crate) fn civil_from_days(days: i64) -> (i64, u32, u32) {
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
    // A repository the user pinned stands in the `Pinned` group of the side
    // bar, above the other groups of the list. A chat of a pinned repository is
    // not pinned with it: the user pins a chat on its own.
    #[serde(default)]
    pub pinned: bool,
    // Full path of the repository directory. It is not stored in repo.json.
    #[serde(skip, default)]
    pub path: PathBuf,
}

// Which of the tools of the program the model may reach in one chat. The badge
// of the tools is the switch over all of them: with the badge off, none of these
// matters, for nothing is offered at all. Each line of the drop-down of the badge
// holds one of these switches, and the name of a line is the name the page sends.
//
// A chat written before these switches existed carries none of them and reads
// with all of them on, which is what such a chat was offered.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSwitches {
    // A command of the terminal tool may run.
    #[serde(default = "switch_on")]
    pub commands: bool,
    // The tools that work on the files of the chat: to change a file, and to
    // make a new one.
    #[serde(default = "switch_on")]
    pub files: bool,
    // A picture of the chat may be looked at.
    #[serde(default = "switch_on")]
    pub picture: bool,
    // A file of the chat may be handed to the user in a download block.
    #[serde(default = "switch_on")]
    pub hand_out: bool,
    // The contents of a file of the chat may be shown to the user in the window
    // of the chat that looks at one file whole.
    #[serde(default = "switch_on")]
    pub display: bool,
    // The model may stop and ask the user a question.
    #[serde(default = "switch_on")]
    pub ask: bool,
    // The model may end an answer with up to three buttons, each one a next
    // step the user may take with a press.
    #[serde(default = "switch_on")]
    pub suggest: bool,
    // The model may make an infographic video: the script, the layout, and the
    // file of frames. The five tools of the suite stand or fall together.
    // Off at first, for a video costs a long run.
    #[serde(default)]
    pub infographics: bool,
    // The model may put the talk of the chat into a handoff summary on its own
    // word, or on the word of the user. The tool is nothing without a handoff
    // summary, so it stands only while the settings make the room of the talk
    // that way: with compaction off the line of the drop-down is hidden on the
    // page and the tool is treated as off, whatever stands here.
    #[serde(default = "switch_on")]
    pub summarise: bool,
}

// The switch of a tool that nothing has touched yet.
fn switch_on() -> bool {
    true
}

impl Default for ToolSwitches {
    fn default() -> Self {
        ToolSwitches {
            commands: true,
            files: true,
            picture: true,
            hand_out: true,
            display: true,
            ask: true,
            suggest: true,
            infographics: false,
            summarise: true,
        }
    }
}

impl ToolSwitches {
    // Turn one line of the drop-down on or off. The name is the name the page
    // sends; a name of another kind is answered with nothing done.
    pub fn set(&mut self, part: &str, on: bool) -> bool {
        match part {
            "commands" => self.commands = on,
            "files" => self.files = on,
            "picture" => self.picture = on,
            "hand_out" => self.hand_out = on,
            "display" => self.display = on,
            "ask" => self.ask = on,
            "suggest" => self.suggest = on,
            "infographics" => self.infographics = on,
            "summarise" => self.summarise = on,
            _ => return false,
        }
        true
    }

    // The switches as the page reads them, for the menu of the badge and the tip
    // over the badge.
    pub fn words(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("commands", self.commands),
            ("files", self.files),
            ("picture", self.picture),
            ("hand_out", self.hand_out),
            ("display", self.display),
            ("ask", self.ask),
            ("suggest", self.suggest),
            ("infographics", self.infographics),
            ("summarise", self.summarise),
        ]
    }

    // Whether any line of the drop-down stands on. With none of them on the
    // badge of the tools has nothing to let through, and so cannot stand on.
    pub fn any_on(&self) -> bool {
        self.words().iter().any(|(_, on)| *on)
    }

    // Whether any line of the drop-down stands on that the model could really
    // be offered. The line of the handoff summary counts only while the
    // settings make the room by a handoff summary, for with compaction off
    // nothing stands behind it: a badge that would rest on that line alone
    // opens a door over nothing, and is read as a fault.
    pub fn any_on_with(&self, compaction_on: bool) -> bool {
        self.words()
            .iter()
            .any(|(part, on)| *on && (compaction_on || *part != "summarise"))
    }

    // What one line of the drop-down stands on. A name of another kind reads as
    // on, for a tool nothing switches off is offered.
    pub fn value_of(&self, part: &str) -> bool {
        match part {
            "commands" => self.commands,
            "files" => self.files,
            "picture" => self.picture,
            "hand_out" => self.hand_out,
            "display" => self.display,
            "ask" => self.ask,
            "suggest" => self.suggest,
            "infographics" => self.infographics,
            "summarise" => self.summarise,
            _ => true,
        }
    }
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
    // its commands reach nothing beyond the working directory until the user
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
    // A chat the user pinned stands in the `Pinned` group of the side bar,
    // above the groups of the repositories.
    #[serde(default)]
    pub pinned: bool,
    // A chat the user put away. It stands in no group of the side bar, and is
    // read again from the list of the archived chats, where it can be brought
    // back.
    #[serde(default)]
    pub archived: bool,
    // The moment the chat was last opened. The side bar puts the chat that was
    // opened last at the head of its group, and a chat that was never opened
    // stands where the time of its last message puts it.
    #[serde(default)]
    pub opened: i64,
    // The MCP servers the user switched on for this chat, by their id. A
    // chat written before the badge existed carries none of them, and stands
    // with no server of the user's own.
    #[serde(default)]
    pub mcp: Vec<String>,
    // Which of the tools of the program the model may reach, now that the badge
    // of the tools holds a drop-down of them. A chat written before the
    // drop-down existed carries none of these and reads with all of them on.
    #[serde(default)]
    pub tools: ToolSwitches,
    // Planning mode. While it stands, the model plans the work and writes it to a
    // plan file, and does none of the work itself. A chat written before planning
    // mode existed carries nothing of it and reads with planning mode off.
    #[serde(default)]
    pub planning: bool,
    // True when this chat planned a piece of work and let the plan go. The heading
    // over the talk says `PLANNED WORK`, and the plan may be read from it.
    #[serde(default)]
    pub planned: bool,
    // The name of the plan file of this chat, in the planning folder. Empty when
    // the chat never planned.
    #[serde(default)]
    pub plan_file: String,
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
            // Every tool of the program stands offered until the user takes one
            // out of the drop-down of the badge.
            tools: ToolSwitches::default(),
            search_enabled: false,
            // The sandbox is on unless the user turns it off.
            sandbox_enabled: true,
            // What a command may reach beyond its own folder stands off, whatever
            // the sandbox itself does.
            sandbox_network: false,
            sandbox_pip: false,
            sandbox_fetch: false,
            // No MCP server of the user's own stands in a new chat.
            mcp: Vec::new(),
            thinking_mode: String::from("auto"),
            preserve_thinking: true,
            pinned: false,
            archived: false,
            opened: 0,
            // No chat of an older run planned anything.
            planning: false,
            planned: false,
            plan_file: String::new(),
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
    // The whole file name, ending included, as the user knew it.
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
    // The files the user hung on the message, in the order the user hung
    // them. A message of the user carries the files it sent. A message of a
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
    // The short of the work, as the corporate display shows it: the blocks that
    // stand inside the one block of an answer. Empty for a chat laid out the
    // plain way, and for a message of the user.
    #[serde(default)]
    pub work: Vec<crate::corporate::WorkBlock>,
    // The head the one block of the corporate display stood at when the answer
    // came to its end.
    #[serde(default)]
    pub work_head: String,
    // True of a tool call the room gave out under, before the model had written
    // the whole of it. Such a call never ran, and its words are not whole, so
    // they go back to the model as the words of a turn and never as a call: a
    // server that read the broken words as arguments would fault over them.
    #[serde(default)]
    pub cancelled: bool,
    // True of a tool call the user stopped with their own hand while the model
    // was still writing it. It never ran either, and its words go back the same
    // way. The two are told apart, for a call the room gave out under had its
    // room made after it by a handoff summary, and a call the user stopped had
    // nothing after it at all. The mark was named for the reader in the chats of
    // before, so that name is still read.
    #[serde(default, alias = "reader_stopped")]
    pub user_stopped: bool,
    // True of an answer of the model that never came to an end of its own,
    // because the user stopped it. It is not the same thing as
    // `user_stopped`, which stands for a call that was cancelled: this says
    // that the words of the model stopped in the middle. The mark stands on the
    // newest answer only, so the page may put the mark of going on with it under
    // that answer and nowhere else.
    #[serde(default)]
    pub stopped: bool,
    // The files a run of the download tool offered to the user. The blocks of
    // the offer stand at the end of the answer that made them, and never while
    // the model is still writing.
    #[serde(default)]
    pub downloads: Vec<crate::download::Gift>,
    // The files one run of the display tool put in the window of the chat. The
    // block of the show stands in the talk with one button in it, and the button
    // opens the window over these files again.
    #[serde(default)]
    pub displayed: Vec<crate::display::Shown>,
    // The next steps one run of the suggestion tool offered. The buttons stand
    // at the end of the answer that made the offer, and only once the model has
    // stopped writing: see `crate::suggest`.
    #[serde(default)]
    pub suggestions: Vec<crate::suggest::Suggestion>,
    // A message of the role "Summary" only. The room the talk took when the
    // handoff summary was asked for, in tokens.
    #[serde(default)]
    pub context_before: u64,
    // A message of the role "Summary" only. The room the handoff summary itself
    // takes, in tokens.
    #[serde(default)]
    pub context_after: u64,
    // A message of the role "Summary" only. True when the summary came from the
    // short prompt of the settings, which is the ask made again when the room ran
    // full under the first ask. The two are asked for with different words and
    // hold different amounts, so the user is told which of them stands in the
    // chat. A chat written before this was asked with the prompt of the settings.
    #[serde(default)]
    pub short: bool,
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
    // what a summary the server never finished leaves behind. The user is
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
    // runs once, when the program starts, and again when the user points the
    // program at another folder of repositories. See the head of this file.
    pub fn migrate_chats_out(&self) {
        migrate_repo_chats(&self.repos_dir, &self.chats_root);
    }

    // Move the record of every repository out of the directory of that
    // repository, into the folder of the records under the data directory. The
    // record used to stand in the directory, where a command of the terminal
    // tool could read it and write over it. This runs once, when the program
    // starts, and again when the user points the program at another folder of
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
            // A new repository is not pinned. The user pins it in the side
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
        put_in_place(&record, &text)?;
        Ok(repo)
    }

    // Store the time a repository was last used. The reason for a record that
    // could not be put down comes back, so the user of the click that asked for
    // it is told instead of left to find the repository without a name later on.
    pub fn touch_repo(&self, repo: &mut Repo) -> Result<(), String> {
        repo.last_used = now_secs();
        self.save_repo(repo)
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
                let read = fs::read_to_string(&path)
                    .ok()
                    .and_then(|text| serde_json::from_str::<Chat>(&text).ok());
                match read {
                    Some(chat) if !chat.meta.id.is_empty() => out.push(chat),
                    // A file of the folder of chats that does not read as a chat
                    // is a chat gone out of the side bar. It is said once in the
                    // log of the program, so that a chat which leaves the bar can
                    // be followed back to its file and to the moment it went.
                    _ => note_unreadable(&path),
                }
            }
        }
        out.sort_by_key(|c| std::cmp::Reverse(c.meta.updated));
        out
    }

    // Write a chat. Parent directories are created when needed, and the reason
    // for a write that could not be finished comes back.
    //
    // The words go into a second file beside the chat and that file is put in the
    // place of the chat with a rename, as `config::save` does. Before this the
    // file was cut short and written again in one go, so a write that could not be
    // finished -- the disk is full, or the program is stopped with the words half
    // written -- left a short file or no file where the chat stood, and the reason
    // was thrown away. The side bar reads the folder of the chats again on every
    // change and passes over a file it cannot read, so the chat of such a file went
    // out of the bar a moment after it was opened, which is the fault this mends.
    // A write that fails now leaves the chat that stands there untouched.
    //
    // The second file is named for the writer as well as for the chat, because a
    // name the writers share is a second way to lose the talk. With one name for
    // every writer, a second write of one chat cut the second file of the first
    // writer short between the moment that writer had filled it and the moment it
    // was moved; the first writer then moved an empty file into the place of the
    // chat, and the chat stood there with nothing in it. Opening a chat from the
    // side bar writes the whole of it again, so each chat the user opened in turn
    // was lost the same way, and each went out of the bar as it went.
    //
    // A chat with nothing in it is never put in the place of a chat that holds a
    // talk. The write above can be made to stand for the whole of a chat from a
    // chat the program had not read whole, and the talk of it would then go with
    // the words that were not there. The file that stands is therefore looked at
    // when the chat coming in holds no message, and a talk in it is kept.
    pub fn save_chat(&self, chat: &Chat) -> Result<(), String> {
        let dir = match self.chats_dir(&chat.meta.repo_id) {
            Some(d) => d,
            None => return Err(String::from("no folder of this place")),
        };
        if let Err(problem) = fs::create_dir_all(&dir) {
            return Err(format!("cannot make {}: {}", dir.display(), problem));
        }
        let path = dir.join(format!("{}.json", chat.meta.id));
        if chat.messages.is_empty() {
            // Only the chat that holds nothing costs a read of the file that
            // stands. Every other write goes straight to the second file.
            if let Some(held) = stood_talk(&path) {
                return Err(format!(
                    "a chat with nothing in it is not put over the {} messages that stand in {}",
                    held, path.display()
                ));
            }
        }
        let text = serde_json::to_string(&chat)
            .map_err(|e| format!("the chat cannot be written out: {}", e))?;
        put_in_place(&path, &text)
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
    // user's own, put into the folder of repositories, is left as it stands.
    //
    // The record goes through a second file, as the words of a chat do. The side
    // bar reads the name of a repository, and the mark of its pin, out of this
    // one file, so a record cut short took the group of chats that stand under
    // that name back to a folder nobody can name.
    pub fn save_repo(&self, repo: &Repo) -> Result<(), String> {
        let Some(record) = self.record_of(repo) else { return Ok(()) };
        if !record.is_file() {
            return Ok(());
        }
        let text = serde_json::to_string_pretty(repo).map_err(|e| {
            format!(
                "the record of that repository cannot be written out: {}",
                e
            )
        })?;
        put_in_place(&record, &text)
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
                put_in_place(&record, &text)?;
            }
            repo.path = path;
            return Ok(repo);
        }
        if newer.exists() {
            return Err(format!("A repository named {} is there already", name));
        }
        fs::rename(&path, &newer).map_err(|e| format!("Cannot move that repository: {}", e))?;
        if let Some(record) = record {
            put_in_place(&record, &text)?;
        }
        repo.path = newer;
        Ok(repo)
    }
}

// Put words into the place of a file through a second file beside it. The words
// go into a file of their own first, and that file is moved over the place with
// a rename, which is a step the machine cannot be caught half way through. A
// write that cannot be finished therefore leaves the file that stood there to be
// read again, and says why.
//
// The second file is named for the writer -- the number of this process and the
// number of this write -- so that no two writers of one place share a second
// file, and so that a write never meets a second file an earlier write left
// behind. A second file left behind because the program was stopped in the
// middle of a write does no harm: no list of chats reads a name that does not
// end in `json`.
fn put_in_place(path: &Path, text: &str) -> Result<(), String> {
    let held = held_path(path);
    if let Err(problem) = fs::write(&held, text) {
        let _ = fs::remove_file(&held);
        return Err(format!("cannot write {}: {}", path.display(), problem));
    }
    if let Err(problem) = fs::rename(&held, path) {
        let _ = fs::remove_file(&held);
        return Err(format!("cannot put {}: {}", path.display(), problem));
    }
    Ok(())
}

// The name of the second file a write goes through: the name of the file it is
// to come into, with the number of this process and a count that moves on at
// every write after it. Two writers of one place therefore stand in two second
// files, and the place itself only ever takes the whole of what a writer put
// into its own. `contexts` writes the short shape of a talk through it too.
pub(crate) fn held_path(path: &Path) -> PathBuf {
    static WRITES: AtomicUsize = AtomicUsize::new(1);
    let stood = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| String::from("file"));
    let here = std::process::id();
    let turn = WRITES.fetch_add(1, Ordering::Relaxed);
    path.with_file_name(format!("{stood}.{here}.{turn}.writing"))
}

// The number of messages that stand in the file of a chat, when the file reads
// as a chat. Nothing comes back for a file that is not there, or that does not
// read as a chat, for there is then no talk to keep.
fn stood_talk(path: &Path) -> Option<usize> {
    let text = fs::read_to_string(path).ok()?;
    let stood: Chat = serde_json::from_str(&text).ok()?;
    (!stood.messages.is_empty()).then_some(stood.messages.len())
}

// Say once in the log of the program that a file of the folder of chats does not
// read as a chat. The side bar reads that folder again on every change, so the
// same file would else be said over and over, and a line that says nothing new a
// thousand times says nothing at all.
fn note_unreadable(path: &Path) {
    static SAID: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let said = SAID.get_or_init(Default::default);
    let name = path.to_string_lossy().to_string();
    let kept = said.lock().unwrap_or_else(|gone| gone.into_inner());
    if kept.contains(&name) {
        return;
    }
    drop(kept);
    // A file of nothing at all is the shape a write that could not be finished
    // leaves behind, so its size stands in the line: it is the difference between
    // a chat of another machine and a chat this program lost.
    let bytes = fs::metadata(path).map(|one| one.len()).unwrap_or(0);
    crate::proglog::note(
        crate::proglog::STORE,
        &format!(
            "a file of the chats does not read as a chat: {} ({} bytes)",
            path.display(),
            bytes
        ),
    );
    let mut kept = said.lock().unwrap_or_else(|gone| gone.into_inner());
    kept.insert(name);
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
            if put_in_place(&newer, &written).is_err() {
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
        store.save_chat(&chat).expect("the chat is written");
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

    // A chat is put on the disk through a second file, which is then renamed into
    // the place of the chat. The talk that stands there is therefore never cut
    // short: a write that cannot be finished says so, and leaves the chat that was
    // there to be read again. This is looked at because the side bar reads the
    // folder of the chats afresh on every change and passes over a file it cannot
    // read, so a short file is a chat gone out of the bar -- which is how a chat
    // opened from the bar used to vanish from it a moment later, on a machine with
    // no room left on its disk.
    #[cfg(unix)]
    #[test]
    fn a_chat_that_cannot_be_written_keeps_the_file_it_had() {
        use std::os::unix::fs::PermissionsExt;
        static TURN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let room = std::env::temp_dir().join(format!(
            "autumn-natter-chat-write-{}-{}",
            std::process::id(),
            TURN.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let talks = room.join("repo_chats");
        let store = Store::with_rooms(
            room.join("repositories"),
            talks.clone(),
            room.join("repo_records"),
        );
        let mut chat = Chat::new("ab12cd34ef", "");
        chat.meta.title = String::from("Standing talk");
        chat.messages.push(Message {
            role: String::from("You"),
            body: String::from("A written question"),
            when: now_secs(),
            ..Default::default()
        });
        store.save_chat(&chat).expect("the chat is written the first time");
        let id = chat.meta.id.clone();
        let group = talks.join("ab12cd34ef");
        let file = group.join(format!("{}.json", id));
        assert!(file.is_file(), "the chat stands on the disk");

        // The folder of the talks will not take a new file, so the second write
        // cannot be finished.
        fs::set_permissions(&group, fs::Permissions::from_mode(0o500))
            .expect("the folder of the talks is shut to writing");
        chat.meta.title = String::from("A name that cannot be kept");
        let problem = store.save_chat(&chat).expect_err("the write is refused");
        fs::set_permissions(&group, fs::Permissions::from_mode(0o700))
            .expect("the folder of the talks is opened again");

        // The chat that stands there is the chat that stood there before, and it
        // still reads as a chat: the row of it stays in the side bar.
        assert!(problem.contains("cannot write"), "{}", problem);
        let again = store.load_chat(&id).expect("the chat is read again");
        assert_eq!(again.meta.title, "Standing talk");
        assert_eq!(store.list_chats("ab12cd34ef").len(), 1, "the chat is still listed");
        // Nothing of the second file is left among the chats of the folder.
        let left: Vec<String> = fs::read_dir(&group)
            .expect("the folder of the talks is read")
            .flatten()
            .map(|one| one.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(left, vec![format!("{}.json", id)], "{:?}", left);
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
        // Such a call was not stopped by the user, and a chat written before the
        // two were told apart reads the same way: the room gave out under it.
        assert!(!back.user_stopped);
        // A call the user stopped keeps a mark of its own, so the words that go
        // back to the model tell which of the two hands stopped it.
        let stopped = r#"{"id":"c","role":"Tool","body":"a file","tool_name":"edit_file","cancelled":true,"user_stopped":true}"#;
        let stopped: Message = serde_json::from_str(stopped).expect("a call the user stopped reads");
        assert!(stopped.cancelled);
        assert!(stopped.user_stopped);
        let again = serde_json::to_string(&stopped).expect("it is written");
        let back: Message = serde_json::from_str(&again).expect("and read again");
        assert!(back.user_stopped);
        // A chat written while the mark carried the other name is read the same
        // way, so nothing is lost of a call stopped by hand.
        let elder = r#"{"id":"c2","role":"Tool","body":"a file","tool_name":"edit_file","cancelled":true,"reader_stopped":true}"#;
        let elder: Message = serde_json::from_str(elder).expect("the old name reads");
        assert!(elder.user_stopped);
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

    // A chat with nothing in it is never put in the place of a chat that holds a
    // talk. Opening a chat from the side bar writes the whole of it again for the
    // sake of one mark -- the moment it was opened, which the order of a group is
    // read from -- so a record made from a chat the program had not read whole
    // would empty the talk of it, and the side bar would show a chat with nothing
    // in it where a talk stood.
    #[test]
    fn a_chat_with_nothing_in_it_never_takes_the_place_of_a_talk() {
        static TURN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let room = std::env::temp_dir().join(format!(
            "autumn-natter-empty-over-talk-{}-{}",
            std::process::id(),
            TURN.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let talks = room.join("repo_chats");
        let store = Store::with_rooms(
            room.join("repositories"),
            talks.clone(),
            room.join("repo_records"),
        );
        let mut chat = Chat::new("ab12cd34ef", "");
        chat.messages.push(Message {
            role: String::from("You"),
            body: String::from("A written question"),
            when: now_secs(),
            ..Default::default()
        });
        store.save_chat(&chat).expect("the chat is written");
        let id = chat.meta.id.clone();
        let file = talks.join("ab12cd34ef").join(format!("{}.json", id));
        assert!(file.is_file(), "the talk stands on the disk");

        // The same chat, with the talk taken out of it. The file that stands holds
        // a talk, so the chat of nothing is refused and the talk is kept.
        chat.messages.clear();
        let problem = store.save_chat(&chat).expect_err("a chat of nothing is refused");
        assert!(problem.contains("is not put over"), "{}", problem);
        let again = store.load_chat(&id).expect("the talk is read again");
        assert_eq!(again.messages.len(), 1, "the talk that stood is kept");
        assert_eq!(store.list_chats("ab12cd34ef").len(), 1, "the chat is still listed");

        // Where nothing stands, a chat of nothing is written as it was before:
        // the guard keeps a talk, and stops nothing else.
        let blank = Chat::new("ab12cd34ef", "");
        store.save_chat(&blank).expect("a new chat of nothing is written");
        assert!(talks.join("ab12cd34ef").join(format!("{}.json", blank.meta.id)).is_file());
        let _ = fs::remove_dir_all(&room);
    }

    // The record of a repository is put on the disk through a second file, as the
    // words of a chat are. The name of a repository and the mark of its pin stand
    // in that one file, and the chats of the repository stand under that name in
    // the side bar, so a record cut short is a group of chats that cannot be read.
    #[cfg(unix)]
    #[test]
    fn the_record_of_a_repository_keeps_the_record_it_had() {
        use std::os::unix::fs::PermissionsExt;
        static TURN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let room = std::env::temp_dir().join(format!(
            "autumn-natter-record-write-{}-{}",
            std::process::id(),
            TURN.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let records = room.join("repo_records");
        fs::create_dir_all(&records).expect("the folder of the records is made");
        let store = Store::with_rooms(
            room.join("repositories"),
            room.join("repo_chats"),
            records.clone(),
        );
        let repo = store.create_repo("Banach").expect("the repository is made");
        let file = records.join(format!("{}.json", repo.id));
        assert!(file.is_file(), "the record stands on the disk");

        // The folder of the records will not take a new file, so the second write
        // cannot be finished.
        fs::set_permissions(&records, fs::Permissions::from_mode(0o500))
            .expect("the folder of the records is shut to writing");
        let mut pinned = repo.clone();
        pinned.pinned = true;
        let problem = store.save_repo(&pinned).expect_err("the write is refused");
        fs::set_permissions(&records, fs::Permissions::from_mode(0o700))
            .expect("the folder of the records is opened again");

        // The record that stands is the record that stood, and it still reads as a
        // record: the group of the side bar keeps its name.
        assert!(problem.contains("cannot write"), "{}", problem);
        let stood: Repo =
            serde_json::from_str(&fs::read_to_string(&file).expect("the record is read"))
                .expect("the record reads as a record");
        assert_eq!(stood.name, "Banach");
        assert!(!stood.pinned, "the mark of the pin did not move");
        // Nothing of the second file is left among the records.
        let left: Vec<String> = fs::read_dir(&records)
            .expect("the folder of the records is read")
            .flatten()
            .map(|one| one.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(left, vec![format!("{}.json", repo.id)], "{:?}", left);
        let _ = fs::remove_dir_all(&room);
    }

    // The second file a write goes through is named for the writer, and not for
    // the place alone. The fault this keeps out stands between two calls, so it is
    // looked at here in the shape of it: with one second file for every writer, a
    // second writer cut the second file of the first short between the moment that
    // writer filled it and the moment it was moved, and the first writer then moved
    // a file of nothing into the place of the chat. Every chat a user opened from
    // the side bar went the same way in turn, because opening a chat writes the
    // whole of it again.
    #[test]
    fn the_second_file_of_a_write_is_named_for_its_writer() {
        static TURN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let room = std::env::temp_dir().join(format!(
            "autumn-natter-second-file-{}-{}",
            std::process::id(),
            TURN.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        fs::create_dir_all(&room).expect("the room is made");
        let path = room.join("chat.json");

        // The shape of the fault, with one second file for every writer.
        let shared = room.join("chat.json.writing");
        fs::write(&shared, "the talk of the first writer").expect("the second file is filled");
        fs::write(&shared, "").expect("the second file is cut short by the second writer");
        fs::rename(&shared, &path).expect("the first writer moves it into the place of the chat");
        assert_eq!(
            fs::read_to_string(&path).expect("the place is read"),
            "",
            "one second file shared by two writers loses the talk"
        );

        // Two writes of one place now stand in two second files, so the place of
        // the chat only ever takes the whole of what a writer put into its own.
        let first = held_path(&path);
        let second = held_path(&path);
        assert_ne!(first, second, "two writes of one place share no second file");
        assert_eq!(first.parent(), path.parent(), "the second file stands beside the place");
        put_in_place(&path, "the talk of the first writer").expect("the first write is put in");
        put_in_place(&path, "the talk of the second writer").expect("the second write is put in");
        assert_eq!(
            fs::read_to_string(&path).expect("the place is read"),
            "the talk of the second writer"
        );
        // Nothing of the second files is left in the room.
        let left: Vec<String> = fs::read_dir(&room)
            .expect("the room is read")
            .flatten()
            .map(|one| one.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(left, vec![String::from("chat.json")], "{:?}", left);
        let _ = fs::remove_dir_all(&room);
    }

    // The line of the handoff summary is a line of the drop-down like every
    // other one, and it stands on from the beginning. With compaction off it
    // counts for nothing: a chat whose only line stands on behind that line has
    // no tool the model could reach, and the badge of the tools does not rest
    // on it.
    #[test]
    fn the_line_of_the_handoff_summary_stands_with_the_other_lines() {
        let mut switches = ToolSwitches::default();
        assert!(switches.value_of("summarise"), "the line stands on at first");
        assert!(switches.words().iter().any(|(part, _)| *part == "summarise"));
        assert!(switches.set("summarise", false));
        assert!(!switches.value_of("summarise"));
        assert!(switches.any_on(), "the other lines stand on");
        assert!(switches.any_on_with(false), "the other lines stand on");
        // Every line but that of the handoff summary is taken away. The lines
        // are read from the switches themselves, so a line the program learns
        // later is taken out here as well rather than leaving this test to say a
        // thing that is no longer true of it. The badge stands on while the room
        // is made by a summary, and goes out when it is not.
        let lines: Vec<&'static str> = switches
            .words()
            .into_iter()
            .map(|(part, _)| part)
            .filter(|part| *part != "summarise")
            .collect();
        for part in lines.iter() {
            switches.set(part, false);
        }
        switches.set("summarise", true);
        assert!(switches.any_on_with(true), "the summary is a tool the model may reach");
        assert!(!switches.any_on_with(false), "with compaction off nothing stands behind the badge");
        switches.set("summarise", false);
        assert!(!switches.any_on_with(true), "no line stands on at all");
    }
}
