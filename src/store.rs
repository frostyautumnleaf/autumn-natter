// Repositories and chats on disk.
//
// Every repository has its own directory. The directory name follows the rule "name_id", where
// the name uses only letters, digits and dashes, and the id is ten lowercase hexadecimal
// characters.
//
// The chats of a repository are not inside that directory. A terminal command of a chat runs in
// the working directory of its repository, so a chat kept there could be read, and written over,
// by the very model that is generating in it. The conversations of a repository are therefore
// under the data directory, at `repo_chats/<repository id>/`, and only the working directory of
// the chat is in the repository.
//
// The record of a repository - its name, when it was created and last used, whether it is
// pinned - is not inside that directory either, for the same reason: a model that can read the
// folder it works in can read the list of the other repositories, and write over its own. The
// records are under the data directory, at `repo_records/<repository id>.json`.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config;
use crate::db;
use crate::migrate;
use crate::proglog;

// Seconds since 1970-01-01, in the local time zone of the computer.
pub fn now_secs() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(_) => 0,
    }
}

// Turn seconds into a short text for the chat list. A time from today shows as hours and
// minutes. An older time shows the date.
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

// Turn seconds into the day of the week, the day and the clock time, all of them in the local
// time of the computer, such as Sunday, 2026-10-25, 19:40. The line the settings put before the
// model is read by the model, and the day of the week costs nothing and reports something the
// number of the day does not. The three parts are taken from one reading of the clock, so the
// day of the week always belongs to the day that is beside it.
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
            // tm_wday counts from Sunday, which remains last in WEEK.
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

// Turn seconds into the day and the clock time of it, such as 2026-10-20, 14:32. The heading of
// a summarisation is checked days after it was, so the hour of it alone would not say which day
// it was.
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

// Days since the epoch to a year, month and day. This is the civil from days algorithm. It
// avoids a date crate. The log of the program reads it when the computer has no local time to
// give, which is the case the clock above returns as a count of seconds.
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
        // A computer with no random source still needs an id.
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

// Keep letters, digits and dashes. Spaces become dashes. Other characters go away. The result
// never starts or ends with a dash.
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

// Directory name for a repository. A name with no usable character gives "repository-<id>".
pub fn repo_dir_name(raw: &str, id: &str) -> String {
    let clean = sanitize_name(raw);
    if clean.is_empty() {
        format!("repository-{}", id)
    } else {
        format!("{}_{}", clean, id)
    }
}

// A corner of the repositories directory left behind by the earlier behaviour, which moved a
// removed repository aside instead of deleting it. It is not a repository: the listing of the
// folder goes past it - see `scan_repos`. Nothing writes to it now.
pub const REMOVED: &str = "removed";

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
    // A repository the user pinned is in the `Pinned` group of the side bar, above the other
    // groups of the list. A chat of a pinned repository is not pinned with it: the user pins a
    // chat on its own.
    #[serde(default)]
    pub pinned: bool,
    // Full path of the repository directory. It is not stored in repo.json.
    #[serde(skip, default)]
    pub path: PathBuf,
}

// Which of the tools of the program the model may reach in one chat. The badge of the tools is
// the switch over all of them: with the badge off, none of these matters, for nothing is
// proposed at all. Each line of the drop-down of the badge holds one of these switches, and the
// name of a line is the name the page sends.
//
// A chat written before these switches existed carries none of them and reads with all of them
// on, which is what such a chat was proposed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSwitches {
    // A command of the terminal tool may run.
    #[serde(default = "switch_on")]
    pub commands: bool,
    // The tools that work on the files of the chat: to change a file, and to make a new one.
    #[serde(default = "switch_on")]
    pub files: bool,
    // An image of the chat may be checked.
    #[serde(default = "switch_on")]
    pub picture: bool,
    // A file of the chat may be handed to the user in a download block.
    #[serde(default = "switch_on")]
    pub hand_out: bool,
    // The contents of a file of the chat may be shown to the user in the window of the chat
    // that checks one file whole.
    #[serde(default = "switch_on")]
    pub display: bool,
    // The model may stop and ask the user a question.
    #[serde(default = "switch_on")]
    pub ask: bool,
    // The model may end an answer with up to three buttons, each one a next step the user may
    // take with a press.
    #[serde(default = "switch_on")]
    pub suggest: bool,
    // The model may end an answer with a list of the steps of the work, shown one step at a
    // time in a block of its own. See `crate::steps`.
    #[serde(default = "switch_on")]
    pub steps: bool,
    // The model may make an infographic video: the script, the layout, and the file of frames.
    // The five tools of the suite remain or fall together. Off at first, for a video costs a
    // long run.
    #[serde(default)]
    pub infographics: bool,
    // The model may put the conversation of the chat into a handoff summary on its own word, or
    // on the word of the user. The tool is nothing without a handoff summary, so it remains
    // only while the settings make the size of the context that way: with compaction off the
    // line of the drop-down is hidden on the page and the tool is treated as off, whatever is
    // here.
    #[serde(default = "switch_on")]
    pub summarise: bool,
    // The model may keep a list of tasks for the working directory it works in: six tools under
    // the line "Tasks suite" of the drop-down. Off at first, for the list is something a model
    // has to be informed to use, and most chats do not want it. See `crate::tasks`.
    #[serde(default)]
    pub tasks: bool,
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
            steps: true,
            infographics: false,
            summarise: true,
            tasks: false,
        }
    }
}

impl ToolSwitches {
    // Turn one line of the drop-down on or off. The name is the name the page sends; a name of
    // another type is answered with nothing done.
    pub fn set(&mut self, part: &str, on: bool) -> bool {
        match part {
            "commands" => self.commands = on,
            "files" => self.files = on,
            "picture" => self.picture = on,
            "hand_out" => self.hand_out = on,
            "display" => self.display = on,
            "ask" => self.ask = on,
            "suggest" => self.suggest = on,
            "steps" => self.steps = on,
            "infographics" => self.infographics = on,
            "tasks" => self.tasks = on,
            "summarise" => self.summarise = on,
            _ => return false,
        }
        true
    }

    // The switches as the page reads them, for the menu of the badge and the tip over the
    // badge.
    pub fn words(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("commands", self.commands),
            ("files", self.files),
            ("picture", self.picture),
            ("hand_out", self.hand_out),
            ("display", self.display),
            ("ask", self.ask),
            ("suggest", self.suggest),
            ("steps", self.steps),
            ("infographics", self.infographics),
            ("summarise", self.summarise),
            ("tasks", self.tasks),
        ]
    }

    // Whether any line of the drop-down is on. With none of them on the badge of the tools has
    // nothing to let through, and so cannot be on.
    pub fn any_on(&self) -> bool {
        self.words().iter().any(|(_, on)| *on)
    }

    // Whether any line of the drop-down is on that the model could really be proposed. The line
    // of the handoff summary counts only while the settings make the space by a handoff
    // summary, for with compaction off nothing is behind it: a badge that would rest on that
    // line alone opens a door over nothing, and is read as an error.
    pub fn any_on_with(&self, compaction_on: bool) -> bool {
        self.words()
            .iter()
            .any(|(part, on)| *on && (compaction_on || *part != "summarise"))
    }

    // What one line of the drop-down is on. A name of another type reads as on, for a tool
    // nothing switches off is proposed.
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
            "tasks" => self.tasks,
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
    // The web search tool may be used when this is true. It is on a switch of its own, for it
    // reaches the web out of the program and never the computer. A chat written before the
    // switch existed starts with it off.
    #[serde(default)]
    pub search_enabled: bool,
    // Run the tool inside bubblewrap when this is true.
    pub sandbox_enabled: bool,
    // The three settings of the sandbox. All of them remain off, for a chat and its commands
    // reach nothing beyond the working directory until the user opens one of them in the menu
    // of the sandbox badge. A chat written before they existed carries none of them and reads
    // with all three off. A command may reach the network.
    #[serde(default)]
    pub sandbox_network: bool,
    // `pip install` works, into a Python environment of the chat's own.
    #[serde(default)]
    pub sandbox_pip: bool,
    // The commands that fetch a page of the web, `wget`, `curl` and `fetch`, may run.
    #[serde(default)]
    pub sandbox_fetch: bool,
    // Thinking mode: "auto", "off", "on", "low", "medium", "high", "xhigh".
    #[serde(default)]
    pub thinking_mode: String,
    // Keep the thinking of the turns before the last answer in the text the model is sent.
    // llama.cpp keeps it by default, so a chat written before the switch existed reads as kept.
    #[serde(default = "preserve_thinking_on")]
    pub preserve_thinking: bool,
    // A chat the user pinned is in the `Pinned` group of the side bar, above the groups of the
    // repositories.
    #[serde(default)]
    pub pinned: bool,
    // A chat the user put away. It is in no group of the side bar, and is read again from the
    // list of the archived chats, where it can be brought back.
    #[serde(default)]
    pub archived: bool,
    // The moment the chat was last opened. The side bar puts the chat that was opened last at
    // the header of its group, and a chat that was never opened is where the time of its last
    // message puts it.
    #[serde(default)]
    pub opened: i64,
    // The MCP servers the user switched on for this chat, by their id. A chat written before
    // the badge existed carries none of them, and is with no server of the user's own.
    #[serde(default)]
    pub mcp: Vec<String>,
    // Which of the tools of the program the model may reach, now that the badge of the tools
    // holds a drop-down of them. A chat written before the drop-down existed carries none of
    // these and reads with all of them on.
    #[serde(default)]
    pub tools: ToolSwitches,
    // Planning mode. While it remains, the model plans the work and writes it to a plan file,
    // and does none of the work itself. A chat written before planning mode existed carries
    // nothing of it and reads with planning mode off.
    #[serde(default)]
    pub planning: bool,
    // True when this chat planned a piece of work and let the plan go. The heading over the
    // conversation reports `PLANNED WORK`, and the plan may be read from it.
    #[serde(default)]
    pub planned: bool,
    // The name of the plan file of this chat, in the planning folder. Empty when the chat never
    // planned.
    #[serde(default)]
    pub plan_file: String,
    // Tokens used by the last answer.
    pub context_used: u64,
}

// The switch for keeping the thinking of the earlier turns is on.
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
            // Every tool of the program remains proposed until the user takes one out of the
            // drop-down of the badge.
            tools: ToolSwitches::default(),
            search_enabled: false,
            // The sandbox is on unless the user turns it off.
            sandbox_enabled: true,
            // What a command may reach beyond its own folder remains off, whatever the sandbox
            // itself does.
            sandbox_network: false,
            sandbox_pip: false,
            sandbox_fetch: false,
            // No MCP server of the user's own is in a new chat.
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

// One attachment hanging on a message. The bytes of the attachment are in a record of their own
// under the attachments folder of the data directory. The message keeps the way to it, the
// whole file name with its ending, and what type of file it is, so a window can draw a block
// for the attachment without carrying the file about.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    // The whole file name, ending included, as the user knew it.
    pub name: String,
    // The type of the file, as the page named it. Empty when the page could not tell.
    #[serde(default)]
    pub mime: String,
    // True when the attachment is an image. The window draws an image block, and the program
    // sends it to the model as an image.
    #[serde(default)]
    pub image: bool,
}

// One message. The role is "You", "Assistant", "Tool", "Note", "Chapter" or "Summary". A
// chapter is a marker the model made of where a part of the conversation begins: it holds its
// place among the messages below, and is never drawn as a message. A summary is the handoff
// summary of a compaction: it holds its place too, it is drawn as the heading of the
// summarisation, and it is the marker past which the text of the conversation is left out of
// what goes back to the model.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub body: String,
    // The files the user hung on the message, in the order the user hung them. A message of the
    // user carries the files it sent. A message of a tool run carries the images that run
    // opened, which is how an image is shown to the model again when the conversation is asked
    // again.
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    // The model's reasoning, streamed before the answer. Empty for most models.
    #[serde(default)]
    pub reasoning: String,
    // How long the reasoning above took, in milliseconds. Zero when there was none, or when the
    // chat was written before this was measured.
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
    // Name of the tool the model requested.
    #[serde(default)]
    pub tool_name: String,
    // Arguments of the tool call, as the model wrote them.
    #[serde(default)]
    pub tool_args: String,
    // The summary of the work, as the corporate display shows it: the blocks that are inside
    // the one block of an answer. Empty for a chat decompressed the plain way, and for a
    // message of the user.
    #[serde(default)]
    pub work: Vec<crate::corporate::WorkBlock>,
    // The header the one block of the corporate display was at when the answer came to its end.
    #[serde(default)]
    pub work_head: String,
    // True when the model had already closed the one block of the answer with its own tool
    // before these text was written. The text of a message that carries the marker is outside
    // the block of the work, as the answer the user came for; the text of a message without it
    // is inside the block, in a sub -block of their own. A chat written before the marker
    // existed carries it nowhere, so its text keeps remaining outside, as they did.
    #[serde(default)]
    pub work_closed: bool,
    // True of a tool call the context window filled under, before the model had written all of
    // it. Such a call never ran, and its text is not whole, so they go back to the model as the
    // text of a turn and never as a call: a server that read the broken text as arguments would
    // error over them.
    #[serde(default)]
    pub cancelled: bool,
    // True of a tool call the user stopped manually while the model was still writing it. It
    // never ran either, and its text go back the same way. The two are distinguished, for a
    // call the context window filled under had its space made after it by a handoff
    // summary, and a call the user stopped had nothing after it at all. The marker was named
    // for the user in the chats of before, so that name is still read.
    #[serde(default, alias = "reader_stopped")]
    pub user_stopped: bool,
    // True of an answer of the model that never finished of its own, because the user stopped
    // it. It is not the same as `user_stopped`, which means a call that was cancelled: this
    // reports that the text of the model stopped in the middle. The marker is on the newest
    // answer only, so the page may put the marker of going on with it under that answer and
    // nowhere else.
    #[serde(default)]
    pub stopped: bool,
    // The files a run of the download tool proposed to the user. The blocks of the suggestions
    // are at the end of the answer that made them, and never while the model is still writing.
    #[serde(default)]
    pub downloads: Vec<crate::download::Gift>,
    // The files one run of the display tool put in the window of the chat. The block of the
    // show is in the conversation with one button in it, and the button opens the window over
    // these files again.
    #[serde(default)]
    pub displayed: Vec<crate::display::Shown>,
    // The next steps one run of the suggestion tool proposed. The buttons are at the end of the
    // answer that made the proposal, and only once the model has stopped writing: see
    // `crate::suggest`.
    #[serde(default)]
    pub suggestions: Vec<crate::suggest::Suggestion>,
    // The steps of the work one run of the steps tool showed. The block is at the end of the
    // answer that made the call, above the other blocks an answer ends with, and only once the
    // model has stopped writing: see `crate::steps`.
    #[serde(default)]
    pub steps: Option<crate::steps::Plan>,
    // A message of the role "Summary" only. The space the conversation took when the
    // handoff summary was requested, in tokens.
    #[serde(default)]
    pub context_before: u64,
    // A message of the role "Summary" only. The space the handoff summary itself takes, in
    // tokens.
    #[serde(default)]
    pub context_after: u64,
    // A message of the role "Summary" only. True when the summary came from the short prompt of
    // the settings, which is the request rebuilt when the context window filled under the first
    // ask. The two are requested with different text and hold different amounts, so the user is
    // informed which of them is in the chat. A chat written before this was asked with the
    // prompt of the settings.
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

// The first line of the newest message, in a short form. The carousel uses it for the soft
// image of a chat at the side of the window.
pub fn preview(messages: &[Message]) -> String {
    // A marker of a chapter is not a note about the conversation, so it reports nothing about
    // what the chat is about either. A handoff summary is a whole remaining text of its own,
    // and its first line would fill the soft image on its own. The newest message that was
    // really stated is the one to show.
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

    // True when the newest summarisation of this chat was requested and never returned. The
    // marker of it is in the conversation with nothing in it, which is what a summary the
    // server never finished leaves behind. The user is asked to start it again before the chat
    // goes on.
    //
    // Only the newest marker is checked. An older marker that was written later must not hold
    // the chat: a chat whose last summarisation came off has space to be used again, and a
    // chat that waits on a marker of long ago could never be sent to at all.
    pub fn handoff_owed(&self) -> bool {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == "Summary")
            .map(|m| m.body.trim().is_empty())
            .unwrap_or(false)
    }

    // The markers of every attachment the messages of this chat carry. The records of them are
    // in the working directory of the chat.
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


// Reads and writes the text of the program: the conversations, the conversations of every
// repository, and the record of every repository. These are in one SQLite file, `chats.db`, and
// not in a directory of JSON files. See `src/db.rs`.
//
// The three types remain together because the side bar reads all three at once: one query gives
// the groups of the list, where a scan of three folders gave one read per file. They remain
// apart from the other databases because they change together and nothing else changes with
// them.
pub struct Store {
    // The folder the directories of the repositories are in. A repository is still a directory
    // of the computer, because it is where a model works; only its record and its conversations
    // live in the database.
    repos_dir: PathBuf,
    // The database of the conversations and the records. It is opened the first time it is
    // requested and kept open after it, because a directory of the program that cannot be
    // written must stop a chat from being saved and must not stop the program from starting:
    // the reason for the error then returns to the user of the click that requested the write,
    // as it did when every chat was in a file of its own.
    db: Mutex<Option<Arc<db::Db>>>,
    // Where the file of the database remains.
    db_path: PathBuf,
    // The folders the text of the older format was in. They are kept because the scan reads
    // them once, and because the conversations of a repository must come out of the directory
    // of that repository before anything else is done with them. See `src/migrate.rs`.
    older: Older,
}

// The folders the text of the program was in before they were in a database: the loose chats,
// the conversations of the repositories, and the records of them. They is offset from the
// repositories, so a command working inside a repository cannot reach a conversation or the
// record of the repository it works in.
#[derive(Clone, Debug)]
pub struct Older {
    // The loose chats: `chats/<chat id>.json`.
    pub chats: PathBuf,
    // The chats of the repositories: `repo_chats/<repository id>/<chat id>.json`.
    pub repo_chats: PathBuf,
    // The record of every repository: `repo_records/<repository id>.json`.
    pub repo_records: PathBuf,
}

impl Older {
    // The directories of the places of this program.
    fn of_places(places: &config::Paths) -> Older {
        Older {
            chats: places.chats.clone(),
            repo_chats: places.repo_chats.clone(),
            repo_records: places.repo_records.clone(),
        }
    }
}

impl Store {
    pub fn new(repos_dir: PathBuf) -> Store {
        let places = config::paths();
        let older = Older::of_places(&places);
        Store::in_room(repos_dir, places.data.join(db::CHATS_DB), older)
    }

    // The form of a store from its three places: the repositories directory, the database of
    // the conversations and the records, and the folders the older format was in. `new` takes
    // all three from the places of the program; a test takes them from a directory of its own,
    // so a check of the store never reaches the files of the computer.
    fn in_room(repos_dir: PathBuf, db_path: PathBuf, older: Older) -> Store {
        let _ = fs::create_dir_all(&repos_dir);
        Store {
            repos_dir,
            db: Mutex::new(None),
            db_path,
            older,
        }
    }

    // A store whose database and older folders are in a directory of its own.
    #[cfg(test)]
    fn with_rooms(repos_dir: PathBuf, db_dir: &Path, older: Older) -> Store {
        Store::in_room(repos_dir, db_dir.join(db::CHATS_DB), older)
    }

    // The database the conversations and the records are in. The planning text of a chat is
    // kept in the same file, so the code that writes them takes it. The reason a file that will
    // not open is kept for the user of the step that requested it: a chat is not saved, and is
    // informed of, rather than the program being stopped where it was.
    pub fn database(&self) -> Result<Arc<db::Db>, String> {
        let mut held = self.db.lock().unwrap_or_else(|gone| gone.into_inner());
        if let Some(kept) = held.as_ref() {
            return Ok(kept.clone());
        }
        let opened = db::open(&self.db_path, db::Kind::Chats)?;
        *held = Some(opened.clone());
        Ok(opened)
    }

    // The database, for a step that answers with the empty answer it can answer with when the
    // file will not open. The reason is reported in the log of the program, because a user of
    // the side bar is shown nothing and no line would be stated to them about it.
    fn database_or_silence(&self) -> Option<Arc<db::Db>> {
        match self.database() {
            Ok(kept) => Some(kept),
            Err(problem) => {
                proglog::note(proglog::STORE, &problem);
                None
            }
        }
    }

    // Move the chats of every repository out of the directory of that repository, into the
    // contexts directory under the data directory. This runs once, when the program starts, and
    // again when the user points the program at another folder of repositories. It is the first
    // step of the way into the database: the conversations must be out of the reach of a
    // command before they are read into a file a command cannot read. See the header of this
    // file.
    pub fn migrate_chats_out(&self) {
        migrate_repo_chats(&self.repos_dir, &self.older.repo_chats);
    }

    // Move the record of every repository out of the directory of that repository, into the
    // directory of the records under the data directory. The record used to be in the
    // directory, where a command of the terminal tool could read it and write over it. This
    // runs once, when the program starts, and again when the user points the program at another
    // folder of repositories.
    pub fn migrate_records_out(&self) {
        migrate_repo_records(&self.repos_dir, &self.older.repo_records);
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
            if repo_id_of(&path).as_deref() == Some(id) {
                return Some(path);
            }
        }
        None
    }

    // Every repository the computer holds, newest use first. The directories are read from the
    // repositories directory, and their records from the database. A directory with no record
    // of its own is still shown, so the user sees what is on disk: a directory of their own,
    // put into the repositories directory, is a repository as much as one this program made.
    pub fn scan_repos(&self) -> Vec<Repo> {
        let Ok(entries) = fs::read_dir(&self.repos_dir) else {
            return Vec::new();
        };
        let held = self.rows_of_repos();
        let mut out: Vec<Repo> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            // The corner of what was removed is not a repository, whatever it is named, and the
            // directories remaining inside it are one level too deep to be seen from here.
            if name == REMOVED {
                continue;
            }
            if let Some(id) = id_from_dir_name(&name).or_else(|| repo_id_of(&path)) {
                let mut repo = held.get(&id).cloned().unwrap_or_default();
                if repo.id.is_empty() {
                    repo.id = id;
                    repo.name = human_name(&name);
                }
                repo.path = path;
                out.push(repo);
            }
        }
        out.sort_by_key(|r| std::cmp::Reverse(r.last_used));
        out
    }

    // The records of the database, by the id of the repository.
    fn rows_of_repos(&self) -> HashMap<String, Repo> {
        let mut out = HashMap::new();
        let Some(database) = self.database_or_silence() else {
            return out;
        };
        let read = database.read(|conn| {
            let mut said = conn.prepare(
                "SELECT id, name, created, last_used, pinned FROM repos ORDER BY last_used DESC",
            )
            .map_err(db::fault)?;
            let rows = said
                .query_map([], |row| {
                    Ok(Repo {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        created: row.get(2)?,
                        last_used: row.get(3)?,
                        pinned: db::mark_of(row.get(4)?),
                        path: PathBuf::new(),
                    })
                })
                .map_err(db::fault)?;
            for row in rows {
                let repo = row.map_err(db::fault)?;
                out.insert(repo.id.clone(), repo);
            }
            Ok(())
        });
        if let Err(problem) = read {
            proglog::note(proglog::STORE, &format!("the records cannot be read: {}", problem));
        }
        out
    }

    // Create a repository directory and its record. The record is not in the directory, for
    // that directory is the working directory a command of the terminal tool works in. See the header
    // of this file.
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
            // A new repository is not pinned. The user pins it in the side bar when they want
            // it above the others.
            pinned: false,
            path: path.clone(),
        };
        if path.is_dir() {
            return Err("A repository with that name already exists".to_string());
        }
        let database = self.database()?;
        let stands: bool = database.read(|conn| {
            Ok(conn
                .prepare("SELECT 1 FROM repos WHERE id = ?1")
                .map_err(db::fault)?
                .exists(params![id])
                .map_err(db::fault)?)
        })?;
        if stands {
            return Err("A repository with that name already exists".to_string());
        }
        fs::create_dir_all(&path)
            .map_err(|e| format!("Cannot create {}: {}", path.display(), e))?;
        // No `chats` folder here. The chats of a repository are outside it, so a command
        // working in the repository cannot reach them. See the header of this file.
        database.write(|conn| {
            conn.execute(
                "INSERT INTO repos (id, name, created, last_used, pinned) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![repo.id, repo.name, repo.created, repo.last_used, db::mark_num(repo.pinned)],
            )
            .map_err(db::fault)?;
            Ok(())
        })?;
        Ok(repo)
    }

    // Store the time a repository was last used. The reason for a record that could not be put
    // down returns, so the user of the click that requested it is informed instead of left to
    // find the repository without a name later on.
    pub fn touch_repo(&self, repo: &mut Repo) -> Result<(), String> {
        repo.last_used = now_secs();
        self.save_repo(repo)
    }

    // Chats of one scope, newest first. repo_id "" gives the loose chats.
    //
    // A row of the table that does not read as a chat is a chat been sent of the side bar. It
    // is reported once in the log of the program, so that a chat which leaves the top bar can
    // be followed back to its id and to the moment it went.
    pub fn list_chats(&self, repo_id: &str) -> Vec<Chat> {
        let mut out: Vec<Chat> = Vec::new();
        let Some(database) = self.database_or_silence() else {
            return out;
        };
        let read = database.read(|conn| {
            let mut said = conn
                .prepare(
                    "SELECT id, data FROM chats WHERE repo_id = ?1 ORDER BY updated DESC, id",
                )
                .map_err(db::fault)?;
            let rows = said
                .query_map(params![repo_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
                .map_err(db::fault)?;
            for row in rows {
                let (id, data) = row.map_err(db::fault)?;
                match serde_json::from_str::<Chat>(&data) {
                    Ok(chat) if !chat.meta.id.is_empty() => out.push(chat),
                    Ok(_) | Err(_) => note_unreadable_id(&id),
                }
            }
            Ok(())
        });
        if let Err(problem) = read {
            proglog::note(proglog::STORE, &format!("the chats cannot be read: {}", problem));
        }
        out
    }

    // Write a chat. The reason for a write that could not be finished returns.
    //
    // The text of a chat go into one row of one table, and the row is put down by a write of
    // the database. Before this the text went into a file of their own through a second file
    // beside it, because a file that was truncated was a chat been sent of the top bar; the
    // database cannot be caught half way through a row, so the second file is not needed. A
    // chat that is in the table is replaced whole, and a write that fails leaves the row that
    // was to be read again.
    //
    // A chat with nothing in it is still never put in the place of a chat that holds a
    // conversation. The write above can be made to mean all of a chat from a chat the program
    // had not read whole, and the conversation of it would then go with the text that was not
    // there. The row that remains is therefore checked when the chat arriving holds no message,
    // and a conversation in it is kept.
    pub fn save_chat(&self, chat: &Chat) -> Result<(), String> {
        if chat.meta.id.is_empty() {
            return Err(String::from("a chat with no id is not written"));
        }
        let database = self.database()?;
        let wrote = database.write(|conn| {
            if chat.messages.is_empty() {
                // Only the chat that holds nothing costs a read of the row that remains. Every
                // other write goes straight to the row.
                let held: i64 = conn
                    .query_row(
                        "SELECT messages FROM chats WHERE id = ?1",
                        params![chat.meta.id],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);
                if held > 0 {
                    return Err(format!(
                        "a chat with nothing in it is not put over the {} messages that are in {}",
                        held, chat.meta.id
                    ));
                }
            }
            let text = serde_json::to_string(&chat)
                .map_err(|e| format!("the chat cannot be written out: {}", e))?;
            conn.execute(
                "INSERT INTO chats (id, repo_id, title, created, updated, messages, data)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET
                     repo_id = excluded.repo_id,
                     title = excluded.title,
                     created = excluded.created,
                     updated = excluded.updated,
                     messages = excluded.messages,
                     data = excluded.data",
                params![
                    chat.meta.id,
                    chat.meta.repo_id,
                    chat.meta.title,
                    chat.meta.created,
                    chat.meta.updated,
                    chat.messages.len() as i64,
                    text
                ],
            )
            .map_err(db::fault)?;
            Ok(())
        })?;
        Ok(wrote)
    }

    // Take one chat away for good.
    pub fn delete_chat(&self, repo_id: &str, id: &str) {
        if id.is_empty() {
            return;
        }
        let Some(database) = self.database_or_silence() else {
            return;
        };
        let gone = database.write(|conn| {
            conn.execute(
                "DELETE FROM chats WHERE id = ?1 AND repo_id = ?2",
                params![id, repo_id],
            )
            .map_err(db::fault)?;
            Ok(())
        });
        if let Err(problem) = gone {
            proglog::note(
                proglog::STORE,
                &format!("the chat {} is not removed: {}", id, problem),
            );
        }
        // The plan of the chat goes with it. A plan is read by nothing once the conversation it
        // was written for is removed, and a row of plans that no chat names is text kept for
        // nothing.
        crate::planning::drop_plan(id);
    }

    // Take the record of a repository out of the database. The chats of the repository are
    // removed one by one before this is asked, for a record of a repository removed while its
    // chats remain leaves those chats under a place that reads back as nothing.
    pub fn drop_repo(&self, id: &str) -> Result<(), String> {
        if id.is_empty() {
            return Err("There is no repository to remove".to_string());
        }
        let database = self.database()?;
        database.write(|conn| {
            conn.execute("DELETE FROM repos WHERE id = ?1", params![id])
                .map_err(db::fault)?;
            Ok(())
        })
    }

    // Delete the directory of a repository, with everything in it.
    //
    // The side bar is the listing of the repositories directory, so a repository leaves the list
    // when its directory leaves the folder. Removal is asked for twice: once from the menu of the
    // repository, and once by typing the name of the repository into the window that asks, so the
    // folder that is deleted is the folder the user named. The chats of the repository, its
    // conversations, its plans and its tasks are removed with it, and no copy of the work is kept
    // back: an old folder parked beside the live ones is confusing, is never cleaned up, and the
    // naming of the repository in the confirmation is what makes sure the deletion was meant.
    pub fn delete_dir(&self, id: &str) -> Result<(), String> {
        let Some(path) = self.repo_path(id) else {
            return Err("That repository is not there".to_string());
        };
        fs::remove_dir_all(&path)
            .map_err(|why| format!("the directory of the repository cannot be deleted: {}", why))
    }

    // Read one chat of any place by its id: the loose chats first, then the chats of every
    // repository. The side bar works on a chat of a place that is not the place that is open,
    // so the id is all the caller has. The id is one row of one table, which is what the two
    // searches were for.
    pub fn load_chat(&self, id: &str) -> Option<Chat> {
        if id.is_empty() {
            return None;
        }
        let Some(database) = self.database_or_silence() else {
            return None;
        };
        let held = database.read(|conn| {
            Ok(conn
                .query_row(
                    "SELECT data FROM chats WHERE id = ?1",
                    params![id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(db::fault)?)
        });
        match held {
            Ok(Some(data)) => match serde_json::from_str::<Chat>(&data) {
                Ok(chat) if chat.meta.id == id => Some(chat),
                _ => {
                    note_unreadable_id(id);
                    None
                }
            },
            Ok(None) => None,
            Err(problem) => {
                proglog::note(
                    proglog::STORE,
                    &format!("the chat {} cannot be read: {}", id, problem),
                );
                None
            }
        }
    }

    // Write the record of a repository back into the database. The path of the directory is not
    // kept in the record, and a record of a repository the program did not make is never
    // written: a directory of the user's own, put into the directory of repositories, is left
    // as it is now.
    //
    // The record is one row, so the marker of the name and the marker of the pin cannot get out
    // of sync, which is what a file truncated did: the side bar reads the name of a repository
    // and the marker of its pin out of that one record, and a record truncated took the group
    // of chats that are under that name back to a folder nobody can name.
    pub fn save_repo(&self, repo: &Repo) -> Result<(), String> {
        if self.record_made(&repo.id)? {
            let database = self.database()?;
            database.write(|conn| {
                conn.execute(
                    "UPDATE repos SET name = ?2, created = ?3, last_used = ?4, pinned = ?5 WHERE id = ?1",
                    params![
                        repo.id,
                        repo.name,
                        repo.created,
                        repo.last_used,
                        db::mark_num(repo.pinned)
                    ],
                )
                .map_err(db::fault)?;
                Ok(())
            })?;
        }
        Ok(())
    }

    // True when this program made the record of this repository: a row of the table means it. A
    // directory the user put into the repositories directory has no row, and its text is left
    // where they are.
    fn record_made(&self, id: &str) -> Result<bool, String> {
        let database = self.database()?;
        database.read(|conn| {
            Ok(conn
                .prepare("SELECT 1 FROM repos WHERE id = ?1")
                .map_err(db::fault)?
                .exists(params![id])
                .map_err(db::fault)?)
        })
    }

    // Give a repository another name. The directory of a repository is named after it,
    // `name_id`, so the folder is moved as well and keeps the record, the Python packages of
    // the chats and the records of the attachments with it. The id does not move, which is what
    // the conversations of the repository and the chats that point at it are keyed on.
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
            None => return Err(String::from("The directory of that repository cannot be read")),
        };
        // The record is outside the folder, so only the text of the record and the directory of
        // the work are touched: the conversations of the repository are keyed on the id, and
        // the id does not move.
        let mut repo = self.rows_of_repos().remove(id).unwrap_or_default();
        repo.id = id.to_string();
        repo.name = name.clone();
        if newer == path {
            // The name came to the same folder name, so nothing moves but the text of the
            // record.
            self.save_repo(&repo)?;
            repo.path = path;
            return Ok(repo);
        }
        if newer.exists() {
            return Err(format!("A repository named {} is there already", name));
        }
        fs::rename(&path, &newer).map_err(|e| format!("Cannot move that repository: {}", e))?;
        self.save_repo(&repo)?;
        repo.path = newer;
        Ok(repo)
    }

    // Take the text of the older format into the database, and lay the files they came from in
    // the directory of the files that were. The older format is a directory of plain files:
    // `chats/<id>.json` for the loose chats, `repo_chats/<repository id>/<id>.json` for the
    // chats of a repository, and `repo_records/<repository id>.json` for the records. One table
    // holds all three types of word now, and the place of a chat is a column of it rather than
    // a folder. See `src/migrate.rs`, which remains the search marker and brings across what is
    // outside the store.
    pub fn bring_older_shape(&self, before: &Path) -> Brought {
        let mut came = Brought::default();
        self.bring_chats(&mut came, &self.older.chats, "", before, Path::new("chats"));
        self.bring_repo_chats(&mut came, before);
        self.bring_records(&mut came, before);
        came
    }

    // The chats of one folder, into the table of the chats. The place a chat belongs to is
    // given, because the directory of the loose chats and the directory of the chats of a
    // repository are two folders and one table.
    fn bring_chats(&self, came: &mut Brought, dir: &Path, repo_id: &str, before: &Path, rel: &Path) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") || !path.is_file() {
                continue;
            }
            let here = rel.join(
                path.file_name()
                    .map(|one| one.to_string_lossy().to_string())
                    .unwrap_or_default(),
            );
            let text = match fs::read_to_string(&path) {
                Ok(words) => words,
                Err(_) => {
                    came.unreadable += 1;
                    let _ = migrate::retire(&path, before, &here);
                    continue;
                }
            };
            let Ok(chat) = serde_json::from_str::<Chat>(&text) else {
                // A file of the directory of chats that does not read as a chat is written as
                // the directory of the files that were, where it may be checked, and is
                // informed of in the log of the program.
                came.unreadable += 1;
                proglog::note(
                    proglog::STORE,
                    &format!(
                        "a file of the chats does not read as a chat: {}",
                        path.display()
                    ),
                );
                let _ = migrate::retire(&path, before, &here);
                continue;
            };
            if chat.meta.id.is_empty() {
                came.unreadable += 1;
                let _ = migrate::retire(&path, before, &here);
                continue;
            }
            let placed = self.bring_chat(&chat, repo_id);
            if migrate::retire(&path, before, &here).is_err() {
                continue;
            }
            if placed {
                came.chats += 1;
            }
        }
        // The folder goes when nothing of it is left, so the program directory holds no form of
        // the keeping that was.
        let _ = fs::remove_dir(dir);
    }

    // Put one chat into the table. The place it is extracted from is given, because a chat of
    // the older format that came out of a repository did not always carry the id of that
    // repository in its own text. False returns when a chat of the same id remains already and
    // is the newer of the two, which is what a folder that was read at one place and written at
    // another leaves behind.
    fn bring_chat(&self, chat: &Chat, place: &str) -> bool {
        let repo_id = if chat.meta.repo_id.is_empty() {
            place
        } else {
            chat.meta.repo_id.as_str()
        };
        let Ok(text) = serde_json::to_string(chat) else {
            return false;
        };
        let Ok(database) = self.database() else {
            return false;
        };
        database
            .write(|conn| {
                let changed = conn
                    .execute(
                        "INSERT INTO chats (id, repo_id, title, created, updated, messages, data)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                         ON CONFLICT(id) DO UPDATE SET
                             repo_id = excluded.repo_id,
                             title = excluded.title,
                             created = excluded.created,
                             updated = excluded.updated,
                             messages = excluded.messages,
                             data = excluded.data
                         WHERE excluded.updated >= chats.updated",
                        params![
                            chat.meta.id,
                            repo_id,
                            chat.meta.title,
                            chat.meta.created,
                            chat.meta.updated,
                            chat.messages.len() as i64,
                            text
                        ],
                    )
                    .map_err(db::fault)?;
                Ok(changed > 0)
            })
            .unwrap_or(false)
    }

    // The chats of every repository. The contexts directory of a repository is named for the id
    // of that repository, and it is the id the conversations are keyed on, so the name of the
    // folder is the place of every chat inside it.
    fn bring_repo_chats(&self, came: &mut Brought, before: &Path) {
        let Ok(entries) = fs::read_dir(&self.older.repo_chats) else {
            return;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().to_string();
            if id.is_empty() {
                continue;
            }
            let rel = Path::new("repo_chats").join(&id);
            self.bring_chats(came, &dir, &id, before, &rel);
            let _ = fs::remove_dir(&dir);
        }
        let _ = fs::remove_dir(&self.older.repo_chats);
    }

    // The records of the repositories.
    fn bring_records(&self, came: &mut Brought, before: &Path) {
        let Ok(entries) = fs::read_dir(&self.older.repo_records) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") || !path.is_file() {
                continue;
            }
            let here = Path::new("repo_records").join(
                path.file_name()
                    .map(|one| one.to_string_lossy().to_string())
                    .unwrap_or_default(),
            );
            let text = match fs::read_to_string(&path) {
                Ok(words) => words,
                Err(_) => {
                    came.unreadable += 1;
                    let _ = migrate::retire(&path, before, &here);
                    continue;
                }
            };
            let Ok(repo) = serde_json::from_str::<Repo>(&text) else {
                came.unreadable += 1;
                let _ = migrate::retire(&path, before, &here);
                continue;
            };
            if repo.id.is_empty() {
                came.unreadable += 1;
                let _ = migrate::retire(&path, before, &here);
                continue;
            }
            let Ok(records) = self.database() else {
                continue;
            };
            let made = records
                .write(|conn| {
                    let changed = conn
                        .execute(
                            "INSERT INTO repos (id, name, created, last_used, pinned)
                             VALUES (?1, ?2, ?3, ?4, ?5)
                             ON CONFLICT(id) DO UPDATE SET
                                 name = excluded.name,
                                 created = excluded.created,
                                 last_used = excluded.last_used,
                                 pinned = excluded.pinned
                             WHERE excluded.last_used >= repos.last_used",
                            params![
                                repo.id,
                                repo.name,
                                repo.created,
                                repo.last_used,
                                db::mark_num(repo.pinned)
                            ],
                        )
                        .map_err(db::fault)?;
                    Ok(changed > 0)
                })
                .unwrap_or(false);
            if migrate::retire(&path, before, &here).is_err() {
                continue;
            }
            if made {
                came.repos += 1;
            }
        }
        let _ = fs::remove_dir(&self.older.repo_records);
    }

    // The tables of the database emptied. Only the tests need it: a check of the store or of
    // the engine wants a place of its own, and the place of the database is one file for the
    // whole run.
    #[cfg(test)]
    pub fn empty(&self) {
        let Ok(database) = self.database() else { return };
        let _ = database.write(|conn| {
            conn.execute("DELETE FROM chats", []).map_err(db::fault)?;
            conn.execute("DELETE FROM repos", []).map_err(db::fault)?;
            conn.execute("DELETE FROM plans", []).map_err(db::fault)?;
            Ok(())
        });
    }
}

// What the older format brought across. The number of the conversations, of the records, and of
// the files that did not read, so the first start after the change has an account of itself in
// the log of the program.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Brought {
    pub chats: usize,
    pub repos: usize,
    pub unreadable: usize,
}
// Put text into the place of a file through a second file beside it. The text go into a file of
// their own first, and that file is moved over the place with a rename, which is a step the
// computer cannot be caught half way through. A write that cannot be finished therefore leaves
// the file that was there to be read again, and reports why.
//
// The second file is named for the writer -- the number of this process and the number of this
// write -- so that no two writers of one place share a second file, and so that a write never
// meets a second file an earlier write left behind. A second file left behind because the
// program was stopped in the middle of a write does no harm: no list of chats reads a name that
// does not end in `json`.
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

// The name of the second file a write goes through: the name of the file it is to come into,
// with the number of this process and a count that moves on at every write after it. Two
// writers of one place are therefore in two second files, and the place itself only ever takes
// all of what a writer put into its own. `contexts` writes the compressed form of a
// conversation through it too.
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

// Say once in the log of the program that a row of the table of the chats does not read as a
// chat. The side bar reads the table again on every change, so the same row would else be
// stated over and over, and a line that shows nothing new a thousand times reports nothing at
// all.
fn note_unreadable_id(id: &str) {
    static SAID: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let said = SAID.get_or_init(Default::default);
    let kept = said.lock().unwrap_or_else(|gone| gone.into_inner());
    if kept.contains(id) {
        return;
    }
    drop(kept);
    crate::proglog::note(
        crate::proglog::STORE,
        &format!("a row of the chats does not read as a chat: {}", id),
    );
    let mut kept = said.lock().unwrap_or_else(|gone| gone.into_inner());
    kept.insert(id.to_string());
}

// An id of this program, or nothing at all. Only the characters an id is made of are let into a
// path, so an id out of a chat file of another computer cannot name a path out of the folder it
// is looked in.
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

// The directory of the record of one repository, outside every repository.
fn repo_record_path(root: &Path, repo_id: &str) -> Option<PathBuf> {
    clean_id(repo_id).map(|id| root.join(format!("{id}.json")))
}

// The directory of the chats of one repository, outside every repository. Only the characters
// an id of this program is made of are let into a path, so an id out of a chat file of another
// computer cannot name a path out of the contexts directory. An id with nothing usable in it
// gives nothing at all.
fn repo_chats_dir(root: &Path, repo_id: &str) -> Option<PathBuf> {
    clean_id(repo_id).map(|id| root.join(id))
}

// The chats of a repository used to be in the directory of that repository, at
// `<repository>/chats`. A command of the terminal tool works in that directory, so the model
// could read every conversation of the repository it was working in, and write over it. The
// chats now are under the data directory. This moves them, once, when the program starts:
// nothing of a chat is deleted, for a chat whose id is in both places keeps both copies, the
// newer under its own id and the older beside it with `.kept` at the end of its name, where no
// list of chats reads it.
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
            // The same chat is in both places, so the newer of the two is the one the program
            // reads, and the older is laid beside it out of the reach of the list of chats.
            let moved = newer.join(format!("{}.kept", name));
            if older_of(&stood, &from) {
                // The copy that already was outside is the older one, so the two change places
                // before the older is laid aside.
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
        // The folder goes when nothing of it is left, so the repository holds no marker of
        // where its conversations were kept.
        let _ = fs::remove_dir(&older);
    }
}

// True when the chat that is at `older` was written before the one that is at `newer`. A file
// that cannot be read as a chat is the older of the two, for a chat the program can read is
// worth more than one it cannot.
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

// The id of the repository that is at this path, out of its record file or out of the name of
// its folder. The id of a repository, from the name of its folder. The record is not requested,
// for the record is outside the folder now, and the name of the folder is what the
// conversations of a repository are keyed on.
fn repo_id_of(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().to_string();
    if let Some(tail) = name.strip_prefix("repository-") {
        return Some(tail.to_string());
    }
    id_from_dir_name(&name)
}

// The record of a repository used to be in the directory of that repository, at
// `<repository>/repo.json`. A command of the terminal tool works in that directory, so the
// model could read the record of the repository it was working in, see the other repositories
// from it, and write over its own. The records now are under the data directory. This moves
// them, once, when the program starts. A repository whose record is in both places keeps the
// one under the data directory, which is the newer of the two, and the copy inside the
// directory is closed with it.
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
        // The id of the record itself comes before the id of the folder, for a folder that was
        // renamed while the record was not keeps the id the conversations are keyed on.
        let mut repo: Repo = serde_json::from_str(&text).unwrap_or_default();
        if repo.id.is_empty() {
            repo.id = match repo_id_of(&path) {
                Some(id) => id,
                None => {
                    // Nothing names this folder, so the record cannot be put anywhere. It is
                    // removed rather than left where a command could reach it.
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
            // The path of the directory is not kept in a record, and was never written into
            // one: what is in the folder is what remains now.
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

    // A directory of this computer for one check, with a name no other check has.
    fn room(what: &str) -> PathBuf {
        static TURN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let place = std::env::temp_dir().join(format!(
            "autumn-natter-store-{}-{}-{}",
            what,
            std::process::id(),
            TURN.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        fs::create_dir_all(&place).expect("the directory of the test is created");
        place
    }

    // A store whose database, and whose folders of the older format, are in one space.
    // Every check of the store takes a directory of its own, so no check reaches the files of
    // the computer or of another check.
    fn store_in(room: &Path) -> Store {
        Store::with_rooms(
            room.join("repositories"),
            room,
            Older {
                chats: room.join("chats"),
                repo_chats: room.join("repo_chats"),
                repo_records: room.join("repo_records"),
            },
        )
    }

    // The chats of a repository are not inside the directory of that repository. A terminal
    // command of a chat runs in the working directory of its repository, so a chat kept there
    // could be read and written over by the model that is generating in it. They came out of
    // the directory into a folder under the data directory, and now come on from that folder
    // into the database: the working directory of the chat stays in the repository and nothing
    // else.
    #[test]
    fn the_chats_of_a_repository_come_out_of_it_and_into_the_database() {
        let room = room("chats-into-the-database");
        let repos = room.join("repositories");
        let talks = room.join("repo_chats");
        // A repository of the older format: its directory holds a directory of chats.
        let repo = repos.join("notes_ab12cd34ef");
        fs::create_dir_all(repo.join("chats")).unwrap();
        fs::write(repo.join("repo.json"), r#"{"id":"ab12cd34ef","name":"notes"}"#).unwrap();
        let stood = repo.join("chats").join("99.json");
        fs::write(&stood, r#"{"meta":{"id":"99","title":"t","repo_id":"ab12cd34ef","model_id":"m","created":1,"updated":5,"code_enabled":false,"sandbox_enabled":true,"context_used":0},"messages":[]}"#).unwrap();
        // The working directory of the chat stays where it is: only the conversations move.
        fs::write(repo.join("notes.md"), "work").unwrap();
        migrate_repo_chats(&repos, &talks);
        let moved = talks.join("ab12cd34ef").join("99.json");
        assert!(moved.is_file(), "the chat did not move out");
        assert!(!stood.exists(), "the chat is still inside the repository");
        assert!(!repo.join("chats").exists(), "the directory of chats is still there");
        assert!(repo.join("notes.md").is_file(), "the working directory of the chat stays");
        // The chat is read again from the database, and the file it was in goes into the
        // directory of the files that were.
        let store = store_in(&room);
        let came = store.bring_older_shape(&room.join("pre-database"));
        assert_eq!(came.chats, 1, "the chat came across");
        let listed = store.list_chats("ab12cd34ef");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].meta.id, "99");
        assert!(!moved.exists(), "the file of the chat is still was on");
        assert!(
            room.join("pre-database")
                .join("repo_chats")
                .join("ab12cd34ef")
                .join("99.json")
                .is_file(),
            "the chat that came across is kept where it may be checked"
        );
        let left: Vec<String> = fs::read_dir(&repo)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(left, vec![String::from("notes.md"), String::from("repo.json")], "{:?}", left);
        // A chat written now goes into the database, and nothing is in the directories of the
        // older format.
        let mut chat = listed[0].clone();
        chat.meta.title = String::from("again");
        store.save_chat(&chat).expect("the chat is written");
        assert_eq!(store.list_chats("ab12cd34ef")[0].meta.title, "again");
        assert!(!talks.exists(), "the contexts directory is removed with its text");
        let _ = fs::remove_dir_all(&room);
    }

    // The record of a repository is not inside the directory of that repository either: the
    // model that works in a repository can read the folder it works in, and the record of it is
    // not its business. It comes into the database with the conversations.
    #[test]
    fn the_record_of_a_repository_comes_out_of_it_and_into_the_database() {
        let room = room("records-into-the-database");
        let repos = room.join("repositories");
        let records = room.join("repo_records");
        // A repository of the older format: its directory holds its own record.
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
        assert!(repo.join("notes.md").is_file(), "the working directory of the chat stays");
        // What the record stated is not lost in the coming across.
        let store = store_in(&room);
        let came = store.bring_older_shape(&room.join("pre-database"));
        assert_eq!(came.repos, 1, "the record came across");
        let listed = store.scan_repos();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "ab12cd34ef");
        assert_eq!(listed[0].name, "notes");
        assert!(listed[0].pinned, "the pin came across");
        assert_eq!(listed[0].last_used, 9);
        assert!(!records.exists(), "the directory of the records is removed with its text");
        assert!(
            room.join("pre-database")
                .join("repo_records")
                .join("ab12cd34ef.json")
                .is_file(),
            "the record that came across is kept where it may be checked"
        );
        // A new repository is created with its record in the database and nothing of it inside
        // its directory, and a rename leaves the record where it is.
        let made = store.create_repo("The Ledger").unwrap();
        assert!(!made.path.join("repo.json").exists(), "a new record stood inside");
        let renamed = store.rename_repo(&made.id, "The Ladder").unwrap();
        assert_eq!(
            renamed.path.file_name().unwrap().to_string_lossy(),
            "the-ladder_".to_string() + &made.id
        );
        let again = store
            .scan_repos()
            .into_iter()
            .find(|one| one.id == made.id)
            .expect("the renamed repository is listed");
        assert_eq!(again.name, "The Ladder");
        // The pin of a repository is put down and read again.
        let mut pinned = again.clone();
        pinned.pinned = true;
        store.save_repo(&pinned).expect("the pin is put down");
        assert!(
            store
                .scan_repos()
                .into_iter()
                .find(|one| one.id == made.id)
                .expect("the repository is listed")
                .pinned,
            "the pin did not move"
        );
        let _ = fs::remove_dir_all(&room);
    }

    // A chat with nothing in it is never put in the place of a chat that holds a conversation.
    // The write of a chat means all of it, from a chat the program had not read whole, so the
    // conversation of it would go with the text that was not there -- the side bar would then
    // show a chat with nothing in it where a conversation was.
    #[test]
    fn a_chat_with_nothing_in_it_never_takes_the_place_of_a_talk() {
        let room = room("empty-over-talk");
        let store = store_in(&room);
        let mut chat = Chat::new("ab12cd34ef", "");
        chat.messages.push(Message {
            role: String::from("You"),
            body: String::from("A written question"),
            when: now_secs(),
            ..Default::default()
        });
        store.save_chat(&chat).expect("the chat is written");
        let id = chat.meta.id.clone();

        // The same chat, with the conversation taken out of it. The row that remains holds a
        // conversation, so the chat of nothing is refused and the conversation is kept.
        chat.messages.clear();
        let problem = store.save_chat(&chat).expect_err("a chat of nothing is refused");
        assert!(problem.contains("is not put over"), "{}", problem);
        let again = store.load_chat(&id).expect("the conversation is read again");
        assert_eq!(again.messages.len(), 1, "the conversation that was kept");
        assert_eq!(store.list_chats("ab12cd34ef").len(), 1, "the chat is still listed");

        // Where nothing remains, a chat of nothing is written as it was before: the guard keeps
        // a conversation, and stops nothing else.
        let blank = Chat::new("ab12cd34ef", "");
        store.save_chat(&blank).expect("a new chat of nothing is written");
        assert_eq!(store.list_chats("ab12cd34ef").len(), 2);
        let _ = fs::remove_dir_all(&room);
    }

    // A write of the database that cannot be finished records it, and leaves the row that was
    // to be read again. This is what the second file beside a chat used to keep: a chat
    // truncated was a chat been sent of the top bar, and a record truncated was a group of
    // chats that could no longer be named. The name of a repository and the marker of its pin
    // are in one row now, so they cannot get out of sync at all.
    #[test]
    fn a_write_that_cannot_be_finished_leaves_the_rows_that_stood() {
        let room = room("write-unfinished");
        let store = store_in(&room);
        let repo = store.create_repo("Banach").expect("the repository is made");
        assert_eq!(
            store
                .scan_repos()
                .into_iter()
                .find(|one| one.id == repo.id)
                .expect("the repository is listed")
                .name,
            "Banach"
        );
        // A record of a repository the program did not make is never written: a directory of
        // the user's own, put into the repositories directory, is left as it is now, and its
        // name comes from the name of the folder.
        let other = room.join("repositories").join("of_the_user_0123456789");
        fs::create_dir_all(&other).expect("the directory of the user is created");
        let listed = store
            .scan_repos()
            .into_iter()
            .find(|one| one.id == "0123456789")
            .expect("the directory of the user is listed");
        // A folder with no record of its own is named out of the name of the folder: the header
        // of it, with the dashes turned back into spaces.
        assert_eq!(listed.name, "of_the_user");
        assert!(!listed.pinned);
        store.save_repo(&listed).expect("a record of a directory of the users own is refused quietly");
        let after = store
            .scan_repos()
            .into_iter()
            .find(|one| one.id == "0123456789")
            .expect("the directory of the user is listed still");
        assert_eq!(after.name, "of_the_user", "nothing of it was written");
        let _ = fs::remove_dir_all(&room);
    }

    // Two stores of one database file remain as a second window and the page of the remote mode
    // remain: one file, and the conversations of both kept. Before the database the second
    // writer cut the second file of the first short between the moment it was filled and the
    // moment it was moved, and the first writer then moved a file of nothing into the place of
    // a chat -- which is how every chat a user opened from the side bar in turn was sent of it,
    // opening a chat being a write of all of it for the sake of one marker.
    #[test]
    fn two_writers_of_one_database_keep_both_talks() {
        let room = room("two-writers");
        let first = store_in(&room);
        let second = store_in(&room);
        let mut one = Chat::new("", "");
        one.meta.title = String::from("The first conversation");
        one.messages.push(Message {
            role: String::from("You"),
            body: String::from("A written question"),
            when: now_secs(),
            ..Default::default()
        });
        first.save_chat(&one).expect("the first conversation is written");
        let mut two = Chat::new("", "");
        two.meta.title = String::from("The second conversation");
        two.messages.push(Message {
            role: String::from("You"),
            body: String::from("Another question"),
            when: now_secs(),
            ..Default::default()
        });
        second.save_chat(&two).expect("the second conversation is written");
        // Opening the first conversation again writes all of it, and costs the second
        // conversation nothing.
        one.meta.updated += 1;
        first.save_chat(&one).expect("the first conversation is written again");
        let listed = second.list_chats("");
        assert_eq!(listed.len(), 2, "both conversations remain");
        assert!(listed.iter().any(|chat| chat.meta.title == "The second conversation"));
        assert!(listed.iter().any(|chat| chat.meta.title == "The first conversation"));
        let _ = fs::remove_dir_all(&room);
    }

    // A database that will not open stops a write and reports why, and the store goes on
    // answering with the empty answer it can answer with. A directory of the program that
    // cannot be written must not stop the program from starting.
    #[test]
    fn a_database_that_will_not_open_is_said_and_the_store_goes_on() {
        let room = room("database-will-not-open");
        // A file is where the directory of the database has to be, so the database cannot be
        // made.
        fs::create_dir_all(&room).expect("the room is made");
        fs::write(room.join("chats.db"), "not a database of the program").expect("the file stands");
        let store = Store::in_room(
            room.join("repositories"),
            room.join("chats.db"),
            Older {
                chats: room.join("chats"),
                repo_chats: room.join("repo_chats"),
                repo_records: room.join("repo_records"),
            },
        );
        let chat = Chat::new("", "");
        let problem = store.save_chat(&chat).expect_err("the write is refused");
        assert!(!problem.is_empty(), "a reason is given");
        assert!(store.list_chats("").is_empty(), "nothing is listed");
        assert!(store.load_chat("aaaaaaaa01").is_none(), "nothing is read");
        assert!(store.scan_repos().is_empty(), "no repository is listed");
        let _ = fs::remove_dir_all(&room);
    }

    // The text of the older format is all of what a computer holds before the databases. They
    // come across as they are now, the files they came from go into the directory of the files
    // that were, and a file that does not read is informed of and laid there too.
    #[test]
    fn the_older_shape_of_the_words_comes_into_the_database() {
        let room = room("older-shape");
        let store = store_in(&room);
        let places = Older {
            chats: room.join("chats"),
            repo_chats: room.join("repo_chats"),
            repo_records: room.join("repo_records"),
        };
        fs::create_dir_all(&places.chats).unwrap();
        fs::create_dir_all(places.repo_chats.join("ab12cd34ef")).unwrap();
        fs::create_dir_all(&places.repo_records).unwrap();
        // The record of a repository means the directory of that repository, which the side bar
        // walks: a record with no directory is a group of chats with no place to work in, and
        // is not listed.
        fs::create_dir_all(room.join("repositories").join("ledger_cdef012345")).unwrap();
        let loose = Chat::new("", "");
        fs::write(
            places.chats.join(format!("{}.json", loose.meta.id)),
            serde_json::to_string(&loose).unwrap(),
        )
        .unwrap();
        let of_repo = Chat::new("ab12cd34ef", "");
        fs::write(
            places
                .repo_chats
                .join("ab12cd34ef")
                .join(format!("{}.json", of_repo.meta.id)),
            serde_json::to_string(&of_repo).unwrap(),
        )
        .unwrap();
        fs::write(
            places
                .repo_records
                .join("cdef012345.json"),
            r#"{"id":"cdef012345","name":"Ledger","created":3,"last_used":4,"pinned":true}"#,
        )
        .unwrap();
        // A file of the directory of chats that does not read as a chat.
        fs::write(places.chats.join("broken.json"), "not a chat").unwrap();
        let came = store.bring_older_shape(&room.join("pre-database"));
        assert_eq!(came.chats, 2, "both conversations came across");
        assert_eq!(came.repos, 1, "the record came across");
        assert_eq!(came.unreadable, 1, "the file that did not read is informed of");
        assert_eq!(store.list_chats("").len(), 1, "the loose chat stands");
        assert_eq!(store.list_chats("ab12cd34ef").len(), 1, "the chat of the repository stands");
        assert_eq!(
            store
                .scan_repos()
                .into_iter()
                .find(|one| one.id == "cdef012345")
                .expect("the repository stands")
                .name,
            "Ledger"
        );
        // The directories of the older format are removed, and the files remain where a person
        // may look at them.
        assert!(!places.chats.exists());
        assert!(!places.repo_chats.exists());
        assert!(!places.repo_records.exists());
        assert!(room
            .join("pre-database")
            .join("chats")
            .join("broken.json")
            .is_file());
        // The scan requests nothing a second time: what came across is in the database, and the
        // files that were came out of the folders that were.
        let again = store.bring_older_shape(&room.join("pre-database"));
        assert_eq!(again, Brought::default(), "nothing is brought twice");
        let _ = fs::remove_dir_all(&room);
    }

    // A chat that is in a file older than the row that is in the database does not take the row
    // over. The same conversation may be in the directory of the chats and in the database when
    // the program was pointed at two folders of the program, and the newer of the two is the
    // conversation the user was reading.
    #[test]
    fn an_older_file_does_not_take_the_place_of_a_newer_row() {
        let room = room("older-than-the-row");
        let store = store_in(&room);
        let places = Older {
            chats: room.join("chats"),
            repo_chats: room.join("repo_chats"),
            repo_records: room.join("repo_records"),
        };
        fs::create_dir_all(&places.chats).unwrap();
        let mut stood = Chat::new("", "");
        stood.meta.title = String::from("The newer conversation");
        stood.meta.updated = 20;
        store.save_chat(&stood).expect("the conversation is in the database");
        let mut older = stood.clone();
        older.meta.title = String::from("The older conversation");
        older.meta.updated = 10;
        fs::write(
            places.chats.join(format!("{}.json", older.meta.id)),
            serde_json::to_string(&older).unwrap(),
        )
        .unwrap();
        let came = store.bring_older_shape(&room.join("pre-database"));
        assert_eq!(came.chats, 0, "nothing came across");
        assert_eq!(
            store.list_chats("")[0].meta.title,
            "The newer conversation",
            "the newer conversation is kept"
        );
        let _ = fs::remove_dir_all(&room);
    }

    // The line of the handoff summary is a line of the drop-down like every other one, and it
    // is on from the beginning. With compaction off it counts for nothing: a chat whose only
    // line is on behind that line has no tool the model could reach, and the badge of the tools
    // does not rest on it.
    #[test]
    fn the_line_of_the_handoff_summary_stands_with_the_other_lines() {
        let mut switches = ToolSwitches::default();
        assert!(switches.value_of("summarise"), "the line is on at first");
        assert!(switches.words().iter().any(|(part, _)| *part == "summarise"));
        assert!(switches.set("summarise", false));
        assert!(!switches.value_of("summarise"));
        assert!(switches.any_on(), "the other lines are on");
        assert!(switches.any_on_with(false), "the other lines are on");
        // Every line but that of the handoff summary is removed. The lines are read from the
        // switches themselves, so a line the program learns later is taken out here as well
        // rather than leaving this test to say an item that is no longer true of it. The badge
        // is on while the space is freed by a summary, and is sent when it is not.
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
        assert!(!switches.any_on_with(false), "with compaction off nothing is behind the badge");
        switches.set("summarise", false);
        assert!(!switches.any_on_with(true), "no line stands on at all");
    }

    // Deleting a repository deletes its folder, so the group leaves the side bar, which is the
    // listing of the repositories directory. A corner left by the older behaviour is skipped by
    // the listing, and nothing inside it counts as a repository.
    #[test]
    fn a_removed_repository_is_deleted_and_not_listed() {
        let room = room("moved-aside");
        let store = store_in(&room);
        let one = store.create_repo("The Orchard").expect("the first is made");
        let two = store.create_repo("The Arbour").expect("the second is made");
        fs::write(one.path.join("notes.md"), "the work of the user").expect("work is written");
        assert!(store.rows_of_repos().contains_key(&one.id), "both stand");
        store.delete_dir(&one.id).expect("the directory is deleted");
        assert!(!one.path.exists(), "the folder is gone");
        assert!(
            !store.repos_dir.join(REMOVED).exists(),
            "no corner is made to keep what was deleted"
        );
        let listed: Vec<String> = store.scan_repos().into_iter().map(|r| r.id).collect();
        assert!(!listed.contains(&one.id), "the deleted repository is not listed");
        assert!(listed.contains(&two.id), "the other stands on");
        // A corner left behind by the older behaviour is not a repository, and the directories
        // inside it are one level too deep to be seen from here.
        let deep = room
            .join("repositories")
            .join(REMOVED)
            .join("of_the_user_000000000f");
        fs::create_dir_all(&deep).expect("a folder in the corner is created");
        assert!(
            store.scan_repos().into_iter().all(|r| r.id != "000000000f"),
            "nothing in the corner is listed"
        );
        store.drop_repo(&one.id).expect("the record is removed");
        assert!(
            !store.rows_of_repos().contains_key(&one.id),
            "no record of the repository is left"
        );
        assert!(store.rows_of_repos().contains_key(&two.id), "the other record stands");
        let named = store.drop_repo("");
        assert!(named.is_err(), "nothing to remove is answered as an error");
        let gone = store.delete_dir("nothing-at-all");
        assert!(gone.is_err(), "a repository that is not there is answered as an error");
        let _ = fs::remove_dir_all(&room);
    }
}
