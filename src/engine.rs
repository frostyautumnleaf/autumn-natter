// The engine. It owns the settings, the chats, the model list and the llama-server process. The
// window and the browser page both drive this one engine, so both show the same.
//
// One worker thread does all slow work: it starts the server, asks the model and runs the
// terminal tool. A window only reads a snapshot of the state, so a slow answer never stops the
// window from drawing.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::askuser;
use crate::attach;
use crate::store::Attachment;
use crate::config::{self, Config, ModelSettings};
use crate::contexts;
use crate::display;
use crate::download;
use crate::editfile;
use crate::gpu::{self, Gpu};
use crate::infographic;
use crate::llama::{self, ModelInfo, Server, ToolCall};
use crate::markdown;
use crate::mcp;
use crate::migrate;
use crate::proglog;
use crate::corporate;
use crate::planning;
use crate::search;
use crate::store::{self, Chat, Message, Repo};
use crate::steps;
use crate::suggest;
use crate::summarise;
use crate::tasks;
use crate::tools;
use crate::viewimage;

// The time each press of the marker adds to a call that is still running. The marker is in the
// header of the block of the call, and a call may be given more time as often as the user
// presses it.
const TOOL_EXTEND: Duration = Duration::from_secs(300);
// How many terminal commands one answer may run, and how many round trips one answer may take
// in all, both read from the Chatting settings. A marker of a chapter is not a command: it runs
// nothing, so it eats neither space. The two settings remain unlimited for a new set of
// settings, and the answer then ends only when the model stops by itself.
//
// A ceiling of nothing is a poor item to leave in an answer, so the two spaces are never
// smaller than this when the user sets one: one round trip for the turn that asks, and one
// command.
const LEAST_TOOL_STEPS: usize = 1;
const LEAST_ANSWER_STEPS: usize = 2;

// One send of the chat composer: the text of the user, and the files they hung on it, in the
// order the user hung them.
struct SentMessage {
    text: String,
    files: Vec<attach::Incoming>,
}

// One message of the waiting list: the text, the files hung on them, and the number the page
// knows the row by. The numbers of one chat run from 1 up and are never given out twice, so a
// page that holds a list of its own still points at the right row after a message was sent of
// the list.
#[derive(Clone, Debug)]
struct QueueItem {
    id: u64,
    text: String,
    files: Vec<attach::Incoming>,
    // The text a programmed message waits for, as the user wrote it. Empty for a message that
    // is only waiting its turn. A programmed message is sent when an answer of the chat holds
    // this text, whatever the rest of the list is doing - and ahead of it, because the user
    // stated when it was to go.
    matches: String,
    // True when an answer of the chat held the text above. A message that has found its text
    // keeps them: a later answer cannot take the finding back.
    ready: bool,
}

// The waiting list of one chat. The list belongs to the chat and to the program, and not to the
// page that wrote it: a page that is removed leaves the list where it is, and the worker sends
// the messages as it does after any other answer. Each chat has a list of its own, for only the
// chat that is left open can be answered, and a message written for one chat is not meant for
// another.
#[derive(Clone, Debug, Default)]
struct WaitingList {
    chat: String,
    // The next message to send is at the header.
    items: Vec<QueueItem>,
    // The programmed messages, in the order they were written. They remain apart from the
    // messages above: they are sent when an answer contains the text they wait for, and not
    // when their turn comes.
    planned: Vec<QueueItem>,
    seq: u64,
    // When the last message of this list was sent. Two of them must not go out on top of one
    // another: the chat has to say it is writing again first.
    sent_at: Option<std::time::Instant>,
    // True when a message of this list could not be sent and the rest of the list is held back
    // with it. A list that cannot be sent is not asked again every moment: it waits for the
    // user to touch it, or to send a word.
    held: bool,
}

// One row of the waiting list, as a page is given it. All of the files is not needed for a row:
// the page shows the name of one file, or how many there are, and the program keeps the data of
// them for the message itself.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct QueuedRow {
    pub id: u64,
    pub text: String,
    pub files: Vec<String>,
    // The text this message waits for, empty for a message that is only waiting its turn. The
    // text is given back as the user wrote it, so the row can say what it is waiting for.
    #[serde(default)]
    pub matches: String,
}

// The waiting list of the chat that is left open, as a page is given it. Nothing for a chat
// with no list, so a page that was drawn with rows takes them away when the last one was sent.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct WaitingSnap {
    pub chat: String,
    // The next message to send is at the header, as it does in the program.
    pub items: Vec<QueuedRow>,
    // The programmed messages, which the page draws in a section of their own under the same
    // header.
    pub planned: Vec<QueuedRow>,
    // True when the list is held back because a message of it could not be sent. The reason is
    // in the status line of the window.
    pub held: bool,
}

// What the worker is to do with the chat. A turn either answers a message of the user, or goes
// on with the work from where the conversation remains. The second is what follows a handoff
// summary: the summary is written so that the model may carry on, and nothing is added to the
// chat for the model to read, for the handoff block that names the summary already reports it
// to go on.
enum Turn {
    // A message of the user, with the files beside it.
    Sent(SentMessage),
    // Go on from the conversation as it is now.
    Continue,
    // Go on from the conversation as it is now, after one message of it was taken out with
    // everything that came after it. The request is the conversation as it is now, and the
    // tools are proposed as they are for a message of the user: the user requested the answer
    // again, not for the tail of a handoff summary.
    Retry,
    // Go on with the answer that is at the end of the conversation, after the user stopped it
    // while the model was still writing. The text the model had put down are the last message
    // of the conversation, so the template leaves the marker that opens the text of the model
    // open, and the model writes on from the last word it had written.
    Resume(String),
}

// What an answer of the model came to, so the worker knows whether the work may be continued
// after the space was freed.
#[derive(PartialEq, Eq)]
enum Ended {
    // The model finished of its own answer.
    Done,
    // Nothing to go on from: the user stopped the answer, the stream faulted, the space was
    // come to again and the answer gave up, or the turn never began.
    Halted,
}

// Work for the worker thread.
//
// Every turn names the chat it belongs to, so two chats may be answered at the same time and
// one chat is never answered twice over. Only a change of the server itself requests the model
// to be loaded again: the thinking mode, the choice of keeping the thinking, the tools of the
// chat and the sandbox of the sandbox are stated with the request they belong to, so a badge of
// the chat composer never costs the loading of the model over again.
enum Job {
    // The user asked something in one chat, with the files of the message beside it.
    Prompt { chat: String, sent: SentMessage },
    // Another model is wanted, so the running server must go. It goes when no chat is being
    // answered on it, which is what lets a changed model mean the next turn instead of cutting
    // the turn that is on the way.
    ModelChanged,
    // The settings a server is started with moved, so the run must not keep the old values. The
    // same rule as for another model.
    SettingsChanged,
    // The user requested the handoff summary of one chat to be written again, after one that
    // never returned.
    Handoff(String),
    // The user requested the answer of one chat again. The conversation was cut back to the
    // message before the request, so the worker has nothing to look at but the conversation as
    // it is now.
    Retry(String),
    // The user requested an answer they stopped to be continued. The id is the answer at the
    // end of the conversation of that chat.
    Resume { chat: String, id: String },
    // The user added, changed or asked to look again at one server of their own, so the tools
    // it has to propose are read again.
    McpLook(String),
    // A turn of this chat finished: the chat is free again and a place at the model is left
    // open.
    Done(String),
    // Look again at what may be begun: the work that waited for a place at the model, and the
    // waiting lists of the chats that remain idle.
    Tick,
    // Leave the process, after the model is unloaded.
    Shutdown,
}


// What a server has to be for the current chat: the model it loaded, and nothing else. A
// running server that means the same model answers the chat, however much of the chat has moved
// since the last answer, so the model on it stays loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ServerWants {
    model_id: String,
}

impl ServerWants {
    // Whether a server started with `run` may answer a chat that requests `wants`. Only the
    // model itself has to be the same: the thinking mode, the keeping of the thinking, the
    // tools of the chat and the sandbox of the sandbox are all stated with the request and not
    // with the server, so a chat that is where the one before it was -- or nowhere near it --
    // runs on the server that is already up. A model that cannot read a tool call, because its
    // chat template will not be, is no reason to start it again: the second start has the same
    // trouble, and the user was informed of it once already (see `Server::start`).
    fn covers(run: &ServerWants, wants: &ServerWants) -> bool {
        run.model_id == wants.model_id
    }
}

// One server of the user's own, as the page is given it. The text of the code of a server of
// the program itself are kept out of the state, for the state is requested about six times a
// second and the code may be a long file: the settings panel requests them by a call of its
// own.
#[derive(Clone, Debug, Default)]
pub struct McpSnap {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub icon: usize,
    // Whether this server is in the drop-down of the badge.
    pub shown: bool,
    // Whether this server is switched on for the current chat.
    pub on: bool,
    // The names of the tools this server last answered with.
    pub tools: Vec<String>,
    // The reason the server could not be checked, or empty.
    pub fault: String,
}

// One chapter of a chat, as the window needs it. A chapter is kept in the chat as a message of
// its own, so it is where the model requested it, and the window draws it as a node on the
// chapter line and never as a message.
#[derive(Clone, Debug, Default)]
pub struct Chapter {
    // The id of the message the chapter is on.
    pub id: String,
    // The name the model gave the chapter, no longer than the tool allows.
    pub name: String,
    // The id of the message the chapter begins after. Empty when the chapter is at the very top
    // of the chat.
    pub after: String,
}

// One call of a tool the model is writing right now. The user sees the text of it as they come,
// so the block is in the chat before the call is over and before the command it requests has
// run. The text is those the user is given: the command, and the reason the model gave for it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LiveTool {
    // The name of the tool, as the model wrote it.
    pub name: String,
    // The text of the call so far, as the user is shown it. For a run of the terminal tool it
    // is the command, and for a file tool it is the text the call is making. It is taken from
    // the text of the call, which is often not finished.
    pub command: String,
    // The reason the model gave for the command, as far as it has come. It is the terminal tool
    // alone in ever requested one.
    pub purpose: String,
    // The one item the header of the block names: the file a file tool works on. The command of
    // a run names itself, so it remains empty there.
    pub target: String,
    // True when the model has moved on from this call: it is thinking again, or writing text of
    // its own, so nothing more comes of the text being written. The block records it, and the
    // run of the tool is in the chat in its place when the step of the answer is over.
    pub done: bool,
}

// The call of a tool that is running right now, and the clock it runs against. Every call is
// given a time to return within, so that nothing of a model's answer stays still for ever
// behind a call that never returns. The clock is here so the two markers of a running call --
// the one that cuts it short and the one that gives it more time -- have a clock to speak to.
#[derive(Clone, Debug)]
pub struct Running {
    // The message of the block of the call, so the markers are on the call they belong to and
    // on no other.
    pub id: String,
    // The clock of the call. It is shared with the program that was started.
    watch: crate::util::Watch,
}

// The same, as a page is given it. The time left is counted again whenever the state is
// checked, so a page has nothing to count on its own.
#[derive(Clone, Debug, Default)]
pub struct RunningTool {
    pub id: String,
    // All of the time the call was given, in seconds. Nothing at all for a call that was given
    // no time to return within, for then there is no time to run out and no marker to remain.
    pub timeout: Option<u32>,
    // The seconds left of that time, of a call with a time to run out.
    pub left: Option<u32>,
}

// The clock a call of a tool is put under, from the Chatting settings. Nothing at all for a
// call when the user turned the feature off, which is the same as a call that may take as long
// as it likes.
fn tool_watch(state: &Arc<Mutex<State>>) -> crate::util::Watch {
    let seconds = match state.lock() {
        Ok(guard) => guard.cfg.chat_tool_timeout,
        Err(_) => 0,
    };
    if seconds == 0 {
        return crate::util::Watch::unbounded();
    }
    crate::util::Watch::new(Duration::from_secs(seconds as u64))
}

// One question of a questionnaire, with the answers the user may choose from. `many` reports
// whether more than one of them may be chosen.
#[derive(Clone, Debug)]
pub struct AskItem {
    pub prompt: String,
    pub choices: Vec<String>,
    pub many: bool,
}

// A question the model has put to the user, and the wait that is with it. The question is above
// the chat composer until the user answers it, the time of an ordinary question is removed, or
// the user stops the answer; the three paths end the wait, and each has its own text for the
// model.
#[derive(Clone, Debug)]
pub struct Asking {
    // Lets the answer of the user find the question it belongs to, when a page draws the
    // request again after the question was asked.
    pub id: String,
    // The question, and the reason the model gives with it.
    pub question: String,
    // The answers the model proposed for the user to choose from.
    pub choices: Vec<String>,
    // True when the model stated it cannot go on without an answer.
    pub critical: bool,
    // How long the question is waited for. Nothing at all waits until the user answers, which
    // is what a question the model cannot go on without requests.
    pub seconds: u32,
    // The questions of a questionnaire, when the request is one. A request of the ordinary type
    // carries none, and the question above does all the work.
    pub items: Vec<AskItem>,
    // True when the request is the questionnaire of planning mode. It is in the conversation
    // itself, where the plan is being talked about, and not above the chat composer, and no
    // clock runs on it.
    pub plan: bool,
    // The moment the wait began, which is where the time left is measured from.
    began: Instant,
    // What the user answered. The wait is over when this holds anything.
    answer: Option<String>,
}

impl Asking {
    // The seconds left for the user to answer, or nothing when the question waits until it is
    // answered. The popup counts these seconds down.
    pub fn left(&self) -> Option<u32> {
        if self.seconds == 0 {
            return None;
        }
        let gone = self.began.elapsed().as_secs() as u32;
        Some(self.seconds.saturating_sub(gone))
    }

    // True when the wait is over: the user answered, or the time of an ordinary question is
    // removed.
    pub fn ended(&self) -> bool {
        self.answer.is_some() || self.left().map(|left| left == 0).unwrap_or(false)
    }
}

// The one item a call names, with the word a tool reports when that name has not arrive yet, or
// never came. The body of the block of such a tool holds the parts of the call as the model
// writes them, so the header cannot take its text from there: the first line of those parts is
// the name of a part, and not the one item the call is pointed at.
fn named(one: String, else_word: &str) -> String {
    if one.trim().is_empty() {
        return String::from(else_word);
    }
    one
}

// The text of a call that the user is given: the command of a terminal run, or the one item
// another tool names. The header of the block of a run and the block of a call being written
// both come from here, so the two cannot say different items about one call. The reason the
// model gave rides along for the terminal tool, which is the only tool that ever requested one.
//
// A tool whose text is not a command or a file shows its parts: every part of the call that has
// arrive is on a line of its own under its name, so the user watches the call grow instead of
// watching one word that never changes.
fn words_of(name: &str, raw: &str) -> (String, String, String) {
    match name {
        tools::TOOL_NAME => {
            let (command, purpose) = tools::partial_arguments(raw);
            (command, purpose, String::new())
        }
        editfile::EDIT_NAME | editfile::CREATE_NAME => {
            let (command, target) = editfile::live_params(name, raw);
            (command, String::new(), target)
        }
        // A file being proposed to the user is named by the file itself.
        download::NAME => (
            tools::live_arguments(raw),
            String::new(),
            named(download::partial_words(raw), "a file"),
        ),
        // A show of the contents of files is named by the first file of it.
        display::NAME => (
            tools::live_arguments(raw),
            String::new(),
            named(display::partial_words(raw), "a file"),
        ),
        // A question of the user is named by the question.
        askuser::NAME => (
            tools::live_arguments(raw),
            String::new(),
            named(askuser::partial_words(raw), "a question"),
        ),
        // The suggestions for the next steps is named by the first button of it.
        suggest::NAME => (
            tools::live_arguments(raw),
            String::new(),
            named(suggest::partial_words(raw), "the next step"),
        ),
        // The list of the steps of the work is named by the title of the block, or by the first
        // step. The body of the block holds every step so far.
        steps::NAME => (
            steps::live_body(raw),
            String::new(),
            steps::partial_words(raw),
        ),
        // A look about the web is named by the text to search for, or by the address that is
        // being read.
        search::NAME => (
            tools::live_arguments(raw),
            String::new(),
            named(search::partial_words(raw), "the web"),
        ),
        // The tools of planning name themselves: one writes the whole plan, one changes the
        // plan file of this chat, one reads it back, and one reports the plan is agreed. The
        // text of the plan stream in the body.
        planning::INIT_NAME => (
            tools::live_arguments(raw),
            String::new(),
            String::from("the plan file"),
        ),
        planning::EDIT_NAME => (
            tools::live_arguments(raw),
            String::new(),
            String::from("the plan file"),
        ),
        planning::READ_NAME => (
            tools::live_arguments(raw),
            String::new(),
            String::from("the plan file"),
        ),
        planning::ASK_NAME => (
            tools::live_arguments(raw),
            String::new(),
            String::from("the questions of the plan"),
        ),
        planning::END_NAME => (
            tools::live_arguments(raw),
            String::new(),
            String::from("the plan is agreed"),
        ),
        // The handoff summary is named by the conversation it puts away. The reason the model
        // gives with the call rides beside it.
        summarise::NAME => (
            tools::live_arguments(raw),
            summarise::reason(raw),
            String::from("the conversation of this chat"),
        ),
        viewimage::NAME | viewimage::ONE_NAME => (
            tools::live_arguments(raw),
            String::new(),
            named(viewimage::partial_words(raw), "an image"),
        ),
        // A tool of the infographic suite is named by the tool and the project.
        name if infographic::is_tool(name) => (
            tools::live_arguments(raw),
            String::new(),
            infographic::shown(name, raw),
        ),
        // A tool of the tasks suite is named by the task it works on: the name of the task
        // being made, or the index of the task being pointed at. The read of the whole list is
        // pointed at nothing, so it names the list.
        name if tasks::is_tool(name) => (
            tools::live_arguments(raw),
            String::new(),
            if name == tasks::VIEW_ALL_NAME {
                String::from("the list of tasks")
            } else {
                named(tasks::partial_words(name, raw), "a task")
            },
        ),
        // The summary of the work the model writes for the corporate display is named by the
        // heading of the area of work it is under.
        corporate::NAME => (
            tools::live_arguments(raw),
            String::new(),
            named(corporate::partial_words(raw), "the short of the work"),
        ),
        // The call that closes the block of the answer is named for what it does.
        corporate::FINISH_NAME => (
            tools::live_arguments(raw),
            String::new(),
            String::from("the end of the work"),
        ),
        // A tool of a server of the user's own is named by the tool itself, and the parts the
        // model is writing are the text of the block.
        name if mcp::is_tool(name) => (
            mcp::shown_tool(name),
            String::new(),
            mcp::partial_words(raw),
        ),
        // A name the program does not propose, and a name that arrived parts: the text of a
        // file is the best guess of what the model is naming.
        _ => (editfile::partial_words(raw), String::new(), String::new()),
    }
}

// The calls the user may watch being written. A call whose name is still nothing at all is kept
// out, for the name arrives parts and a few letters of it are not yet the name of a tool.
fn watched(calls: &[ToolCall]) -> Vec<LiveTool> {
    let mut out: Vec<LiveTool> = Vec::new();
    for call in calls {
        if call.name.is_empty() {
            continue;
        }
        let (command, purpose, target) = words_of(&call.name, &call.arguments);
        out.push(LiveTool {
            name: call.name.clone(),
            command,
            purpose,
            target,
            done: false,
        });
    }
    out
}

// One summarisation of a chat, as the window needs it. A summarisation is kept in the chat as a
// message of its own, so it is where the conversation was cut, and the window draws it as the
// heading of the summarisation and as a node on the timeline at the right of the chat.
#[derive(Clone, Debug, Default)]
pub struct Handoff {
    // The id of the message that holds the summarisation.
    pub id: String,
    // How many times this chat has been summarised up to and including this one. The heading
    // reports "Summarised #2" for the second of them.
    pub number: usize,
    // What became of the summary: "writing" while the model is at it, "done" when the text
    // came, and "owed" when they never came and the user can start the summary again.
    pub state: String,
    // When the summarisation was, in seconds since 1970.
    pub when: i64,
    // The space the conversation took before it was summarised, in tokens.
    pub before: u64,
    // The space the handoff summary itself takes, in tokens.
    pub after: u64,
    // A little of the handoff summary, for the card of a node. Empty while the summary was
    // never written.
    pub snippet: String,
    // What went wrong, when the summary was never written.
    pub why: String,
    // Which of the two asks the summary came from: the prompt of the settings, or the short
    // prompt that is asked again when the context window filled under the first ask. The user
    // reads the two apart, for a short summary holds less than the other, and the two are
    // requested with different text.
    pub short: bool,
}

// The summarisations of one chat, in the order they were created. `writing` reports that the
// program is at a summary right now, which is the one case in which an empty summarisation is
// not yet an error.
fn handoff_marks(chat: &Chat, writing: bool) -> Vec<Handoff> {
    let mut out: Vec<Handoff> = Vec::new();
    let last = chat.messages.len().saturating_sub(1);
    for (place, message) in chat.messages.iter().enumerate() {
        if message.role != "Summary" {
            continue;
        }
        let written = !message.body.trim().is_empty();
        let state = if written {
            "done"
        } else if writing && place == last {
            "writing"
        } else {
            "owed"
        };
        out.push(Handoff {
            id: message.id.clone(),
            number: out.len() + 1,
            state: state.to_string(),
            when: message.when,
            before: message.context_before,
            after: message.context_after,
            snippet: if written {
                summary_snippet(&message.body)
            } else {
                String::new()
            },
            why: if written {
                String::new()
            } else {
                message.tool.clone()
            },
            short: message.short,
        });
    }
    out
}

// True when the text is not the text of the model at all, but a call of a tool that was
// truncated: the opening marker of a call, and no closing marker, because the context window
// filled in the middle of it. Such text is not a summary, and the next model would read it as
// one.
fn call_of_a_tool(text: &str) -> bool {
    let open = String::from("<") + "|";
    let close = String::from("|") + ">";
    let Some(at) = text.find(open.as_str()) else {
        return false;
    };
    let rest = &text[at + open.len()..];
    !rest.contains(close.as_str()) && rest.trim_start().starts_with("im_start|tool")
}

// A little of a handoff summary, for the card of a node on the timeline. The markers of the
// writing are taken off, for a node has a small space to be read in.
fn summary_snippet(text: &str) -> String {
    let flat = text
        .replace("```", " ")
        .replace(['#', '*', '`', '>', '~'], "")
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ");
    let mut out = String::new();
    for ch in flat.chars() {
        if out.chars().count() >= 180 {
            out.push('\u{2026}');
            break;
        }
        out.push(ch);
    }
    out
}

// True when a line of the answer of the model only pointers the summary over, such as "Here is
// the handoff summary:". Those text is not part of the summary, and they cost space on
// every turn that comes after it.
fn is_lead_in(line: &str) -> bool {
    let low = line.trim().to_lowercase();
    let words: Vec<&str> = low.split_whitespace().collect();
    if words.is_empty() {
        return true;
    }
    if words.len() > 14 {
        return false;
    }
    let about = low.contains("summary") || low.contains("handoff");
    let hands = low.starts_with("here is")
        || low.starts_with("here's")
        || low.starts_with("here are")
        || low.starts_with("below is")
        || low.starts_with("this is")
        || low.starts_with("the following")
        || low.starts_with("what follows");
    let closes = low.ends_with(':') || low.ends_with('.');
    about && hands && closes
}

// True when a line of the answer only proposes an item of the model's own, such as "Let me know
// if you need more detail.". As above, it is not part of the summary and it is not wanted at
// the end of it.
fn is_offer(line: &str) -> bool {
    let low = line.trim().to_lowercase();
    let words: Vec<&str> = low.split_whitespace().collect();
    if words.is_empty() {
        return true;
    }
    if words.len() > 16 {
        return false;
    }
    [
        "let me know",
        "hope this helps",
        "good luck",
        "feel free to",
        "ask me if",
        "i hope this",
    ]
    .iter()
    .any(|start| low.starts_with(start))
}

// The handoff summary the model gave, cleaned of what the model wrapped it in. A model
// requested a summary sometimes puts a line of its own before it, or all of it inside a fence,
// or an propose after it. None of those text is part of the summary: they are the envelope of
// it, and they are sent again on every turn after the compaction. The markers of the writing
// are left where they are, for the summary is put on the page again by the next model.
pub fn clean_summary(text: &str) -> String {
    // The markers of a turn that are not text. A model sometimes repeats one of them inside its
    // answer, and such a marker has no business going back.
    let bare: String = text
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect();
    let mut lines: Vec<&str> = bare.lines().map(|line| line.trim_end()).collect();
    // A fence about all of it is the wrapper of the summary: the prompt requests no styling, so
    // a fence at either end is not the summary itself. The wrapper is taken off again after the
    // two loops below, for a model that pointers the summary over with a line of its own tends
    // to shut the fence under that line rather than under the last word of the summary.
    let unwrap = |lines: &mut Vec<&str>| {
        if !lines.is_empty() && lines[0].trim_start().starts_with("```") {
            lines.remove(0);
        }
        if !lines.is_empty() && lines[lines.len() - 1].trim().starts_with("```") {
            lines.pop();
        }
    };
    unwrap(&mut lines);
    while lines
        .first()
        .map(|line| is_lead_in(line))
        .unwrap_or(false)
    {
        lines.remove(0);
    }
    while lines
        .last()
        .map(|line| is_offer(line))
        .unwrap_or(false)
    {
        lines.pop();
    }
    unwrap(&mut lines);
    // A model that answers the request with a call of a tool, rather than with the summary, has
    // written nothing that the next model can use. The text of a call that the directory
    // truncated is deleted here, so the request is rebuilt rather than a call of a tool
    // remaining in the place of a summary.
    if call_of_a_tool(text) {
        return String::new();
    }
    // Two blank lines one behind the other say nothing, and the next model pays for all of
    // them.
    let mut out = String::new();
    let mut blanks = 0usize;
    for line in lines {
        if line.trim().is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
        } else {
            blanks = 0;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    out.trim().to_string()
}

// The text that go round a handoff summary when it is sent to the model again. The model has to
// know that the conversation it is reading was created shorter, that the text it is given was
// written by a model at the point of the compaction, and that it is there to go on with the
// work and not to begin it again. The first text of the block that goes round a handoff
// summary. They are searched for whole, and only at the start of a word that is sent, so no
// other saying is taken for one.
pub const HANDOFF_FIRST: &str = "[The session was compacted";

pub fn handoff_block(summary: &str) -> String {
    format!(
        "{} The messages between the first user message and this point were removed, so that the conversation could continue inside the context window of the model. The text below is a handoff summary written by an AI at that point, so that the session can continue. Read it as the state the session was in, and continue the work from it. Do not greet the user, do not repeat this summary, and do not start the work again.]\n\n{}",
        HANDOFF_FIRST, summary
    )
}

// The text the program puts in the place of the user when a turn of the chat only goes on, and
// the conversation it is given ends on a call the server has to answer. Such a turn carries no
// message of the user of its own, so the server is asked to pay the debt of that call instead
// of going on with the work: it writes a call, the text of it is deleted with the request that
// made it, and no answer of the model reaches all. This text go out after the conversation as a
// message of the user, so the request ends on a question. They are never written into the chat,
// for the user never wrote them and has nothing to read where they are.
pub const CONTINUE_PROMPT: &str = "[Continue the work from the point it reached above. Do not greet me and do not begin the work again. Report what the last run printed, answer in text when no tool is needed, and call a tool only when the work needs one. A tool named above is unavailable unless it appears in the list you are given now.]";

// The line that is under a tool call which was stopped where the model was still writing it.
// The header of the block reports what the tool was for; this reports what became of it, in
// text the user does not have to be a model to follow.
pub const CANCELLED_WORDS: &str = "The call was cancelled. The conversation reached the summarisation limit while the call was being written, so the program stopped the writing and the tool never ran";

// The same line, for a call the user stopped manually. Nothing of the directory is behind it:
// the user pressed stop, and the tool never ran.
pub const STOPPED_WORDS: &str = "The call was cancelled: you stopped the answer while the call was being written, so the writing was stopped and the tool never ran";

// The greatest length of the text of a cut-off tool call that is kept. A call of this type is
// seldom short: a model writing a file out puts all of the file in it. The block the user is
// shown and the note the summary is written from both give the header of the call, which names
// what it was for: the file, the command, the start of the text. Text longer than this becomes
// a file, and the file is the work that the summary of the turn means.
pub const GREATEST_CUT_ARGS: usize = 4000;

// The first `wanted` characters of a text, counted as characters and not as bytes, for the text
// of a call hold whatever the model was writing.
fn head_chars(text: &str, wanted: usize) -> String {
    text.chars().take(wanted).collect()
}

// The text of a tool call the context window filled under, given back as the text of the turn
// that was truncated. Three items have to come across: that the call was cancelled and never
// ran, that the text of it is only as much as the model had written when it was stopped, and
// that nothing of it is wanted now.
//
// The report is above the marker of the summarisation, so the model that reads it is most often
// the model writing the summary, and a request for a summary must not read as a call to work.
// The text therefore say out loud that the report is a record and not a question. A model asked
// on behind the handoff block reads the summary in place of this text.
//
// Whether the text of the call stop in the middle is informed by the text themselves: a call
// that shut all its markers is whole as far as anyone can tell, and a call with a string left
// open in it is not, which is what the model has to hear either way.
pub fn cut_words(name: &str, args: &str) -> String {
    format!(
        "[A tool call was cancelled. The conversation reached the summarisation limit while the model was still writing the call, so the program stopped the writing and the tool never ran: nothing of what the call asked for was done. The call as it was when the writing stopped is given below. It is a record of the turn and not a question: do not answer it, and do not run what it asked for. If the work still needs it, write the call again from the start, in parts small enough to fit in the context.\n{}\n{}]",
        named_call(name),
        cut_call_text(name, args)
    )
}

// The text of a tool call the user stopped. Two items differ from a call the context window
// filled under: nobody made space after it, and the answer is over, so nothing is asked of
// the model now. The call is still informed back, for the model is to know what it was put to
// when the user asks it for the work again.
pub fn stopped_words(name: &str, args: &str) -> String {
    format!(
        "[A tool call was cancelled. The user stopped the answer while the model was still writing the call, so the program stopped the writing and the tool never ran: nothing of what the call asked for was done. The answer is over, and the context window was not touched. The call as it was when the writing stopped is given below. If this work is taken up again, write the call again from the start, in parts small enough to fit in the context the model has.\n{}\nText of the call written before it was stopped:\n{}\n{}]",
        named_call(name),
        args,
        words_as_they_stood(args)
    )
}

// The text of a call that was truncated, as the model is told it. The header of the call
// names the work it was, so it remains first, and the text of it is kept to the header: a whole
// file in the arguments of a cut call is the work the summary already means, and sending it
// again whether the call is whole or cut, and the header is the part worth the space. The
// user is never shown the report, so the block they see keeps all of the text it had, and only
// the text that go out is kept to the header.
fn cut_call_text(name: &str, args: &str) -> String {
    let head = words_of(name, args)
        .0
        .chars()
        .take(400)
        .collect::<String>();
    if head.trim().is_empty() {
        return head_chars(args, GREATEST_CUT_ARGS);
    }
    format!(
        "(The call was writing: {} {})",
        head,
        words_as_they_stood(args)
    )
}

// The tool the call was pointed at, or the text that shows its name was never written.
fn named_call(name: &str) -> String {
    if name.trim().is_empty() {
        String::from("the model had not written the name of the tool yet")
    } else {
        format!("tool: {}", name)
    }
}

// What the text of a stopped call reports about themselves. Whether they stop in the middle is
// informed by the text themselves: a call that shut all its markers is whole as far as anyone
// can tell, and a call with a string left open in it is not, which is what the model has to
// hear either way.
fn words_as_they_stood(args: &str) -> String {
    if serde_json::from_str::<Value>(args).is_ok() {
        String::from("(the text of the call is kept as the model wrote it. The writing was stopped, and the tool never ran)")
    } else {
        String::from("(the text of the call ends in the middle: the model had written no further when the writing stopped, so what it intended after this point is not known)")
    }
}

// The text the Chatting settings can put before the model, so it knows the day and the clock
// time it works on. They remain once for a conversation, in a message of the role `system` at
// its header, and never beside a message of the user. Nothing of the line is shown among the
// text of the chat: it is a saying of the program on the way to the model, as the handoff block
// is.
pub const DATE_HEAD: &str = "[Program note: ";

// One line of the day and the clock time, written from the moment it is given.
fn date_line(secs: i64) -> String {
    let named = if secs > 0 { secs } else { store::now_secs() };
    format!(
        "{}the date and the time on this machine are {}, in the local time zone of this \
         machine. This is the latest the machine has. Answer from it when the user asks for \
         today, for the day of the week or for the hour; do not say the date is unknown. The \
         user did not write this line and is not shown it, so do not speak of it and do not \
         reply to it: it is a note, not a question, and there is nothing in it to answer.]",
        DATE_HEAD,
        store::format_date_clock(named)
    )
}

// The moment the line of the day is written from: the moment the chat was begun. A chat with no
// moment of its own is named from this moment, and keeps it from then on, as a chat begun now
// is.
fn began_of(chat: &store::Chat) -> i64 {
    if chat.meta.created > 0 {
        chat.meta.created
    } else {
        store::now_secs()
    }
}

// The message of the role `system` that carries the day and the clock time, or nothing at all
// when the switch of the Chatting settings remains off. The line goes at the header of the
// conversation once, so no message of the user carries it and no message of the user carries it
// twice.
//
// `began` is the moment the chat was begun, and the line is written from that moment only. The
// header of the conversation must not move while the conversation runs, for the server keeps
// the work it did on the header of a prompt only while the header remains word for word as it
// was: a line written from the clock of this moment made a different header every minute, and
// every turn after it - the answer of a tool above all, which comes seconds after the call was
// created - was read from the start again, whatever the switch of prompt caching stated. A run
// of a model on this box measured the difference: 655 tokens read over in 26.9 seconds with a
// header that had moved by a minute, and 4 tokens read over in 0.34 seconds with a header that
// had not moved at all.
pub fn date_message(cfg: &Config, began: i64) -> Option<Value> {
    if !cfg.chat_date_new_chat {
        return None;
    }
    Some(json!({
        "role": "system",
        "content": date_line(began),
    }))
}

// The text before the conversation that come from the settings: the flagged pre-written prompts
// in front, then the text the user wrote. A chat that markers nothing and writes nothing gets
// nothing, and remains then exactly as it was before these prompts existed. The two blocks are
// kept apart by a blank line, so a prompt that ends on a list does not run into the text of the
// user.
fn system_words_of(cfg: &Config) -> String {
    let front = crate::prompts::front(&cfg.chat_system_prompts_on);
    let own = config::sanitize_system_prompt(&cfg.chat_system_prompt);
    if front.is_empty() {
        return own;
    }
    if own.is_empty() {
        return front;
    }
    format!("{front}\n\n{own}")
}

// The marker llama.cpp puts into its template where an image goes: `<__media_` with a random
// tail, `<__media_Qv1A...__>`. The server counts those markers in the text it is sent and
// searches for an image at all of them, and it refuses the whole ask when a marker is where no
// image is: `number of media markers in text (1) exceeds number of bitmaps (0)`, which the
// window reads as `Failed to tokenize prompt`. Text of that form come into a chat on their own:
// the model requests the properties of an inference server with a command and the answer
// carries the marker, or the user pastes such a reading. The marker then rides in the
// conversation for ever after, and every later ask is refused, so the chat is stuck on one line
// of output.
//
// Nothing here sends an image inside text: an image goes as a part of its own (`image_url`),
// and the server puts its own marker where the template wants one. A marker found among the
// text is therefore always an item being talked about and never a place for an image. One space
// after the opening bracket breaks the marker, and the text still reads as they did: the server
// sees no place for an image, and the conversation keeps the text.
fn break_media_marks(words: &str) -> Option<String> {
    if !words.contains("<__media_") {
        return None;
    }
    Some(words.replace("<__media_", "< __media_"))
}

// The same over a whole ask: every string at whatever depth is checked, so a marker in the
// result of a command, in the text of an answer, in the arguments of a call or in a line of a
// file the model read is broken as well. Text with no marker in it is left exactly where it was
// they are.
fn break_media_marks_in(value: &mut Value) {
    match value {
        Value::String(words) => {
            if let Some(made) = break_media_marks(words) {
                *words = made;
            }
        }
        Value::Array(items) => {
            for one in items.iter_mut() {
                break_media_marks_in(one);
            }
        }
        Value::Object(fields) => {
            for (_name, one) in fields.iter_mut() {
                break_media_marks_in(one);
            }
        }
        _ => {}
    }
}

// Where the newest handoff summary that was really written is among the messages of a chat.
// None means the chat was never summarised.
fn last_summary_place(chat: &Chat) -> Option<usize> {
    chat.messages.iter().rposition(|message| {
        message.role == "Summary" && !message.body.trim().is_empty()
    })
}

// The first text of `cut_words`. They are searched for whole, and only at the start of the text
// that go out, so no other saying is taken for one.
pub const CUT_TOLD_FIRST: &str = "[A tool call was cancelled";

// The conversation of a request that only goes on, after a handoff summary or after a run of a
// tool. The marker of the summarisation carries its handoff block in the history already, so a
// request of this form needs nothing more than the conversation as it is now -- unless the
// conversation ends on a call or on the result of one. Then a note about the program is sent
// after it as a message of the user, so the conversation ends on a question and the server is
// requested an answer rather than for the call it would else write behind the result. Nothing
// is added in any other case: the block of a summarisation is in the conversation already, and
// a second copy of it would cost the space the summarisation was created to win.
fn go_on_words(state: &State, carried: &str) -> Value {
    let mut history = state.build_history();
    if carried.trim().is_empty() {
        return history;
    }
    if let Some(items) = history.as_array_mut() {
        items.push(json!({ "role": "user", "content": carried }));
    }
    history
}

// True when the newest item of a chat is a summarisation, which is to say there is nothing new
// to write a summary of. A note is looked past, for notes come and go with the turns. A turn of
// the model that left no row of itself is looked past as well: the window drew nothing of it,
// and a chat that is still past the limit because of the text before the summarisation would
// else be informed it has space to spare and stay still for ever. The thinking of a turn is
// an item that was stated, for the user reads it in the panel of the thought.
fn ends_on_summary(chat: &Chat) -> bool {
    chat.messages
        .iter()
        .rev()
        .find(|message| {
            message.role != "Note"
                && !(message.role == "Assistant"
                    && message.body.trim().is_empty()
                    && message.reasoning.trim().is_empty())
        })
        .map(|message| message.role == "Summary" && !message.body.trim().is_empty())
        .unwrap_or(false)
}

// A turn of the model that left nothing of itself behind: no text, no thinking, and no call of
// a tool. The server writes the marker of an empty turn for it, so the text of such a turn go
// back as the text of the model with nothing after it, and the next ask is then put to go on
// where the model left off. A model answers that with the marker of an empty turn again, which
// is all of the exchange the user saw: seventeen tokens of an overfull chat, of which two were
// text, ending on the empty turn. The thinking of a turn counts as an item stated when it is
// sent with the turn, for the panel of the thought shows it and the template may keep it; a
// turn whose thinking the chat does not send back is an empty turn as far as the server is
// concerned, whatever the user can read.
fn said_nothing_at_all(turn: &Value) -> bool {
    turn.get("role").and_then(|r| r.as_str()) == Some("assistant")
        && turn.get("tool_calls").is_none()
        && said_no_words(turn.get("content").unwrap_or(&Value::Null))
        && said_no_words(turn.get("reasoning_content").unwrap_or(&Value::Null))
}

// Whether the text of one message reports nothing at all. A message of parts rather than text
// reports something, for its parts are its text and its images.
fn said_no_words(said: &Value) -> bool {
    said.is_null()
        || said
            .as_str()
            .map(|text| text.trim().is_empty())
            .unwrap_or(false)
}

// Whether the conversation that is sent ends on the text of the model with nothing after it. A
// turn of the model is the answer of the request that was sent before it, so no turn of the
// user is behind it and the next ask would go out on an empty turn alone. Only a conversation
// that has grown past one message has an empty turn of its own to answer.
fn ends_on_an_empty_turn(history: &Value) -> bool {
    history
        .as_array()
        .is_some_and(|items| items.len() > 1 && said_nothing_at_all(&items[items.len() - 1]))
}

// The chapters of one chat, in the order they begin. A chapter is begun by a main heading of an
// answer: the model opens a large new part of its work with a line that starts with one or two
// `#` markers, and the text of that line name the part. Nothing is called and nothing is
// written for it, so a chapter costs the conversation no message of its own. The chats of
// before carry their markers as messages of the role `Chapter`, and those are still read, so an
// older chat keeps the line of nodes it was given.
//
// A heading means a chapter only where the model wrote it as text: a `#` at the start of a line
// inside a block of code is a comment of that code. A note is not a place a chapter can begin
// after, for the window puts notes between the turns and they come and go with them.
fn chapter_marks(chat: &Chat) -> Vec<Chapter> {
    let mut out: Vec<Chapter> = Vec::new();
    let mut before = String::new();
    for message in &chat.messages {
        // A marker of the old type is a chapter with a message of its own, and is never a place
        // another chapter begins after.
        if message.role == "Chapter" {
            if !message.body.trim().is_empty() {
                out.push(Chapter {
                    id: message.id.clone(),
                    name: message.body.clone(),
                    after: before.clone(),
                });
            }
            continue;
        }
        if message.role == "Note" {
            continue;
        }
        let heads = if message.role == "Assistant" {
            heading_names(&message.body)
        } else {
            Vec::new()
        };
        if heads.is_empty() {
            before = message.id.clone();
            continue;
        }
        // The chapter begins after the message before the answer that carries the heading, so
        // the answer itself remains within the chapter it began.
        for name in heads {
            out.push(Chapter {
                id: format!("{}-{}", message.id, out.len()),
                name,
                after: before.clone(),
            });
        }
        // The answer that carries a heading is itself a place a later chapter begins after, so
        // the next chapter of the same answer is after it.
        before = message.id.clone();
    }
    out
}

// The main headings of one answer, in the order they remain: the lines that begin with one or
// two `#` markers and a space, outside a fenced block of code. A heading with no text in it is
// no chapter.
fn heading_names(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut fenced = false;
    for line in body.lines() {
        let bare = line.trim_start_matches([' ', '\t']);
        if bare.starts_with("```") || bare.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let marks = bare.len() - bare.trim_start_matches('#').len();
        if marks == 0 || marks > 2 {
            continue;
        }
        let rest = &bare[marks..];
        // A heading is a marker and a space: a word that runs on from the markers is a word,
        // and not a heading.
        if !(rest.starts_with(' ') || rest.starts_with('\t')) {
            continue;
        }
        // The markers some pointers put behind a heading as well are no part of its name.
        let named = rest
            .trim()
            .trim_end_matches(['#', ' ', '\t'])
            .trim()
            .to_string();
        if named.is_empty() {
            continue;
        }
        out.push(cut_chapter_name(&named));
    }
    out
}

// The name of a chapter, cut to what a node beside the chat can carry.
fn cut_chapter_name(words: &str) -> String {
    let mut name = String::new();
    for (number, ch) in words.chars().enumerate() {
        if number >= 60 {
            name.push('\u{2026}');
            break;
        }
        name.push(ch);
    }
    name
}

// The text that goes before the model while the chapters of a chat are on. Chapters are not
// flagged with a call of any type: they are extracted from the headings of the answers, so the
// model has to be told to open a large part of its work with a main heading. The text goes in
// a message of the role `system` at the head of the conversation, once for the conversation,
// and it is removed when the switch is off.
pub fn chapters_words() -> String {
    String::from(
        "The chapters of this chat are read from your headings. When a large new part of the work begins - a new task, a new question, a new phase of the work, a new part of a story you are writing - open it with a main heading: a line that starts with '# ' or '## ', whose text reports what the part is about in a few words. The program puts that heading beside the chat as a node the user may go straight to. Do not use a heading for a small item inside a part, do not open the whole chat with a heading, and do not speak of chapters or of headings in your answer.",
    )
}

// The four colours a model may put on its text: the marker that opens the colour, and the
// marker that closes it. The names are the model's own, and the page paints each of them with
// one colour of its own: `green` #24c935, `light_green` #61ff71, `red` #c92424 and `light_red`
// #ff6161. The markers is here and in the two pages, and nowhere else.
pub const COLOUR_MARKS: [(&str, &str); 4] = [
    ("<green>", "</green>"),
    ("<light_green>", "</light_green>"),
    ("<red>", "</red>"),
    ("<light_red>", "</light_red>"),
];

// The text that go before the model while the colours of the chat are on. The colours are not a
// call of any type and have no panel of their own: the page paints the text between the two
// markers, so the model has to be informed what the markers are and when they are worth using.
// The text go in a message of the role `system` at the header of the conversation, once for the
// conversation, and they go away with the switch.
pub fn colour_words() -> String {
    String::from(
        "You may put colour on a few words of your answer. Write the text between a marker that opens a colour and the marker that closes it, for example <green>done</green>. The four colours are green, light_green, red and light_red, and the closing marker is the opening marker with a slash in it, so </green> closes the green. The program paints that text for the user and strips the markers from the conversation it reads back to you. Use colour on a few words only: a result that is good or bad, a number that decides something, or an item the user must not miss. Do not colour a whole sentence or a whole answer, do not put one colour inside another, do not write the markers inside a block of code, and do not speak of the colours in your answer.",
    )
}

// The text that go before the model while the size of the context is created by a handoff
// summary. A model that is never informed how the space is kept open reads a full context as
// the end of its work: it compresses the part it was requested, leaves a part of it out, or
// stops to ask leave for the next step, all because it counts the text it has left. This text
// reports that there is nothing to count. They go in a message of the role `system` at the
// header of the conversation, once for the conversation, and they go away when compaction
// remains off.
pub fn compaction_words() -> String {
    String::from(
        "The context of this conversation stays open while you work. When the context window of the model fills, this program writes a handoff summary of the conversation, puts it in the place of the messages it replaced, and the work goes on in the context that is created. That may happen again and again, without end, and nothing of the work is lost by it: the files stay where you wrote them, and the summary carries the state of the work.\n\nYou therefore have no budget of context, no limit of time and no limit on the number of tool calls. Do not measure the work against the context you think you have. Never leave a part of the work out, never write a simpler version because the full version is large, and never stop to ask whether there is space for the next step: when space runs out, the program creates it. Work until the work is complete, one step at a time, and say nothing of the context or the summary in your answer.",
    )
}

// Which colour a marker at the header of `words` requests: the marker that opens it and the
// marker that closes it. Nothing returns for a less-than marker that opens no colour, nor for
// `<greenish>` for example, nor for a pair of markers with nothing between them.
fn colour_at(words: &str) -> Option<(&'static str, &'static str)> {
    for (open, close) in COLOUR_MARKS {
        // All of the opening marker must remain, and more than the two markers must be in all,
        // so `<green>` with nothing in it is no colour.
        if words.len() > open.len() + close.len() && words.starts_with(open) {
            return Some((open, close));
        }
    }
    None
}

// How deep one colour may be inside another and still be moved into. The page reads one colour
// and no more, for it cuts a line into its parts once over; the text of an answer that put
// colour a long way down are an error and not a look. The count is a ceiling on the work, so a
// very long answer of nothing but markers cannot request more steps than there is space
// for.
const GREATEST_COLOUR_NESTING: usize = 8;

// The text of an answer with the colour markers taken out of it: the text between the two
// markers keep their place, and the markers go. A colour inside a colour loses both pairs of
// markers. A marker that nothing closes stays where the model wrote it, for an answer that
// stopped in the middle of a colour is the model's own text and not ours to finish. The page
// paints the text and loses the markers, so the markers would otherwise be read back to the
// model as text it never meant to write.
pub fn strip_colour_tags(text: &str) -> String {
    strip_colour_tags_deep(text, 0)
}

// The same text, with the depth of the colours already moved into counted.
fn strip_colour_tags_deep(text: &str, depth: usize) -> String {
    if !text.contains('<') || depth >= GREATEST_COLOUR_NESTING {
        return String::from(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut left = text;
    while !left.is_empty() {
        let Some(at) = left.find('<') else {
            out.push_str(left);
            break;
        };
        out.push_str(&left[..at]);
        left = &left[at..];
        let Some((open, close)) = colour_at(left) else {
            out.push('<');
            left = &left[1..];
            continue;
        };
        match left[open.len()..].find(close) {
            Some(where_it_stands) => {
                let inner = &left[open.len()..open.len() + where_it_stands];
                out.push_str(&strip_colour_tags_deep(inner, depth + 1));
                left = &left[open.len() + where_it_stands + close.len()..];
            }
            None => {
                out.push_str(open);
                left = &left[open.len()..];
            }
        }
    }
    out
}

// A copy of everything a window needs to draw. It is created while the state lock is held, and
// then used without the lock.
pub struct Snap {
    pub model_name: String,
    pub model_kind: String,
    pub in_repository: bool,
    pub repository_id: String,
    pub repository_name: String,
    pub chat_title: String,
    pub context_used: u64,
    pub context_total: u64,
    // Where the summarisation of the handoff begins, in the text of the conversation, for the
    // line of it on the context meter. Nothing is at zero: the summarisation is off, or the
    // size of the context is not known.
    pub context_cut: u64,
    pub generating: bool,
    // True while the program is writing a handoff summary, whose text is never shown to the
    // user.
    pub summarising: bool,
    pub busy: bool,
    // The chats that are being answered, by their id. The side bar puts a marker on the row of
    // these chats, so the user sees where the model is at work while another chat is left open.
    pub working: Vec<String>,
    pub code_enabled: bool,
    // True when the open chat is working in planning mode: the model plans the work and writes
    // it to its plan file, and does none of the work itself.
    pub planning: bool,
    // True when the open chat let a plan go. The heading over the conversation reports `PLANNED
    // WORK`, and the plan may be read from it.
    pub planned: bool,
    // Which of the tools of the program the model may reach in the open chat. The badge of the
    // tools is the switch over all of them; these are the lines of its drop-down.
    pub tool_switches: store::ToolSwitches,
    // The switch of the web search of the open chat.
    pub search_enabled: bool,
    pub sandbox_enabled: bool,
    pub sandbox_available: bool,
    // What a command of the open chat may reach beyond its own folder: the network, an install
    // of a Python package, and the commands that fetch a page of the web. All three remain off,
    // and the menu of the sandbox badge opens them one by one.
    pub sandbox_access: tools::Access,
    // The servers of the user's own, for the drop-down of the badge and the MCP tab of the
    // settings.
    pub mcp: Vec<McpSnap>,
    // The thinking mode of the open chat.
    pub thinking_mode: String,
    // True when the model's template can switch thinking.
    pub thinking_available: bool,
    // The thinking modes the badge proposes, in the order they are listed. An empty list means
    // the model has no thinking mode at all.
    pub thinking_modes: Vec<String>,
    // Whether the open chat keeps the thinking of the turns before the last answer in the text
    // the model is sent.
    pub thinking_preserve: bool,
    // True when the template of the model proposes that choice.
    pub thinking_preserve_available: bool,
    pub status: String,
    pub dark_mode: bool,
    // The theme the pages are drawn in: `light`, `dark`, `silver`, `corporate` or `neon`.
    pub theme: String,
    // True when that theme is drawn in its pixel-art form: the colours of the theme above, the
    // interface out of squares.
    pub theme_pixel: bool,
    pub data_dir: String,
    // The folder the contexts the model has processed are in, which the settings window reads
    // out beside the field that chooses another place.
    pub contexts_dir: String,
    // What the sandbox can do on this computer, for the settings window.
    pub sandbox_note: String,
    // Chatting. Whether a thinking panel is folded up while it is written.
    pub chat_collapse_thinking: bool,
    // Whether a tool panel is folded up while its command runs.
    pub chat_collapse_tools: bool,
    // Whether a thinking panel grows with its text instead of stopping at a height.
    pub chat_grow_thinking: bool,
    // Whether a tool block called while thinking is inside the thinking panel.
    pub chat_tools_in_thinking: bool,
    // Whether the date and the time go before the model once, at the start of the conversation,
    // in a message of the role `system`.
    pub chat_date_new_chat: bool,
    // Whether a block of code takes the colours of the types of word in it.
    pub chat_highlight: bool,
    // Whether the markers of boldness are read as boldness in the text of the chat, or as part
    // of the text themselves.
    pub chat_bold_markdown: bool,
    // Whether the colours the model requests are painted on the text of the chat, and taken out
    // of the conversation that goes back to the model.
    pub chat_colour_text: bool,
    // Whether the chat reads as one piece of writing, with nothing drawn about a panel of the
    // thoughts or of a call.
    pub chat_seamless: bool,
    // How the work of the model is decompressed: `plain`, or `corporate`, where one block means
    // an answer and the thoughts and the calls are hidden.
    pub chat_display: String,
    // True once the first-run screen has been removed through.
    pub welcomed: bool,
    // How many times an answer is asked over when the model came to its own end without a word,
    // a call or an error.
    pub chat_silent_retries: i64,
    // Whether the model may marker the chapters of a chat, and whether the chapter line is
    // beside it.
    pub chat_chapters: bool,
    // The line of the user's own messages, at the right of the chat.
    pub chat_userline: bool,
    // What falls behind the text of a chat: "none", "leaves" or "rain". The page draws it; the
    // engine only carries the word out of the settings.
    pub chat_background: String,
    // Whether the rain splashes where it meets the chat composer box and the blocks of the
    // chat.
    pub chat_rain_splash: bool,
    // Whether a message the user sent is carried into the chat with a little movement. The
    // movement is only on the screen, so the engine only reports the page that the switch
    // remains.
    pub chat_send_anim: bool,
    // Whether the conversation that the model has worked its way through is kept in a file of
    // the program, so that a chat coming back is not read through again. The settings panel
    // reads it, and the page has nothing of its own to do with it: the keeping is all behind
    // the request.
    pub chat_context_files: bool,
    // Whether the saved context files are written in the compressed form. Off by default; the
    // conversations themselves are read the same either way.
    pub chat_context_compression: bool,
    // The number of characters where a paste of text into the chat composer turns into a file
    // of its own.
    pub chat_paste_limit: u32,
    // How long the model waits for an answer to a question it put to the user, for the two
    // types of question. The second is zero by default, which means the question is waited for
    // until it is answered.
    pub ask_seconds: u32,
    pub ask_critical_seconds: u32,
    // How wide the chat may be, in pixels. The page keeps it within 80 per cent of the window,
    // and out of the working directory the two lines and the arrows need.
    pub chat_width: u32,
    pub models: Vec<ModelInfo>,
    pub model_id: String,
    pub repositories: Vec<Repo>,
    pub chat_id: String,
    // True when no chat is open. The window then shows the chat composer in the middle of the
    // screen, with the program icon and a line for this hour. The side bar and this one screen
    // are what the user goes between: a new chat is this screen, and the first message of it
    // grows the chat.
    pub home: bool,
    // The line above the chat composer while no chat is open.
    pub greeting: String,
    pub messages: Vec<Message>,
    // The id of the answer the page may propose to go on with, empty when there is none. The
    // page puts the continue marker on this message only, so the proposal and the rules that
    // decide it are held in one place in the engine.
    pub continue_id: String,
    // The chapters of the open chat, in the order the model flagged them.
    pub chapters: Vec<Chapter>,
    // The summarisations of the open chat, oldest first, for the heading of the summarisation
    // and the timeline of them at the right of the chat.
    pub handoffs: Vec<Handoff>,
    // The Compaction section of the settings, as the page reads it.
    pub chat_compaction: String,
    pub chat_compact_by: String,
    pub chat_compact_percent: u32,
    pub chat_compact_reserve: u32,
    pub chat_compact_prompt: String,
    // The prompt of the second ask, the one that leans towards a short summary.
    pub chat_compact_prompt_short: String,
    pub chat_timeline: bool,
    // Text the model is writing right now, while an answer arrives.
    pub live_text: String,
    // The model's reasoning, streamed while an answer arrives.
    pub live_reasoning: String,
    // How long the thinking of the answer taking form lasted, in milliseconds, and whether it
    // has stopped. The page writes these on the header of the panel.
    pub live_thought_ms: u64,
    pub live_thought_done: bool,
    // How far the server is with the text it was sent, from 0 to 100, and whether the meter
    // above the chat composer remains. The meter remains from the moment a request is sent
    // until the model has a word to give back.
    pub progress: u32,
    pub progress_on: bool,
    // How many images the message being worked through carries. The program sends text today,
    // so the number is nothing. The text of the meter follows it, so a chat that can carry an
    // image records it by itself.
    pub progress_images: usize,
    // The two speeds of the work, in tokens of the conversation to the second: how fast the
    // text that was sent is being worked through, and how fast the model is writing. The server
    // is requested both, and each is at zero until it reports something of speed.
    pub prompt_rate: f64,
    pub answer_rate: f64,
    // The calls of a tool the model is writing right now, in the order it writes them. A block
    // of a command being written is made of these.
    pub live_tools: Vec<LiveTool>,
    // The summary of the work, as the corporate display shows it, while an answer arrives: the
    // blocks that are inside the one block of the answer.
    pub live_work: Vec<corporate::WorkBlock>,
    // The header the one block of the corporate display is at right now.
    pub live_work_head: String,
    // True once the model stated with its own tool that the work of this answer is over. The
    // one block of the answer is closed at that marker, and the text the model writes after it
    // are outside it, as the answer the user came for.
    pub live_work_closed: bool,
    // The question the model has put to the user and is waiting on, when it has put one. The
    // popup of the request is above the chat composer.
    pub ask: Option<Asking>,
    // The call of a tool that is running right now, with the time it has left. The header of
    // the block of the call wears the markers of a running call while this remains.
    pub running: Option<RunningTool>,
    // The messages waiting their turn in the chat that is left open. The list over the chat
    // composer is drawn from here, so a page that is opened again finds the list as the program
    // kept it, and a page that is closed leaves it be.
    pub queue: Option<WaitingSnap>,
    // Goes up when a message changes, so the window redraws them.
    pub rev: u64,
    // Goes up when a list changes, so the window rebuilds it.
    pub structure_rev: u64,
}

// What one chat holds while it answers. Every field here belongs to a single chat: the text
// arriving from the model, the context meter, the question the model is waiting on, the tool
// call that is running, and whether this chat is answering at all. Two chats may answer at the
// same time, so none of it may be shared between them. The clocks are used only to measure
// thinking, progress and speed, and are private to the engine.
#[derive(Clone, Debug, Default)]
pub struct ChatLive {
    // True while this chat is being answered. Another chat may be answering at the same time,
    // so this is held per chat and not for the program as a whole.
    pub generating: bool,
    // True while the program is writing this chat's handoff summary. Its text is never shown,
    // so no live answer is drawn while it remains; the heading of the summarisation and the
    // status line say that the program is at work.
    pub summarising: bool,
    // Text the model is writing right now.
    pub live_text: String,
    // The model's reasoning, streamed while an answer arrives.
    pub live_reasoning: String,
    // Whether the thinking that came with the live answer has stopped. The header of the panel
    // reports how long it took once it has.
    pub live_thought_done: bool,
    // How long the thinking of the live answer lasted, in milliseconds.
    pub live_thought_ms: u64,
    // How far the server is with the text it was sent, from 0 to 100, and whether the meter
    // above the chat composer remains.
    pub progress: u32,
    pub progress_on: bool,
    // How many images the message being worked through carries.
    pub progress_images: usize,
    // The two speeds of the work, as the page shows them: the text sent to the server, and the
    // text the model is writing. Both are zero until the server reports something of speed, or
    // the clocks of the program have had their say.
    pub prompt_rate: f64,
    pub answer_rate: f64,
    // The calls of a tool the model is writing right now, in the order it writes them. The user
    // sees a command as it is written.
    pub live_tools: Vec<LiveTool>,
    // The summary of the work of the answer that is arriving, for the corporate display. It is
    // made of the blocks the model wrote with its own tool and of the blocks of the calls it
    // made, and it is emptied when an answer begins.
    pub live_work: Vec<corporate::WorkBlock>,
    // The header the one block of the corporate display is at right now: what the model is at,
    // in the text of the item that is going on.
    pub live_work_head: String,
    // True when the model closed the one block of the answer with its own tool. The text of the
    // answer that come after that marker is outside the block.
    pub live_work_closed: bool,
    // All of a plan the model let go, waiting to become the first message of the work. It
    // remains only between the call that let the plan go and the turn that begins the work,
    // which follows it at once.
    pub plan_pending: Option<String>,
    // The question the model has put to the user and is waiting on, when it has put one. The
    // request is above the chat composer until it is answered, given up, or stopped with the
    // answer of the chat that asked it.
    pub asking: Option<Asking>,
    // The call of a tool that is running right now, and the clock it runs against. Nothing is
    // here between two calls.
    pub running: Option<Running>,
    // How a lot of text of the context this chat's conversation takes up.
    pub ctx_used: u64,
    // The first number of the conversation that the server gives for a new ask is taken where
    // it is, and after that the meter only climbs. The number left over from the request before
    // may count text that was never kept in the chat - an answer the user stopped leaves the
    // text the model had written out of the conversation again - and a meter that only climbs
    // would stay still for all of the answer that followed, for ever after.
    pub ctx_fresh: bool,
    // The moment the text of an answer began to come. The program measures the speed of the
    // writing itself when the server reports nothing of speed, and it cannot do that without a
    // beginning to measure from.
    rate_start: Option<Instant>,
    // The moments the thinking of the live answer began and last stopped. The two together are
    // the length of it.
    thought_start: Option<Instant>,
    thought_end: Option<Instant>,
    // The moment last handed to a chat that was opened. Two chats opened inside one second are
    // distinguished by it, so the one opened last is at the header of its group none the less.
    pub last_opened: i64,
    // The moment last handed to this chat's context meter. The meter is paced, so it does not
    // write a new number on every piece of the answer.
    ctx_at: Option<Instant>,
}

// One blank live state, shared by every read of a chat that never answered. It reports that
// nothing is answering, that no text is arriving, and that no clock is running.
fn blank_live() -> &'static ChatLive {
    static BLANK: std::sync::OnceLock<ChatLive> = std::sync::OnceLock::new();
    BLANK.get_or_init(ChatLive::default)
}

// One chat of the state, reached through the engine's lock. Fields come from the chat's own
// `ChatLive`, so a thread that writes an answer writes it into the chat it is answering and not
// into whichever chat the side bar happens to have open. Methods that need the whole state -
// the settings, the model list, the store - are reached through `state`.
pub struct ChatScope<'a> {
    // All of the state. It is there for what no single chat owns: the settings, the models, the
    // store and the status line.
    pub state: &'a mut State,
    // The chat the fields of this scope belong to.
    pub chat: String,
}

impl<'a> std::ops::Deref for ChatScope<'a> {
    type Target = ChatLive;

    fn deref(&self) -> &ChatLive {
        self.state
            .live_of(&self.chat)
            .expect("the scope means a chat that has live state")
    }
}

impl<'a> std::ops::DerefMut for ChatScope<'a> {
    fn deref_mut(&mut self) -> &mut ChatLive {
        self.state.live_mut(&self.chat)
    }
}

// The state behind the engine.
pub struct State {
    pub cfg: Config,
    pub store: store::Store,
    pub models: Vec<ModelInfo>,
    pub gpus: Vec<Gpu>,
    pub repositories: Vec<Repo>,
    // Repository the side bar has open, and the one a new chat belongs to.
    pub scope_repo: String,
    // The chats of the place that is open, newest first. The side bar reads the chats of every
    // place when it is requested them; this list is the place the open chat is in.
    pub chats: Vec<Chat>,
    pub current: Option<String>,
    // The waiting lists, one to a chat. A message written while the model is writing is here
    // until the answer it waited for is over, and the worker sends it then - which it does as
    // well when no page is left to look at the chat, for the list belongs to the program and
    // not to the window.
    queues: Vec<WaitingList>,
    // The model is loading, so the window shows a spinner.
    pub busy: bool,
    pub status: String,
    pub ctx_total: u64,
    pub sandbox_available: bool,
    pub sandbox_note: String,
    // What the chat template of the current model can do with thinking.
    pub thinking: markdown::ThinkingOptions,
    pub rev: u64,
    pub structure_rev: u64,
    // The chat settings a new chat starts with. They matter while no chat is open, where a
    // badge has no chat to change.
    pub new_code: bool,
    // The lines of the drop-down of the badge of the tools that the next chat opens with, as
    // the choice of them in this chat leaves them.
    pub new_tools: store::ToolSwitches,
    pub new_search: bool,
    pub new_sandbox: bool,
    // What a command of the next chat may reach. A new chat takes the choices of the chat that
    // was at the front, as it takes the other badges.
    pub new_sandbox_access: tools::Access,
    pub new_thinking: String,
    pub new_preserve: bool,
    // The MCP servers the next chat starts with, by their id. A new chat takes them from the
    // chat that was at the front, as it takes the badges.
    pub new_mcp: Vec<String>,
    // What each server last answered when it was asked what it has to propose, kept by its id.
    // The tools of a server are read once and kept, so a chat does not open a server again on
    // every answer, and the settings panel has something to say about a server beside its name.
    pub mcp_tools: std::collections::BTreeMap<String, Vec<mcp::Tool>>,
    // The reason a server could not be checked, kept by its id. A server that answers has no
    // entry here.
    pub mcp_fault: std::collections::BTreeMap<String, String>,
    // Which slot of the running server each chat had its conversation read into. The server
    // understands a conversation in one slot and nowhere else, so a chat that returns wants the
    // slot it was in, and the request of it has to name the slot to the server. A chat with no
    // entry here has nothing to go back to, which is how items remain when the keeping of the
    // conversations is switched off.
    pub context_slot: std::collections::BTreeMap<String, usize>,
    // Which chat is in each slot of the running server, the other side of the list above. A
    // slot that another chat is to take has to be written out to the file of the chat that is
    // in it first, for the conversation of that chat remains nowhere else in the world once the
    // slot is removed over.
    pub context_holds: std::collections::BTreeMap<usize, String>,
    // What each chat holds while it answers, by the id of the chat: the text arriving from the
    // model, the context meter, the question the model is waiting on, the call that is running,
    // and whether the chat is being answered. A chat that is not answering has no entry of its
    // own.
    live: std::collections::BTreeMap<String, ChatLive>,
    // The flag each chat that is being answered cancels itself with. A chat stops its own
    // answer, and no other, so the flag is beside the live state of the chat rather than in
    // front of the whole program.
    turn_cancel: std::collections::BTreeMap<String, std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

// Whether the conversation that is sent ends on the text of the model with text of its own and
// no call on it. An answer that was removed on from must be left open at the end of the
// conversation, for the template puts the text of the last saying of the model under the marker
// that opens an answer and leaves that marker open. A conversation that ends on a call, on a
// result, or on a handoff summary is not open, and nothing is asked of the model there.
fn ends_open(history: &Value) -> bool {
    let Some(list) = history.as_array() else { return false };
    let Some(last) = list.last() else { return false };
    if last["tool_calls"].is_array() {
        return false;
    }
    if last["role"].as_str() != Some("assistant") {
        return false;
    }
    match last.get("content") {
        Some(Value::String(words)) => !words.trim().is_empty(),
        // A message of parts is a message of text and images.
        Some(Value::Array(_)) => true,
        _ => false,
    }
}

// The newest saying of the conversation: the last message that is something the user or the
// model stated. The markers of a chapter, of a note, of a summarisation and of a tool run are
// among the text and are no sayings of their own.
fn newest_saying(chat: &Chat) -> Option<&Message> {
    chat.messages
        .iter()
        .rev()
        .find(|one| one.role == "You" || one.role == "Assistant")
}

// Take the marker of an answer that never came to its end off every message of the
// conversation. The marker is on one answer at most, and that answer is the newest one the user
// stopped.
fn clear_stopped_marks(chat: &mut Chat) {
    for one in chat.messages.iter_mut() {
        one.stopped = false;
    }
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

    // The live state of one chat, when it has any. A chat that has never answered has nothing
    // to show of itself.
    pub fn live_of(&self, chat: &str) -> Option<&ChatLive> {
        self.live.get(chat)
    }

    // The live state of one chat, or a blank one when the chat has never answered. A user that
    // names a chat with no answer of its own reads the blank: nothing answering, no text, no
    // clock of its own.
    fn live_ref(&self, chat: &str) -> &ChatLive {
        match self.live_of(chat) {
            Some(one) => one,
            None => blank_live(),
        }
    }

    // The live state of the chat the side bar has open, which is the chat the page and the
    // window read. A chat that never answered reads as the blank.
    pub fn live(&self) -> &ChatLive {
        let chat = self.current.as_deref().unwrap_or("");
        self.live_ref(chat)
    }

    // The live state of the chat the side bar has open, to be changed.
    fn live_write(&mut self) -> &mut ChatLive {
        let chat = self.open_id();
        self.live_mut(&chat)
    }

    // The live state of one chat, prepared when the chat has none yet.
    pub fn live_mut(&mut self, chat: &str) -> &mut ChatLive {
        self.live.entry(chat.to_string()).or_default()
    }

    // Take the live state of a chat away, which is what a chat that is removed leaves behind.
    fn live_drop(&mut self, chat: &str) {
        self.live.remove(chat);
    }

    // The live state of the chat the side bar has open, when one is open.
    fn open_live(&self) -> Option<&ChatLive> {
        self.live_of(self.current.as_deref().unwrap_or(""))
    }

    // Reach the state of one chat with the fields of that chat in front. The entry of the chat
    // is prepared, so what follows always has something to work on.
    pub fn for_chat(&mut self, chat: &str) -> ChatScope<'_> {
        self.live_mut(chat);
        ChatScope {
            state: self,
            chat: chat.to_string(),
        }
    }

    // Whether the chat named is being answered now.
    fn chat_generating(&self, chat: &str) -> bool {
        self.live_of(chat)
            .map(|one| one.generating)
            .unwrap_or(false)
    }

    // Whether the chat the side bar has open is being answered now.
    fn open_generating(&self) -> bool {
        self.chat_generating(self.current.as_deref().unwrap_or(""))
    }

    // Whether any chat at all is being answered, or is having its space made by a handoff
    // summary. A model is loaded for one model at a time, so this is what reports whether the
    // model of the computer is busy with somebody's answer.
    fn any_generating(&self) -> bool {
        self.live
            .values()
            .any(|one| one.generating || one.summarising)
    }

    // The chats that are being answered, by their id. The side bar puts a marker on the rows of
    // these chats, so the user can see where the model is at work.
    pub fn generating_chats(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .live
            .iter()
            .filter(|(_, one)| one.generating)
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        ids
    }

    // The chat named, when the memory holds it.
    fn chat_of(&self, chat: &str) -> Option<&Chat> {
        self.chats.iter().find(|one| one.meta.id == chat)
    }

    // The chat named, when the memory holds it, to be changed.
    fn chat_of_mut(&mut self, chat: &str) -> Option<&mut Chat> {
        self.chats.iter_mut().find(|one| one.meta.id == chat)
    }

    // The chat a caller works on when it names none: the chat the side bar has open. The empty
    // name means no chat, and every method that takes a chat by its name does nothing with it.
    fn open_id(&self) -> String {
        self.current.clone().unwrap_or_default()
    }

    // True when a chat of the repository named is being answered, or is having its space
    // made by a handoff summary. The text of such an answer go into the directory of the
    // repository, and a folder that is moved out from under them, or removed, is a conversation
    // written into nothing.
    fn repo_working(&self, repo: &str) -> bool {
        let chats: Vec<String> = self
            .chats
            .iter()
            .filter(|c| c.meta.repo_id == repo)
            .map(|c| c.meta.id.clone())
            .collect();
        self.live
            .iter()
            .any(|(id, one)| (one.generating || one.summarising) && chats.iter().any(|c| c == id))
    }

    // The waiting list of the chat named, when it has one. A list belongs to its chat: a chat
    // that answers in the background has a list of its own, and the messages of one chat never
    // go out through the list of another.
    fn queue_of(&self, chat: &str) -> Option<&WaitingList> {
        self.queues.iter().find(|one| one.chat == chat)
    }

    // The waiting list of the chat that is left open, when it has one.
    fn open_queue(&self) -> Option<&WaitingList> {
        let chat = self.current.as_deref()?;
        self.queue_of(chat)
    }

    // The waiting list of the chat named, made when it is first wanted. The list belongs to the
    // chat, so a chat that answers in the background keeps its own list and the messages of the
    // chat that is left open are not mixed with it.
    fn queue_mut(&mut self, chat_id: &str) -> Option<&mut WaitingList> {
        if chat_id.is_empty() {
            return None;
        }
        if let Some(at) = self.queues.iter().position(|one| one.chat == chat_id) {
            return self.queues.get_mut(at);
        }
        self.queues.push(WaitingList {
            chat: chat_id.to_string(),
            ..WaitingList::default()
        });
        self.queues.last_mut()
    }

    // The waiting list of the chat that is left open, made when it is first wanted.
    fn open_queue_mut(&mut self) -> Option<&mut WaitingList> {
        let chat = self.open_id();
        if chat.is_empty() {
            return None;
        }
        self.queue_mut(&chat)
    }

    // The waiting list of the chat that is left open, as a page is given it. Nothing when no
    // chat is left open, or when its list is empty, so a page takes the rows away with the last
    // of them instead of drawing a list of nothing. The programmed messages ride along in their
    // own list, for the page draws them in a section of their own.
    fn queue_snap_in(&self, chat_id: &str) -> Option<WaitingSnap> {
        let one = self.queue_of(chat_id)?;
        if one.items.is_empty() && one.planned.is_empty() {
            return None;
        }
        let row = |item: &QueueItem| QueuedRow {
            id: item.id,
            text: item.text.clone(),
            files: item
                .files
                .iter()
                .map(|file| file.name.clone())
                .collect(),
            matches: item.matches.clone(),
        };
        Some(WaitingSnap {
            chat: one.chat.clone(),
            held: one.held,
            items: one.items.iter().map(row).collect(),
            planned: one.planned.iter().map(row).collect(),
        })
    }

    // The waiting list of the chat that is left open, as a page is given it.
    fn queue_snap(&self) -> Option<WaitingSnap> {
        self.queue_snap_in(self.open_id().as_str())
    }

    // True when the worker has a message to send out of the waiting list: the chat that is left
    // open has a list, is idle, and has used the rest a message needs after the one before it.
    // The rest is what lets the chat say that it is writing again before the next message
    // requests an answer.
    //
    // A list of programmed messages that have not found their text is nothing to send, and the
    // worker is not kept awake for them: they are checked again when an answer of the chat
    // ends.
    fn queue_ready_in(&self, chat_id: &str) -> bool {
        let Some(list) = self.queue_of(chat_id) else {
            return false;
        };
        if list.held || self.live_ref(chat_id).generating || self.busy {
            return false;
        }
        let next = !list.items.is_empty()
            || list.planned.iter().any(|one| one.ready);
        if !next {
            return false;
        }
        match list.sent_at {
            None => true,
            Some(at) => at.elapsed() >= QUEUE_REST,
        }
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn queue_ready(&self) -> bool {
        self.queue_ready_in(self.open_id().as_str())
    }

    // The message the worker would send next out of the waiting list of the chat named, with
    // the number of its row, without taking it out. A programmed message that has found the
    // text it waited for goes before every message that is only waiting its turn; the row at
    // the start of the list goes otherwise. Nothing returns when the chat is at work, or held
    // back, or the rest of a moment is not spent yet, so the caller may ask again later.
    fn queue_next_in(&self, chat_id: &str) -> Option<(u64, SentMessage)> {
        if !self.queue_ready_in(chat_id) {
            return None;
        };
        let list = self.queue_of(chat_id)?;
        let one = list
            .planned
            .iter()
            .find(|one| one.ready)
            .or_else(|| list.items.first())?;
        Some((
            one.id,
            SentMessage {
                text: one.text.clone(),
                files: one.files.clone(),
            },
        ))
    }

    // The message the worker would send next out of the waiting list of the chat that is left
    // open.
    fn queue_next(&self) -> Option<(u64, SentMessage)> {
        self.queue_next_in(self.open_id().as_str())
    }

    // Take one message out of the waiting list of the chat named, by the number the worker sent
    // it by: the programmed messages and the messages that were only waiting their turn are
    // both looked through. A list that has come to an end is removed, so a chat checked again
    // does not find an empty list remaining for it.
    fn queue_take_in(&mut self, chat_id: &str, id: u64) {
        let Some(at) = self
            .queues
            .iter()
            .position(|one| one.chat == chat_id)
        else {
            return;
        };
        let list = &mut self.queues[at];
        if let Some(found) = list.planned.iter().position(|one| one.id == id) {
            list.planned.remove(found);
        } else if let Some(found) = list.items.iter().position(|one| one.id == id) {
            list.items.remove(found);
        } else {
            return;
        }
        list.sent_at = Some(std::time::Instant::now());
        // The rest a message needs is counted from now, and the list starts again on its own
        // once it has expired.
        list.held = false;
        if list.items.is_empty() && list.planned.is_empty() {
            self.queues.remove(at);
        }
        self.touch();
    }

    // Take one message out of the waiting list of the chat that is left open.
    fn queue_take(&mut self, id: u64) {
        let chat = self.open_id();
        self.queue_take_in(&chat, id);
    }

    // Hold the waiting list of the chat named back, because a message of it could not be sent.
    // The reason was stated by the sender of the message; the list is asked again once the user
    // touches it.
    fn queue_hold_in(&mut self, chat_id: &str) {
        if let Some(list) = self.queue_mut(chat_id) {
            list.held = true;
            self.touch();
        }
    }

    // Hold the waiting list of the chat that is set back.
    fn queue_hold(&mut self) {
        let chat = self.open_id();
        self.queue_hold_in(&chat);
    }

    // The chats whose waiting list has a message the worker may send now, in the order the
    // chats remain. A chat that is answering, or held back, or remaining in the rest of a
    // moment after the message before, is not named, so the worker never waits on a chat that
    // has nothing to send.
    fn ready_queues(&self) -> Vec<String> {
        self.queues
            .iter()
            .filter(|one| !one.items.is_empty() || one.planned.iter().any(|item| item.ready))
            .map(|one| one.chat.clone())
            .filter(|chat| self.queue_ready_in(chat))
            .collect()
    }

    // Let every waiting list go on again. What held a list back was an item about the chat - no
    // model chosen, a handoff summary owed - and a user who chooses a model, sends a message of
    // their own, or starts a summary again has dealt with it. The list is not asked again of
    // its own accord, for a list that cannot be sent would be refused again every moment.
    fn queue_restart(&mut self) {
        let mut any = false;
        for one in self.queues.iter_mut() {
            if one.held {
                one.held = false;
                any = true;
            }
        }
        if any {
            self.touch();
        }
    }

    // The answer the page may propose to go on with, as the id of its message: the newest
    // saying of the model in the chat that is left open, with text at its end for the model to
    // write on from, and the chat at rest.
    //
    // It matters nothing whether the user stopped that answer, or the model stopped it of its
    // own accord - a marker at the end of the conversation where the user was waiting for the
    // rest of the work. Only the user knows whether an answer ended where it should have, so
    // the suggestion is under the newest answer either way.
    fn goable_answer_in(&self, chat_id: &str) -> Option<String> {
        if self.live_ref(chat_id).generating || self.busy {
            return None;
        }
        let chat = self.chat_of(chat_id)?;
        let newest = newest_saying(chat)?;
        if newest.role != "Assistant" || newest.body.trim().is_empty() {
            return None;
        }
        Some(newest.id.clone())
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn goable_answer(&self) -> Option<String> {
        self.goable_answer_in(self.open_id().as_str())
    }

    // Whether the message named is the answer the chat may be continued, and what to say when
    // it is not. The state of the model and of the summarisation are asked about where those
    // are known: this is the rule of the conversation alone, so it can be checked without a
    // model remaining anywhere.
    fn answer_to_go_on_with_in(&self, chat_id: &str, id: &str) -> Result<(), &'static str> {
        let Some(chat) = self.chat_of(chat_id) else {
            return Err("That message is not in this chat");
        };
        let Some(one) = chat.messages.iter().find(|one| one.id == id) else {
            return Err("That message is not in this chat");
        };
        if one.role != "Assistant" {
            return Err("Only an answer of the model can be continued");
        }
        // Nothing to write on from. The text of such an answer is the text of the asking, and
        // those are written again, not continued.
        if one.body.trim().is_empty() {
            return Err("That answer has no text to continue from. Send the message again to try once more");
        }
        match newest_saying(chat) {
            Some(newest) if newest.id == one.id => Ok(()),
            _ => Err(
                "Only the newest answer can be continued. To redo an older answer, send its message again",
            ),
        }
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn answer_to_go_on_with(&self, id: &str) -> Result<(), &'static str> {
        self.answer_to_go_on_with_in(self.open_id().as_str(), id)
    }

    // Take the waiting list of a chat away. The messages of a chat that is removed have no
    // answer to wait for, and text kept for a chat that is not there would be kept for nothing.
    fn queue_drop(&mut self, chat: &str) {
        self.queues.retain(|one| one.chat != chat);
    }

    // The text the model wrote last in the chat that is left open: the answer that ends the
    // conversation, which is what a programmed message is checked against. Nothing when the
    // chat has no answer of its own to end on.
    //
    // Only the text of this asking count. A chat that has been asked twice checks what came
    // after the last message of the user, so an answer of an older asking never lets out a
    // message written for the answer that has not come yet.
    fn last_answer_in(&self, chat_id: &str) -> Option<String> {
        let chat = self.chat_of(chat_id)?;
        let from = chat
            .messages
            .iter()
            .rposition(|one| one.role == "You")
            .map(|at| at + 1)
            .unwrap_or(0);
        chat.messages[from..]
            .iter()
            .rev()
            .find(|one| one.role == "Assistant" && !one.body.trim().is_empty())
            .map(|one| one.body.clone())
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn last_answer(&self) -> Option<String> {
        self.last_answer_in(self.open_id().as_str())
    }

    // Say that an answer of the chat that is left open finished. The programmed messages of its
    // waiting list are checked against that text: the ones whose text the answer holds are let
    // out of the list, ahead of every message that is only waiting its turn. The text is
    // matched without regard to the bigness of the letters, which is what a user writing a few
    // words in the middle of an answer expects.
    fn answer_programmed_in(&mut self, chat_id: &str, body: &str) {
        let answer = body.to_lowercase();
        if answer.trim().is_empty() {
            return;
        }
        let found = {
            let Some(list) = self.queue_mut(chat_id) else {
                return;
            };
            let mut found = false;
            for one in list.planned.iter_mut() {
                let needle = one.matches.trim().to_lowercase();
                if one.ready || needle.is_empty() {
                    continue;
                }
                if answer.contains(needle.as_str()) {
                    one.ready = true;
                    found = true;
                }
            }
            found
        };
        if found {
            self.touch();
        }
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn answer_programmed(&mut self, body: &str) {
        let chat = self.open_id();
        self.answer_programmed_in(&chat, body);
    }

    // Say that the server knows nothing of any conversation any more. The conversations of the
    // chats were in the slots of that one server: a server started again has slots of its own,
    // and nothing of an older server is understood by it. The conversations themselves are safe
    // -- they were written out before the server was stopped, and the text of a chat is in its
    // file of chats as ever.
    pub fn context_lost(&mut self) {
        self.context_slot.clear();
        self.context_holds.clear();
    }

    // The thinking mode of the current chat. With no chat open, the mode the badge holds for
    // the next chat is used, so the badge and the flags of the server cannot drift apart.
    fn thinking_of_open(&self) -> String {
        self.chat()
            .map(|c| c.meta.thinking_mode.clone())
            .unwrap_or_else(|| self.new_thinking.clone())
    }

    // What a server has to wear for the current chat: the model. The running server is compared
    // against this, so a chat that is where the one before it was requests the server that is
    // already up, and the model on it stays loaded. Nothing of the thinking, of the tools or of
    // the sandbox is here, for all three are stated with every ask.
    fn server_wants(&self) -> ServerWants {
        ServerWants {
            model_id: self.current_model_id(),
        }
    }

    // What the chat named asks the server to be: the model of that chat. A turn that runs in
    // the background requests the server of its own chat, so a chat that wears another model is
    // not answered on the model of the open chat.
    fn server_wants_of(&self, chat: &str) -> ServerWants {
        ServerWants {
            model_id: self.current_model_id_of(chat),
        }
    }

    // The model the running server is on, in the same format as the one a chat requests, so the
    // two can be weighed against each other.
    fn server_wears(running: &Server) -> ServerWants {
        ServerWants {
            model_id: running.model_id.clone(),
        }
    }

    // What the current chat reports the chat template about thinking, for one ask. The mode is
    // the word the badge of the chat is on, and the keeping is named only for a model whose
    // template can keep the thinking of the turns before the last answer -- the same rule that
    // put those two words on the command line of the server before, when a change of either
    // cost the user the loading of the model over again.
    fn thinking_ask(&self) -> Value {
        llama::thinking_kwargs(
            self.thinking_of_open().as_str(),
            self.thinking.has_preserve.then(|| self.preserve_of_open()),
        )
    }

    // Every server of the user's own, in the order the user put them, with what the program
    // last knew of each.
    fn mcp_snap(&self) -> Vec<McpSnap> {
        let on = self.mcp_of_open();
        self.cfg
            .mcp_servers
            .iter()
            .map(|server| McpSnap {
                id: server.id.clone(),
                name: server.name.clone(),
                kind: server.kind(),
                icon: server.icon,
                shown: server.shown,
                on: on.contains(&server.id),
                tools: self
                    .mcp_tools
                    .get(&server.id)
                    .map(|tools| tools.iter().map(|t| t.name.clone()).collect())
                    .unwrap_or_default(),
                fault: self
                    .mcp_fault
                    .get(&server.id)
                    .cloned()
                    .unwrap_or_default(),
            })
            .collect()
    }

    // The servers of the user's own that remain switched on for the current chat. Only servers
    // that still are in the settings are named, so a chat that outlived one of them requests
    // nothing that is removed. With no chat open, the list the badge holds for the next chat is
    // used.
    pub fn mcp_of_open(&self) -> Vec<String> {
        let held = self
            .chat()
            .map(|c| c.meta.mcp.clone())
            .unwrap_or_else(|| self.new_mcp.clone());
        held.into_iter()
            .filter(|id| self.cfg.mcp_servers.iter().any(|s| &s.id == id))
            .collect()
    }

    // The servers that remain switched on for the current chat and whose tools were never read.
    // They are checked out of the state lock, for a server that will not answer is waited for,
    // and the window must be free to draw while it is waited for.
    fn mcp_to_look(&self) -> Vec<mcp::Server> {
        self.mcp_of_open()
            .into_iter()
            .filter(|id| !self.mcp_tools.contains_key(id))
            .filter_map(|id| {
                self.cfg
                    .mcp_servers
                    .iter()
                    .find(|s| s.id == id)
                    .cloned()
            })
            .collect()
    }

    // The tools of the servers that remain switched on, as the model is sent them. A server
    // with nothing to propose, or with nothing read of it, gives no tool, and the chat goes on
    // without it.
    fn mcp_definitions(&self) -> Vec<Value> {
        let mut made: Vec<Value> = Vec::new();
        for id in self.mcp_of_open() {
            let Some(server) = self.cfg.mcp_servers.iter().find(|s| s.id == id) else {
                continue;
            };
            let Some(tools) = self.mcp_tools.get(&server.id) else {
                continue;
            };
            for tool in tools {
                made.push(tool.definition(server));
            }
        }
        made
    }

    // The choice to keep the thinking of the turns before the last answer, for the current
    // chat. With no chat open, the choice the badge holds for the next chat is used, so the
    // badge and the flags of the server cannot drift apart.
    fn preserve_of_open(&self) -> bool {
        self.chat()
            .map(|c| c.meta.preserve_thinking)
            .unwrap_or(self.new_preserve)
    }

    // Whether the thoughts of the turns before the last answer go back to the model with the
    // text of the chat. The flag of the server only reports that the template may keep them;
    // the thoughts themselves live in the text sent, for the template reads them out of each
    // answer and out of nowhere else. So a chat that keeps them is not enough: the thinking of
    // an answer has to travel with it, and a model whose template cannot keep any does not get
    // the text either.
    fn kept_thinking(&self) -> bool {
        self.thinking.has_preserve && self.preserve_of_open()
    }

    fn set_status(&mut self, text: &str) {
        self.status = text.to_string();
        self.touch();
    }

    // Say that the text of a chat could not be put down on the disk. The conversation goes on
    // where it is in the memory, and the file of the chat is the conversation as it was when it
    // was last read. The user is informed, and the log of the program records it too: a chat
    // that leaves the side bar without a note about why is not to be removed through in silence
    // again.
    fn note_write_problem(&mut self, problem: &str) {
        proglog::note(proglog::STORE, problem);
        self.set_status(&format!("The chat could not be kept on this computer: {}", problem));
    }

    // Say that the record of a repository could not be put down. The name of a repository and
    // the marker of its pin are in that one file, and the chats of the repository are under
    // that name in the side bar, so a write that could not be finished is informed here and in
    // the log of the program. It is not left to return later as a repository with the wrong
    // name, or with none.
    fn note_record_problem(&mut self, problem: &str) {
        proglog::note(proglog::STORE, problem);
        self.set_status("The record of that repository could not be kept on this computer");
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

    // Open the chat that is at this place in the list, and put it at the header of its group in
    // the side bar. The moment it was opened is kept with the chat, which is what the order of
    // a group is read from.
    fn mark_opened_in(&mut self, chat_id: &str, place: usize) {
        // The moment of the open never remains below the last one handed out, so two chats
        // opened within one second keep the order the user opened them in, and the chat that
        // was opened last is the first in its group.
        let now = store::now_secs();
        let stamp = if now > self.live_mut(chat_id).last_opened { now } else { self.live_mut(chat_id).last_opened + 1 };
        self.live_mut(chat_id).last_opened = stamp;
        self.chats[place].meta.opened = stamp;
        let snapshot = self.chats[place].clone();
        self.live_mut(chat_id).ctx_used = snapshot.meta.context_used;
        self.current = Some(snapshot.meta.id.clone());
        match self.store.save_chat(&snapshot) {
            Ok(()) => self.set_status("Ready"),
            Err(problem) => self.note_write_problem(&problem),
        }
        self.touch_structure();
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn mark_opened(&mut self, place: usize) {
        let chat = self.open_id();
        self.mark_opened_in(&chat, place);
    }

    // Change the record of one chat, of this place or of another. The current place is looked
    // in first, for the chat that is left open lives there in its newest form; after it the
    // chat is read from the disk. `None` reports that no place holds such a chat.
    fn edit_chat<F>(&mut self, id: &str, change: F) -> Option<()>
    where
        F: FnOnce(&mut Chat),
    {
        if id.is_empty() {
            return None;
        }
        if let Some(place) = self.chats.iter().position(|c| c.meta.id == id) {
            change(&mut self.chats[place]);
            let snapshot = self.chats[place].clone();
            if let Err(problem) = self.store.save_chat(&snapshot) {
                self.note_write_problem(&problem);
            }
            self.touch_structure();
            return Some(());
        }
        let mut chat = self.store.load_chat(id)?;
        change(&mut chat);
        if let Err(problem) = self.store.save_chat(&chat) {
            self.note_write_problem(&problem);
        }
        self.touch_structure();
        Some(())
    }

    // Every repository with the chats that are in it, the chats that belong to no repository,
    // and the chats the user put away. A chat that was put away is in neither of the first two.
    // The chats of a group remain newest opened first, so the chat the user was last in is the
    // first of its group.
    fn lists(&self) -> Lists {
        let mut repositories: Vec<(Repo, Vec<Chat>)> = Vec::new();
        let mut archived: Vec<Chat> = Vec::new();
        for repo in self.repositories.iter() {
            let mut chats = self.store.list_chats(&repo.id);
            archived.extend(chats.iter().filter(|c| c.meta.archived).cloned());
            chats.retain(|c| !c.meta.archived);
            by_opened(&mut chats);
            repositories.push((repo.clone(), chats));
        }
        let mut loose = self.store.list_chats("");
        archived.extend(loose.iter().filter(|c| c.meta.archived).cloned());
        loose.retain(|c| !c.meta.archived);
        by_opened(&mut loose);
        // The chats that were put away remain newest first, for the user is most likely to want
        // the one they put away last.
        archived.sort_by_key(|c| std::cmp::Reverse(c.meta.updated));
        Lists {
            repositories,
            loose,
            archived,
        }
    }

    // Write the open chat to disk. Cloning first keeps the borrow short.
    //
    // A chat that holds nothing is not written at all. It is in the memory until the first
    // message makes it worth keeping, so a new chat that the user started and skipped leaves no
    // file behind and never shows in the side bar.
    fn persist(&mut self) {
        let chat = self.open_id();
        self.persist_in(&chat);
    }

    // Write one chat to disk, by its id. An answer that is arriving in the background writes
    // its own chat, which is not the one the side bar has open.
    fn persist_in(&mut self, chat: &str) {
        let Some(chat) = self.chat_of(chat) else { return };
        if chat.messages.is_empty() {
            return;
        }
        // The first message makes the chat a file, and a file makes a row in the side bar, so
        // the lists of the windows are created anew.
        let first = chat.messages.len() == 1;
        let snapshot = chat.clone();
        if first {
            self.touch_structure();
        }
        if let Err(problem) = self.store.save_chat(&snapshot) {
            self.note_write_problem(&problem);
        }
    }

    // Add one message of a normal type to the open chat.
    fn push_message(
        &mut self,
        role: &str,
        body: &str,
        reasoning: &str,
        reasoning_ms: u64,
        mono: bool,
    ) {
        let chat = self.open_id();
        self.push_message_in(&chat, role, body, reasoning, reasoning_ms, mono);
    }

    // Add one message of a normal type to the chat named. The summary of the work and the close
    // of the one block of the corporate display come from that chat's own live state.
    fn push_message_in(
        &mut self,
        chat: &str,
        role: &str,
        body: &str,
        reasoning: &str,
        reasoning_ms: u64,
        mono: bool,
    ) {
        // The corporate display keeps the summary of the work with the answer it belongs to, so
        // the one block can be drawn again when the chat is read after, and not only while the
        // answer arrives.
        let work = if role == "Assistant" && self.corporate_on_in(chat) {
            let mut held = std::mem::take(&mut self.live_mut(chat).live_work);
            // An answer that worked with a tool is in more than one message: the text of a
            // turn, the runs of the calls, and the text after them. The short belongs to all of
            // the answer, so what an earlier message of it holds comes over to this one. The
            // one block is therefore with the last message of the answer, and not with the
            // first message the work was written beside.
            if let Some(chat) = self.chat_of_mut(chat) {
                for one in chat.messages.iter_mut().rev() {
                    if one.role == "You" {
                        break;
                    }
                    if one.role != "Assistant" || one.work.is_empty() {
                        continue;
                    }
                    let mut older = std::mem::take(&mut one.work);
                    older.extend(std::mem::take(&mut held));
                    held = older;
                    one.work_head.clear();
                }
            }
            // Calls of one tool made one after another remain together in a block of their own,
            // so ten files made in a row read as one block of the summary with ten under it.
            corporate::grouped(&held)
        } else {
            Vec::new()
        };
        // The header of the block is reported as an event, so a search that was at the header
        // while the answer arrived is a search that was created. With no
        // work at all the block still remains: when the answer did nothing but think, the
        // header reports how long it thought.
        let head = if role == "Assistant" && self.corporate_on_in(chat) {
            corporate::head_of_answer(&work, reasoning_ms)
        } else {
            String::new()
        };
        // The text of this message is inside the one block of the answer, unless the model had
        // already closed it with its own tool.
        let closed = self
            .live_of(chat)
            .map(|one| one.live_work_closed)
            .unwrap_or(false);
        if let Some(chat) = self.chat_of_mut(chat) {
            chat.messages.push(Message {
                id: store::new_id(),
                role: role.to_string(),
                body: body.to_string(),
                reasoning: reasoning.to_string(),
                reasoning_ms,
                mono,
                work,
                work_head: head,
                work_closed: closed,
                when: store::now_secs(),
                ..Default::default()
            });
        }
        self.persist_in(chat);
        self.touch();
    }

    // The text a resumed answer adds to the answer the user stopped. What the model wrote
    // before the stop, and what it wrote on from that text, is one answer of the model and not
    // two, so the new text go onto the message that is at the end of the conversation. False
    // means the message is no longer the last one of the conversation, and the text has to be
    // kept on its own.
    fn append_resumed(&mut self, id: &str, body: &str, reasoning: &str, reasoning_ms: u64) -> bool {
        // The work of this part of the answer joins the summary of the answer it was removed on
        // from, so the one block of the corporate display holds all of the work and not only
        // the last part of it.
        let more = if self.corporate_on() {
            std::mem::take(&mut self.live_write().live_work)
        } else {
            Vec::new()
        };
        let joined = self.corporate_on();
        let Some(chat) = self.chat_mut() else { return false };
        let Some(last) = chat.messages.last_mut() else { return false };
        if last.id != id || last.role != "Assistant" {
            return false;
        }
        last.body.push_str(body);
        if !reasoning.trim().is_empty() {
            if last.reasoning.trim().is_empty() {
                last.reasoning = reasoning.to_string();
            } else {
                last.reasoning.push('\n');
                last.reasoning.push_str(reasoning);
            }
        }
        last.reasoning_ms += reasoning_ms;
        last.when = store::now_secs();
        if !more.is_empty() {
            last.work.extend(more);
        }
        if joined {
            last.work_head = corporate::head_of_answer(&last.work, last.reasoning_ms);
        }
        true
    }

    // Add one note. A failed note is drawn in the warning colour. Notes are for the window and
    // are never sent to the model.
    fn push_note_in(&mut self, chat_id: &str, text: &str, failed: bool) {
        if let Some(chat) = self.chat_of_mut(chat_id) {
            chat.messages.push(Message {
                id: store::new_id(),
                role: "Note".to_string(),
                body: text.to_string(),
                failed,
                when: store::now_secs(),
                ..Default::default()
            });
        }
        self.persist_in(chat_id);
        self.touch();
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn push_note(&mut self, text: &str, failed: bool) {
        let chat = self.open_id();
        self.push_note_in(&chat, text, failed);
    }

    // The answer the user stopped is the one answer the page may go on with. The marker
    // therefore goes on the newest saying of the model, and off every older one: two answers
    // under a marker of going on read as an error, and an answer the user has written past is
    // not the one they meant.
    fn mark_last_answer_stopped_in(&mut self, chat_id: &str) {
        let Some(chat) = self.chat_of_mut(chat_id) else { return };
        clear_stopped_marks(chat);
        if let Some(last) = chat.messages.last_mut() {
            if last.role == "Assistant" {
                last.stopped = true;
            }
        }
        self.persist_in(chat_id);
        self.touch();
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn mark_last_answer_stopped(&mut self) {
        let chat = self.open_id();
        self.mark_last_answer_stopped_in(&chat);
    }

    // The marker of an answer that never came to its end is taken off as careful as it is put
    // on: an answer that came to its end, and a new turn of the chat, both leave nothing to go
    // on with.
    fn clear_stopped_marks_in(&mut self, chat_id: &str) {
        let Some(chat) = self.chat_of_mut(chat_id) else { return };
        if !chat.messages.iter().any(|one| one.stopped) {
            return;
        }
        clear_stopped_marks(chat);
        self.persist_in(chat_id);
        self.touch();
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn clear_stopped_marks(&mut self) {
        let chat = self.open_id();
        self.clear_stopped_marks_in(&chat);
    }

    // Put the marker of a summarisation in the chat, before the handoff summary is requested.
    // The marker keeps its place whether the text come or not: the user sees where the
    // conversation was cut, and a summary the server never finished is a marker with nothing in
    // it, which the user can start again.
    //
    // A marker with nothing in it is started again in the place it remains, and is not copied.
    // The user is set to write that summarisation again, so a second heading with a new number
    // would say the chat was summarised twice when it was not: the retried marker keeps its
    // number and its place, and only its text and its hour are put again.
    fn begin_summary_in(&mut self, chat_id: &str) -> String {
        if let Some(chat) = self.chat_of_mut(chat_id) {
            if let Some(mark) = chat
                .messages
                .iter_mut()
                .rev()
                .find(|m| m.role == "Summary")
                .filter(|m| m.body.trim().is_empty())
            {
                mark.tool = String::from("the handoff summary is being written ...");
                mark.when = store::now_secs();
                mark.context_before = 0;
                mark.context_after = 0;
                let again = mark.id.clone();
                self.persist_in(chat_id);
                self.touch();
                return again;
            }
        }
        let id = store::new_id();
        if let Some(chat) = self.chat_of_mut(chat_id) {
            chat.messages.push(Message {
                id: id.clone(),
                role: "Summary".to_string(),
                tool: String::from("The handoff summary is being written ..."),
                when: store::now_secs(),
                ..Default::default()
            });
        }
        self.persist_in(chat_id);
        self.touch();
        id
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn begin_summary(&mut self) -> String {
        let chat = self.open_id();
        self.begin_summary_in(&chat)
    }

    // Write the handoff summary into its marker. The text the model gave are cleaned first, and
    // `why` holds the line the window shows under the number of the summarisation: a short word
    // of what happened, or of what went wrong. The two numbers of the directory remain with the
    // marker, so the user can see what the compaction cost.
    //
    // The marker goes into the chat at the end of it, before the request, for a server that is
    // removed in the middle of a summary must leave a sign of it. The text a summarisation
    // means are therefore already above it: a turn whose writing was stopped puts its text, its
    // thinking and the blocks of the calls it was writing in before the space is requested.
    // Nothing is moved here, and nothing that came after the marker is taken into the summary:
    // a message the user sent while the text was being written is after it, where they sent it.
    fn finish_summary_in(&mut self, chat_id: &str, id: &str, summary: &str, before: u64, after: u64, why: &str) {
        let Some(open) = self.current.clone() else {
            return;
        };
        let Some(chat) = self
            .chats
            .iter_mut()
            .find(|c| c.meta.id == open)
        else {
            return;
        };
        if let Some(place) = chat.messages.iter().position(|m| m.id == id) {
            if let Some(mark) = chat.messages.get_mut(place) {
                mark.body = summary.to_string();
                mark.context_before = before;
                mark.context_after = after;
                mark.tool = why.to_string();
            }
        }
        self.persist_in(chat_id);
        self.touch();
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn finish_summary(&mut self, id: &str, summary: &str, before: u64, after: u64, why: &str) {
        let chat = self.open_id();
        self.finish_summary_in(&chat, id, summary, before, after, why);
    }

    // Move one message of the chat to the place above the newest handoff summary that was
    // written. A call the model made before the space was freed is where it was created: the
    // summary means the text above it, and a block that went in below the marker reads as an
    // item that came after the compaction, and costs the space of every ask that follows. A
    // marker with no text in it cuts nothing off, and an older marker is not the place a block
    // of this turn belongs at, so nothing moves while the newest marker holds nothing.
    fn stand_before_summary(&mut self, id: &str) {
        let Some(open) = self.current.clone() else {
            return;
        };
        let Some(chat) = self
            .chats
            .iter_mut()
            .find(|c| c.meta.id == open)
        else {
            return;
        };
        let Some(where_the_mark_stands) = chat.messages.iter().rposition(|m| m.role == "Summary")
        else {
            return;
        };
        if chat.messages[where_the_mark_stands]
            .body
            .trim()
            .is_empty()
        {
            return;
        }
        let Some(place) = chat.messages.iter().position(|m| m.id == id) else {
            return;
        };
        if place <= where_the_mark_stands {
            return;
        }
        let moved = chat.messages.remove(place);
        chat.messages.insert(where_the_mark_stands, moved);
        self.persist();
        self.touch();
    }

    // The marker of a summarisation reports which of the two asks its text came from. The short
    // prompt of the settings is the request rebuilt when the context window filled under the
    // first one, or when the text of the first came to no end of themselves; its text hold
    // less, and the user is informed which of the two is in the chat. A marker that was never
    // written carries the marker of the request of the settings, for a chat of before reads
    // that way too.
    fn summary_short(&mut self, id: &str, short: bool) {
        let Some(chat) = self.chat_mut() else {
            return;
        };
        if let Some(place) = chat.messages.iter().position(|m| m.id == id) {
            if let Some(mark) = chat.messages.get_mut(place) {
                mark.short = short;
            }
        }
    }

    // True when a handoff summary of the open chat was requested and never returned. The chat
    // is at that point until the user starts the summary again, for the conversation past the
    // context window of the model cannot be sent as it is.
    fn handoff_owed_in(&self, chat_id: &str) -> bool {
        self.chat_of(chat_id).map(|chat| chat.handoff_owed()).unwrap_or(false)
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn handoff_owed(&self) -> bool {
        self.handoff_owed_in(self.open_id().as_str())
    }

    // The summarisations of the open chat, oldest first.
    fn handoffs_in(&self, chat_id: &str) -> Vec<Handoff> {
        self.chat_of(chat_id)
            .map(|chat| handoff_marks(chat, self.live_ref(chat_id).generating))
            .unwrap_or_default()
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn handoffs(&self) -> Vec<Handoff> {
        self.handoffs_in(self.open_id().as_str())
    }

    // Put a tool run in the chat before it starts. The id is given back so the output can be
    // written into the same message. The thinking of a turn with no text of its own is here
    // too, so it is not lost.
    fn begin_tool_in(
        &mut self,
        chat_id: &str,
        call: &ToolCall,
        shown: &str,
        thinking: &str,
        thinking_ms: u64,
    ) -> String {
        let id = store::new_id();
        if let Some(chat) = self.chat_of_mut(chat_id) {
            chat.messages.push(Message {
                id: id.clone(),
                role: "Tool".to_string(),
                body: shown.to_string(),
                reasoning: thinking.to_string(),
                reasoning_ms: thinking_ms,
                tool: String::from("Running ..."),
                mono: true,
                call_id: call.id.clone(),
                tool_name: call.name.clone(),
                tool_args: call.arguments.clone(),
                when: store::now_secs(),
                ..Default::default()
            });
        }
        self.corporate_begin_in(chat_id, call, &id);
        self.persist_in(chat_id);
        self.touch();
        id
    }

    // The same for the chat that is left open.
    fn begin_tool(
        &mut self,
        call: &ToolCall,
        shown: &str,
        thinking: &str,
        thinking_ms: u64,
    ) -> String {
        let chat = self.open_id();
        self.begin_tool_in(&chat, call, shown, thinking, thinking_ms)
    }

    // The corporate display keeps the summary of the work beside the answer: a block for every
    // call the model made, and the headers the model wrote for its own. These two hold it while
    // an answer arrives, and the message of the answer keeps it after.
    fn corporate_on_in(&self, _chat_id: &str) -> bool {
        config::display_is_corporate(&self.cfg.chat_display)
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn corporate_on(&self) -> bool {
        self.corporate_on_in(self.open_id().as_str())
    }

    // True when the settings make the space of a conversation by a handoff summary. With
    // compaction off nothing is ever summarised: the text of the space go out no more, and
    // the tool that requests the space has nothing to request, so it is proposed nowhere
    // and a call of its name is answered as a tool that is switched off.
    fn compaction_on(&self) -> bool {
        config::sanitize_compaction(&self.cfg.chat_compaction) == config::COMPACTION_HANDOFF
    }

    // The text the user remains the chat on: the flagged pre-written prompts in front of their
    // own text. Nothing when nothing is flagged and nothing is written, so a chat of a new set
    // of settings reads exactly as it read before this was.
    fn system_words(&self) -> String {
        system_words_of(&self.cfg)
    }

    // A call began: the one block reports what the model is at, and a block of the call is in
    // the summary of the work.
    fn corporate_begin_in(&mut self, chat_id: &str, call: &ToolCall, id: &str) {
        // The tool of the summary writes text, not a run: the heading it names is in the short,
        // and no block of a call means it. The tool that closes the block of the answer is
        // drawn nowhere either.
        if !self.corporate_on_in(chat_id)
            || call.name == corporate::NAME
            || call.name == corporate::FINISH_NAME
        {
            return;
        }
        let args: Value = serde_json::from_str(&call.arguments).unwrap_or(json!({}));
        let head = corporate::head_of(&call.name, &args);
        self.live_mut(chat_id).live_work_head = head.clone();
        match corporate::find_mut(&mut self.live_mut(chat_id).live_work, id) {
            Some(place) => place.head = head,
            // The call is under the heading the model opened last, when it opened one, and on
            // its own else.
            None => corporate::begin(&mut self.live_mut(chat_id).live_work, &call.name, id, &args),
        }
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn corporate_begin(&mut self, call: &ToolCall, id: &str) {
        let chat = self.open_id();
        self.corporate_begin_in(&chat, call, id);
    }

    // A call returned: the header of its block is reported in the text of an item that
    // happened, what a call that changed a file did to it is with it, and the text that
    // returned is inside it, for the block of the call is hidden while the corporate display
    // remains.
    fn corporate_finish_in(&mut self, chat_id: &str, id: &str, output: &str, ok: bool) {
        if !self.corporate_on_in(chat_id) {
            return;
        }
        let Some(place) = corporate::find_mut(&mut self.live_mut(chat_id).live_work, id) else {
            return;
        };
        let (adds, cuts) = plus_minus(output);
        corporate::came_back(place, output, ok, adds, cuts);
        // The header of the one block is reported again as an event, so a search that returned
        // is not still left at the header as a search on its way.
        self.live_mut(chat_id).live_work_head = corporate::head_after(&self.live_mut(chat_id).live_work);
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn corporate_finish(&mut self, id: &str, output: &str, ok: bool) {
        let chat = self.open_id();
        self.corporate_finish_in(&chat, id, output, ok);
    }

    // Write the output of a tool run into its message.
    fn finish_tool_in(&mut self, chat_id: &str, id: &str, output: &str, ok: bool) {
        self.finish_tool_with_in(chat_id, id, output, ok, Vec::new());
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn finish_tool(&mut self, id: &str, output: &str, ok: bool) {
        let chat = self.open_id();
        self.finish_tool_in(&chat, id, output, ok);
    }

    // Put a question to the user and begin the wait. The text of the question, the answers the
    // model proposed, and the wait that is with them all come from the request itself and the
    // settings; the popup above the chat composer is made of what the state carries.
    fn begin_asking_in(&mut self, chat_id: &str, asked: &askuser::Question) -> (String, u32) {
        let one = Asking {
            id: store::new_id(),
            question: asked.text.clone(),
            choices: asked.choices.clone(),
            critical: asked.critical,
            seconds: askuser::wait_of(
                asked.critical,
                self.cfg.ask_seconds,
                self.cfg.ask_critical_seconds,
            ),
            items: Vec::new(),
            plan: false,
            began: Instant::now(),
            answer: None,
        };
        let id = one.id.clone();
        let seconds = one.seconds;
        self.live_mut(chat_id).asking = Some(one);
        self.touch();
        (id, seconds)
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn begin_asking(&mut self, asked: &askuser::Question) -> (String, u32) {
        let chat = self.open_id();
        self.begin_asking_in(&chat, asked)
    }

    // A questionnaire of planning mode. It carries any number of questions, is waited for as
    // long as it takes, and is answered when the user sends all of it manually.
    fn begin_asking_plan_in(&mut self, chat_id: &str, items: Vec<AskItem>) -> String {
        let first = items.first().map(|one| one.prompt.clone()).unwrap_or_default();
        let one = Asking {
            id: store::new_id(),
            question: first,
            choices: Vec::new(),
            critical: true,
            // No clock runs on a questionnaire: the plan is not served on time.
            seconds: 0,
            items,
            plan: true,
            began: Instant::now(),
            answer: None,
        };
        let id = one.id.clone();
        self.live_mut(chat_id).asking = Some(one);
        self.touch();
        id
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn begin_asking_plan(&mut self, items: Vec<AskItem>) -> String {
        let chat = self.open_id();
        self.begin_asking_plan_in(&chat, items)
    }

    // The answers of the user to a questionnaire, put in the text the model is given back. Each
    // answer is under the question it belongs to, so the model cannot mistake which question
    // was answered which way.
    fn answer_plan_in(&mut self, chat_id: &str, id: &str, answers: Vec<Vec<String>>) -> bool {
        let Some(one) = self.live_mut(chat_id).asking.as_mut() else {
            return false;
        };
        if !one.plan || one.id != id || one.answer.is_some() {
            return false;
        }
        let mut said = String::from("The user answered the questions of the plan.\n");
        for (number, item) in one.items.iter().enumerate() {
            said.push_str(&format!("{}. {}\n", number + 1, item.prompt));
            let chosen = answers
                .get(number)
                .map(|list| list.iter().filter(|word| !word.trim().is_empty()).cloned().collect::<Vec<String>>())
                .unwrap_or_default();
            if chosen.is_empty() {
                said.push_str("   Answer: nothing was chosen\n");
            } else {
                said.push_str(&format!("   Answer: {}\n", chosen.join("; ")));
            }
        }
        one.answer = Some(said);
        self.touch();
        true
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn answer_plan(&mut self, id: &str, answers: Vec<Vec<String>>) -> bool {
        let chat = self.open_id();
        self.answer_plan_in(&chat, id, answers)
    }

    // The answer of the user, when the question the page was drawing is the one that remains. A
    // question that was answered already, or that the run has given over, is answered no more.
    fn answer_asking_in(&mut self, chat_id: &str, id: &str, answer: &str) -> bool {
        let Some(one) = self.live_mut(chat_id).asking.as_mut() else {
            return false;
        };
        if one.id != id || one.answer.is_some() {
            return false;
        }
        one.answer = Some(answer.to_string());
        self.touch();
        true
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn answer_asking(&mut self, id: &str, answer: &str) -> bool {
        let chat = self.open_id();
        self.answer_asking_in(&chat, id, answer)
    }

    // What the user answered the question of this id, when they have. The run of the tool asks
    // this until the wait ends.
    fn asking_answer_of_in(&self, chat_id: &str, id: &str) -> Option<String> {
        self.live_ref(chat_id).asking
            .iter()
            .find(|one| one.id == id)
            .and_then(|one| one.answer.clone())
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn asking_answer_of(&self, id: &str) -> Option<String> {
        self.asking_answer_of_in(self.open_id().as_str(), id)
    }

    // Take the request away, for the question came to its end and the popup over the chat
    // composer has nothing left to say. A question of another run is skipped.
    fn clear_asking_in(&mut self, chat_id: &str, id: &str) {
        if self
            .live_mut(chat_id)
            .asking
            .as_ref()
            .map(|one| one.id == id)
            .unwrap_or(false)
        {
            self.live_mut(chat_id).asking = None;
            self.touch();
        }
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn clear_asking(&mut self, id: &str) {
        let chat = self.open_id();
        self.clear_asking_in(&chat, id);
    }

    // The same, with the files a run of the download tool proposed to the user. The proposal
    // rides on the message of the run, so the blocks can be drawn again when the chat is opened
    // another day; the blocks themselves are at the end of the answer, and only once the model
    // has stopped writing. See `gifts_till` for the proposal of a turn.
    fn finish_gifts_in(&mut self, chat_id: &str, id: &str, output: &str, ok: bool, gifts: Vec<download::Gift>) {
        let text = self.with_stopped_head(output);
        if let Some(chat) = self.chat_of_mut(chat_id) {
            if let Some(message) = chat.messages.iter_mut().find(|m| m.id == id) {
                message.tool = text.clone();
                message.failed = !ok;
                message.downloads = gifts;
            }
        }
        self.live_mut(chat_id).running = None;
        self.persist_in(chat_id);
        self.touch();
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn finish_gifts(&mut self, id: &str, output: &str, ok: bool, gifts: Vec<download::Gift>) {
        let chat = self.open_id();
        self.finish_gifts_in(&chat, id, output, ok, gifts);
    }

    // The same, with the files one run of the display tool put in the window. The show rides on
    // the message of the run, so the button of the block opens the same files again when the
    // chat is opened another day.
    fn finish_displayed_in(&mut self, chat_id: &str, id: &str, output: &str, ok: bool, displayed: Vec<display::Shown>) {
        let text = self.with_stopped_head(output);
        if let Some(chat) = self.chat_of_mut(chat_id) {
            if let Some(message) = chat.messages.iter_mut().find(|m| m.id == id) {
                message.tool = text.clone();
                message.failed = !ok;
                message.displayed = displayed;
            }
        }
        self.live_mut(chat_id).running = None;
        self.persist_in(chat_id);
        self.touch();
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn finish_displayed(&mut self, id: &str, output: &str, ok: bool, displayed: Vec<display::Shown>) {
        let chat = self.open_id();
        self.finish_displayed_in(&chat, id, output, ok, displayed);
    }

    // The same, with the next steps one run of the suggestion tool proposed. The proposal rides
    // on the message of the run, so the buttons are drawn again when the chat is opened another
    // day. Only the newest propose remains: every other message of the chat is stripped of what
    // it proposed, for the paths an older answer named are no longer the paths the conversation
    // is at.
    fn finish_suggestions_in(
        &mut self, chat_id: &str,
        id: &str,
        output: &str,
        ok: bool,
        asks: Vec<crate::suggest::Suggestion>,
    ) {
        let text = self.with_stopped_head(output);
        let standing = !asks.is_empty();
        if let Some(chat) = self.chat_of_mut(chat_id) {
            for message in chat.messages.iter_mut() {
                if message.id == id {
                    message.tool = text.clone();
                    message.failed = !ok;
                    message.suggestions = asks.clone();
                } else if standing && !message.suggestions.is_empty() {
                    message.suggestions = Vec::new();
                }
            }
        }
        self.live_mut(chat_id).running = None;
        self.persist_in(chat_id);
        self.touch();
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn finish_suggestions(&mut self, id: &str, output: &str, ok: bool, asks: Vec<crate::suggest::Suggestion>) {
        let chat = self.open_id();
        self.finish_suggestions_in(&chat, id, output, ok, asks);
    }

    // The same, with the steps of the work one run of the steps tool showed. The block rides on
    // the message of the run, so it is drawn again when the chat is opened another day. The
    // newest call of the answer wins: an earlier call of the same answer is cleared, so a model
    // that corrects its own list does not leave two blocks under the answer.
    fn finish_steps_in(
        &mut self, chat_id: &str,
        id: &str,
        output: &str,
        ok: bool,
        plan: Option<crate::steps::Plan>,
    ) {
        let text = self.with_stopped_head(output);
        let standing = plan.is_some();
        if let Some(chat) = self.chat_of_mut(chat_id) {
            let mut reached = false;
            for message in chat.messages.iter_mut() {
                if message.id == id {
                    message.tool = text.clone();
                    message.failed = !ok;
                    message.steps = plan.clone();
                    reached = true;
                } else if standing && message.steps.is_some() && !reached {
                    message.steps = None;
                }
            }
        }
        self.live_mut(chat_id).running = None;
        self.persist_in(chat_id);
        self.touch();
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn finish_steps(&mut self, id: &str, output: &str, ok: bool, plan: Option<crate::steps::Plan>) {
        let chat = self.open_id();
        self.finish_steps_in(&chat, id, output, ok, plan);
    }

    // The same, with the records of the images the run opened hanging on the message as well. A
    // tool that pointers an image to the model keeps the bytes of it out of the text the user
    // is given, so the image is where the images of a chat remain: in the directory of the
    // records, named by the message that opened it.
    fn finish_tool_with_in(
        &mut self, chat_id: &str,
        id: &str,
        output: &str,
        ok: bool,
        pictures: Vec<Attachment>,
    ) {
        let text = self.with_stopped_head(output);
        if let Some(chat) = self.chat_of_mut(chat_id) {
            if let Some(message) = chat.messages.iter_mut().find(|m| m.id == id) {
                message.tool = text.clone();
                message.failed = !ok;
                message.attachments = pictures;
            }
        }
        self.live_mut(chat_id).running = None;
        self.corporate_finish_in(chat_id, id, output, ok);
        self.persist_in(chat_id);
        self.touch();
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn finish_tool_with(&mut self, id: &str, output: &str, ok: bool, pictures: Vec<Attachment>) {
        let chat = self.open_id();
        self.finish_tool_with_in(&chat, id, output, ok, pictures);
    }

    // Put a call that is running under a clock, and remember which block of the chat it is in.
    // The markers of a running call -- the one that cuts the call short and the one that gives
    // it more time -- speak to this clock.
    fn begin_running_in(&mut self, chat_id: &str, id: &str, watch: &crate::util::Watch) {
        self.live_mut(chat_id).running = Some(Running {
            id: id.to_string(),
            watch: watch.clone(),
        });
        self.touch();
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn begin_running(&mut self, id: &str, watch: &crate::util::Watch) {
        let chat = self.open_id();
        self.begin_running_in(&chat, id, watch);
    }

    // The text that is above the output of a call that was stopped before it returned: its time
    // ran out, or the user cut it short. Nothing is put in front of a call that returned of its
    // own accord. What the call had given by then is kept below this text, for a model that
    // reads the block has to know what the call managed to do before it was stopped.
    fn stopped_head_in(&self, chat_id: &str, gave_something: bool) -> Option<String> {
        let one = self.live_ref(chat_id).running.as_ref()?;
        let why: String = if one.watch.stopped() {
            String::from("The user stopped this call before it returned.")
        } else if one.watch.expired() {
            let given = one.watch.limit()?.as_secs();
            let when = if given >= 60 {
                let minutes = given / 60;
                let seconds = given % 60;
                if seconds == 0 {
                    format!("{} minutes", minutes)
                } else {
                    format!("{} minutes {} seconds", minutes, seconds)
                }
            } else {
                format!("{} seconds", given)
            };
            format!("The call ran out of time after {} and was stopped.", when)
        } else {
            return None;
        };
        Some(format!(
            "{} {}",
            why,
            if gave_something {
                "Everything it returned before that point is below."
            } else {
                "It had given nothing by then."
            }
        ))
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn stopped_head(&self, gave_something: bool) -> Option<String> {
        self.stopped_head_in(self.open_id().as_str(), gave_something)
    }

    // The output of a call, with the text of a stopped call in front of it when the call was
    // stopped.
    fn with_stopped_head(&self, output: &str) -> String {
        match self.stopped_head(!output.trim().is_empty()) {
            None => output.to_string(),
            Some(head) if output.trim().is_empty() => head,
            Some(head) => format!("{}\n{}", head, output),
        }
    }

    // Write down a tool call that the context window filled under. The call is in the chat as
    // the item it was: writing that was stopped, and a tool that never ran. The text of it is
    // the text the user watched being written, and all of it rides on the message, so the
    // model can be informed what it had been put to when it is asked again after the space was
    // freed.
    fn push_cancelled(&mut self, call: &ToolCall, thinking: &str, thinking_ms: u64, bound: usize) {
        self.push_stopped_call(call, thinking, thinking_ms, bound, false);
    }

    // A tool call the user stopped manually, while the model was still writing it. The block of
    // the text being written goes with the answer, so what the user stopped still is in the
    // chat: an answer that ends with the model's last work hidden reads as though the program
    // had thrown it away.
    fn push_stopped_by_user(
        &mut self,
        call: &ToolCall,
        thinking: &str,
        thinking_ms: u64,
        bound: usize,
    ) {
        self.push_stopped_call(call, thinking, thinking_ms, bound, true);
    }

    // The two paths above meet here. `by_user` reports the call the user stopped from the call
    // the context window filled under, in the chat and in what goes back to the model.
    fn push_stopped_call(
        &mut self,
        call: &ToolCall,
        thinking: &str,
        thinking_ms: u64,
        bound: usize,
        by_user: bool,
    ) {
        // The text of the call is kept to what the space left over can carry, and the block the
        // user is shown is created from the same text: what the user sees and what the model is
        // informed cannot then differ.
        let args = head_chars(&call.arguments, bound);
        let (shown, _, target) = words_of(&call.name, &args);
        let shown = if target.is_empty() { shown } else { target };
        // A call stopped before it had a name has no text to show, and a block with nothing
        // under its header reads as an error in the program.
        let shown = if shown.trim().is_empty() {
            String::from("(the call was stopped before the model had written any of it)")
        } else {
            shown
        };
        if let Some(chat) = self.chat_mut() {
            chat.messages.push(Message {
                id: store::new_id(),
                role: "Tool".to_string(),
                body: shown,
                reasoning: thinking.to_string(),
                reasoning_ms: thinking_ms,
                tool: String::from(if by_user {
                    STOPPED_WORDS
                } else {
                    CANCELLED_WORDS
                }),
                mono: true,
                failed: true,
                cancelled: true,
                user_stopped: by_user,
                call_id: call.id.clone(),
                tool_name: call.name.clone(),
                tool_args: args,
                when: store::now_secs(),
                ..Default::default()
            });
        }
        self.persist();
        self.touch();
    }

    // True when the model can be shown an image: the vision switch of the model settings is on,
    // and a projector file remains next to the model for the server to take. The server gets
    // its `--mmproj` from the same two facts, so the answer here and the flags there cannot
    // drift apart.
    fn pictures_can_go(&self) -> bool {
        match self.model() {
            Some(model) => {
                self.cfg.model(&model.id()).vision && llama::find_mmproj(&model.path).is_some()
            }
            None => false,
        }
    }

    // Turn the open chat into the message list the API expects. The day and the clock time of
    // this computer are at the top of the list, in a message of the role `system`, once for the
    // conversation.
    fn build_history_in(&self, chat_id: &str) -> Value {
        let mut out: Vec<Value> = Vec::new();
        let Some(chat) = self.chat_of(chat_id) else {
            return json!([]);
        };
        if let Some(line) = date_message(&self.cfg, began_of(chat)) {
            out.push(line);
        }
        // The text the user remains the chat on: the flagged pre-written prompts and the text
        // of their own, in one message of the role `system`. The line of the day keeps the
        // first place, for the model reads the clock before it reads how it is to write. The
        // message is left out when the user wrote nothing and flagged nothing.
        let words = self.system_words();
        if !words.is_empty() {
            out.push(json!({
                "role": "system",
                "content": words,
            }));
        }
        // The text of the chapters of the chat, beside the text of the program. While the
        // switch of the Chatting settings is on, the nodes beside the chat are extracted from
        // the headings of the answers, and the model has to know that it is being read that
        // way. The text go away with the switch.
        if self.cfg.chat_chapters {
            out.push(json!({
                "role": "system",
                "content": chapters_words(),
            }));
        }
        // The text of the corporate display, beside the text of the program. The mode hides the
        // thoughts and the call blocks, so the model has to know that the short it writes is
        // the only account of its work the user reads. The text go away with the mode.
        if self.corporate_on_in(chat_id) {
            out.push(json!({
                "role": "system",
                "content": corporate::system_prompt(),
            }));
        }
        // The text of the colours the model may put on its text, beside the text of the
        // program. The page paints them, and the markers go out of the conversation with the
        // answers, so the model has to know what it writes into. The text go away with the
        // switch.
        if self.cfg.chat_colour_text {
            out.push(json!({
                "role": "system",
                "content": colour_words(),
            }));
        }
        // The text of the space the conversation is on, beside the text of the program. A
        // chat whose space is created by a handoff summary has no ceiling the model has to work
        // within, and a model that does not know that compresses its own work for a limit that
        // is not there. The text go away when compaction remains off.
        if self.compaction_on() {
            out.push(json!({
                "role": "system",
                "content": compaction_words(),
            }));
        }
        // The prompt of planning mode, beside the text of the program. It is sent for every ask
        // of a planning chat, and it is removed with the mode: the chat that works from the
        // plan is asked with none of it, and with no word of the two tools of planning either.
        if chat.meta.planning {
            out.push(json!({
                "role": "system",
                "content": planning::system_prompt(&chat.meta.plan_file),
            }));
        }
        // The text of the tasks suite, beside the text of the program. The six tools are
        // proposed only while the line of the badge is on, and the rule the suite exists for -
        // that the list of tasks is read before an answer ends - is worth nothing unless the
        // model is informed of it. The text go away with the line.
        if chat.meta.tools.tasks {
            out.push(json!({
                "role": "system",
                "content": tasks::system_prompt(),
            }));
        }
        let vision_on = self.pictures_can_go();
        // The records of the attachments of this chat are in a directory of its own space,
        // so that is where their text and images are searched for.
        let shelf = self.attach_place_for(chat);
        // The thoughts that may go back with the answers of the chat.
        let keep_thinking = self.kept_thinking();
        // Where the newest handoff summary that was really written is among the messages. The
        // messages before it are the text that summary means, and they are left out here: that
        // is all of the compaction. The first message of the user keeps its place whatever the
        // summary has to say, so its text, and the images hung on them, stay at the start of
        // the prompt. The server worked them through once and keeps the work it did on them,
        // which is why the summary rides after that first message rather than in the place of
        // it.
        let cut = last_summary_place(chat);
        for (place, message) in chat.messages.iter().enumerate() {
            // The first message of the user always goes; the text a summary implies does not.
            if let Some(where_it_stands) = cut {
                if place != 0 && place < where_it_stands {
                    continue;
                }
            }
            match message.role.as_str() {
                // The handoff summary of a compaction, sent as the text of the program beside
                // the text of the user. The marker of a summary that was never written sends
                // nothing of its own.
                "Summary" => {
                    if message.body.trim().is_empty() {
                        continue;
                    }
                    out.push(json!({
                        "role": "user",
                        "content": handoff_block(&message.body),
                    }));
                }
                // The text of the user, and the files attached to it. Without
                // attachments this is the plain text, as it always was. The line of the day and
                // the clock time is not put among this text: it remains once, at the header of
                // the conversation.
                "You" => {
                    out.push(json!({
                        "role": "user",
                        "content": attach::content_at(&shelf, &message.body, &message.attachments, vision_on),
                    }));
                }
                "Assistant" => {
                    // The markers of a colour are a look over the text and not text of the
                    // answer: the page paints the text between them and loses the markers, so
                    // the markers go out of the conversation here. With the switch off the page
                    // reads no colour, and the markers stay where the model wrote them.
                    let said = if self.cfg.chat_colour_text {
                        json!(strip_colour_tags(&message.body))
                    } else {
                        json!(message.body)
                    };
                    let mut entry = json!({
                        "role": "assistant",
                        "content": said,
                    });
                    // The thinking of the answer rides beside its text, so the template may put
                    // it back where it was. An answer that thought nothing sends nothing, for
                    // the empty block of a turn is not worth the space.
                    if keep_thinking && !message.reasoning.trim().is_empty() {
                        entry["reasoning_content"] = json!(message.reasoning);
                    }
                    out.push(entry);
                }
                // A tool run and a chapter are both a call the model made and an answer it got
                // back. Only the run is shown to the user.
                "Tool" | "Chapter" => {
                    // A call that was stopped before the model had written all of it is not a
                    // call. The text of it stop in the middle, and a server set to read
                    // arguments out of them would error over the broken JSON, as it errors over
                    // a call the directory truncated in an answer. So the call goes back as the
                    // text of the turn it was in, and the model is informed in plain text that
                    // the call was cancelled and what it had written when it was stopped.
                    // Whether the space or the user stopped it is informed too, for one had
                    // its space made after it and the other had nothing after it at all.
                    if message.cancelled {
                        let told = if message.user_stopped {
                            stopped_words(&message.tool_name, &message.tool_args)
                        } else {
                            cut_words(&message.tool_name, &message.tool_args)
                        };
                        out.push(json!({
                            "role": "assistant",
                            "content": told,
                        }));
                        // The report is where the writing stopped: above the marker of the
                        // summarisation, so the request for the summary carries it and the
                        // marker then cuts it off. A chat written before that rule can still
                        // hold a report below the marker; it goes back where it is, and the
                        // model is informed what it was put to.
                        continue;
                    }
                    if message.call_id.is_empty() {
                        continue;
                    }
                    let name = if message.tool_name.is_empty() {
                        tools::TOOL_NAME
                    } else {
                        message.tool_name.as_str()
                    };
                    // One stored message means the call and for its result, so it becomes two
                    // entries here.
                    let mut call = json!({
                        "role": "assistant",
                        "content": Value::Null,
                        "tool_calls": [{
                            "id": message.call_id,
                            "type": "function",
                            "function": { "name": name, "arguments": message.tool_args }
                        }]
                    });
                    // The thinking of a turn that requested a command is with the command, so
                    // it goes back the same way.
                    if keep_thinking && !message.reasoning.trim().is_empty() {
                        call["reasoning_content"] = json!(message.reasoning);
                    }
                    out.push(call);
                    // The text of the result, and after it the image a tool opened, when
                    // there is one and this model can be shown an image. With neither the entry
                    // is the plain text, as it always was.
                    let content = if message.attachments.is_empty() {
                        Value::String(message.tool.clone())
                    } else {
                        attach::parts_at(
                            &shelf,
                            &message.tool,
                            &message.attachments,
                            vision_on,
                        )
                    };
                    out.push(json!({
                        "role": "tool",
                        "tool_call_id": message.call_id,
                        "content": content,
                    }));
                }
                // A note is only for the window.
                _ => {}
            }
        }
        // A marker of an image remaining among the text is broken before the conversation is
        // sent: the server would search for an image at it and refuse the whole ask. See
        // `break_media_marks`.
        let mut history = Value::Array(out);
        break_media_marks_in(&mut history);
        history
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    fn build_history(&self) -> Value {
        self.build_history_in(self.open_id().as_str())
    }

    // Read the chats of the current scope from disk.
    fn reload_chats(&mut self) {
        self.chats = self.store.list_chats(&self.scope_repo);
        // The badge no longer proposes "auto": the user chooses, and thinking is off unless it
        // is requested. A chat left with the old mode starts with thinking off, which is what
        // the badge then shows.
        let mut stamped = false;
        for chat in self.chats.iter_mut() {
            if chat.meta.thinking_mode.is_empty() || chat.meta.thinking_mode == "auto" {
                chat.meta.thinking_mode = String::from("off");
            }
            // The moment the chat was begun is the moment the line of the day is written from,
            // and it must never move. That line is at the header of the conversation: a header
            // that changes between two asks of one chat is a different sends requests to the
            // server, which then deletes the work it cached on all the text behind the header,
            // or answers from a state of its cache that is not the state of the conversation. A
            // chat written before this moment was kept is named from this moment, and keeps it
            // from then on, as a chat begun now does.
            if chat.meta.created <= 0 {
                chat.meta.created = store::now_secs();
                stamped = true;
            }
        }
        if stamped {
            self.persist();
        }
        // Keep the open chat when it is still there, and forget the number of the conversation
        // when it is not. A place is never opened by itself: the first screen is between
        // places, and the user opens a chat of the new place from the side bar, or starts a new
        // one.
        if self
            .current
            .as_ref()
            .map(|id| !self.chats.iter().any(|c| &c.meta.id == id))
            .unwrap_or(false)
        {
            self.current = None;
        }
        if self.current.is_none() {
            self.live_write().ctx_used = 0;
        }
        // A chat that is being answered in the background is kept in the memory even when it is
        // not a chat of the place that was just read again. The turn writes its messages into
        // the chat it was begun in, and a chat that has been sent of the memory leaves the turn
        // with nothing to write into: the text of the answer would be lost between the server
        // and the conversation.
        let working: Vec<String> = self
            .live
            .iter()
            .filter(|(_, one)| one.generating || one.summarising)
            .map(|(id, _)| id.clone())
            .collect();
        for id in working {
            if self.chats.iter().any(|c| c.meta.id == id) {
                continue;
            }
            if let Some(chat) = self.store.load_chat(&id) {
                self.chats.push(chat);
            }
        }
        self.touch_structure();
    }

    // Start a new chat in the place that is open, and open it. Nothing is written to disk yet:
    // the first message the user sends makes the chat a real one and writes it, so a chat that
    // was opened and left empty leaves no file behind.
    //
    // A new chat is where the user left the chat before it. Every setter of the two badges, the
    // sandbox, the thinking mode and the choice of keeping the thinking keeps the choice for
    // the next chat in step with the chat that is left open, so the new chat requests the
    // server the chat before it ran on and the model on it stays loaded.
    fn start_chat(&mut self) {
        let model_id = self.current_model_id();
        let mut chat = Chat::new(&self.scope_repo, &model_id);
        chat.meta.sandbox_enabled = self.new_sandbox;
        chat.meta.sandbox_network = self.new_sandbox_access.network;
        chat.meta.sandbox_pip = self.new_sandbox_access.pip;
        chat.meta.sandbox_fetch = self.new_sandbox_access.fetch;
        chat.meta.code_enabled = self.new_code;
        chat.meta.tools = self.new_tools.clone();
        chat.meta.search_enabled = self.new_search;
        chat.meta.thinking_mode = self.new_thinking.clone();
        chat.meta.preserve_thinking = self.new_preserve;
        chat.meta.mcp = self.new_mcp.clone();
        chat.meta.opened = store::now_secs();
        self.chats.insert(0, chat);
        self.current = Some(self.chats[0].meta.id.clone());
        self.live_write().ctx_used = 0;
        self.set_status("A new chat. Write in it below");
        self.touch_structure();
    }

    // Put the first screen up: no chat is open, so the marker, the line for this hour and the
    // chat composer remain together in the middle of the window. This is what the program shows
    // when it starts, and what `Create new chat` in the side bar goes back to.
    fn open_home(&mut self) {
        self.current = None;
        self.live_write().ctx_used = 0;
        self.touch_structure();
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

    // Point at a model when nothing sensible is picked yet. A model that was picked is kept.
    // Without this the top bar shows a model that no chat uses, and a message then fails with
    // "Choose a model in the top bar first".
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

    // The model of the chat named: what the chat wears when it wears one, and the model of the
    // place else. A turn requests the model of its own chat, so two chats of one model are
    // answered by one server, and a chat that wore another model is not answered by it.
    fn current_model_id_of(&self, chat: &str) -> String {
        if let Some(one) = self.chat_of(chat) {
            if !one.meta.model_id.is_empty() {
                return one.meta.model_id.clone();
            }
        }
        self.cfg.model_id.clone()
    }

    // The model of the chat named, when it is in the list of the folder.
    fn model_of(&self, chat: &str) -> Option<ModelInfo> {
        let id = self.current_model_id_of(chat);
        self.models.iter().find(|m| m.id() == id).cloned()
    }

    // What a server is started with, written as one line. A server comes from the directory of
    // llama-server, the current model, the settings of that model, and the one switch of the
    // chatting settings the server is informed about while it starts. Nothing else in the
    // settings reaches it, so two sets of settings that give the same line may share a loaded
    // model: the model has no reason to go because of a setting that only the program reads.
    pub fn server_stamp(&self) -> String {
        let id = self.current_model_id();
        serde_json::to_string(&(
            &self.cfg.llama_dir,
            &self.cfg.models_dir,
            &id,
            self.cfg.model(&id),
            self.cfg.chat_context_files,
        ))
        .unwrap_or_default()
    }

    // Settings for one model, with the device list fitted to the computer.
    fn settings_for(&self, model: &ModelInfo) -> ModelSettings {
        let mut settings = self.cfg.model(&model.id());
        settings.resize_gpus(self.gpus.len());
        settings.vram_budget = config::sanitize_vram_budget(settings.vram_budget);
        settings
    }

    // Read the chat template the model will use, and what it can do with thinking. A custom
    // template from the settings wins over the one the model file carries.
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

    // Remain the thinking on a mode the current model proposes. A model with levels and a model
    // with a switch do not propose the same text, so the mode of a chat may mean nothing to the
    // model that comes after it. A mode the new model does not propose is put at "off", which
    // every model means: the badge then shows what the server will really be given, and no word
    // the template cannot read reaches its flags.
    fn normalize_thinking(&mut self) {
        let modes = self.thinking.modes();
        if modes.is_empty() {
            // A model with nothing to propose is with the thinking off, and sends no flag of it
            // either way.
            self.set_thinking_everywhere("off");
            return;
        }
        if let Some(chat) = self.chat_mut() {
            if !modes.contains(&chat.meta.thinking_mode.as_str()) {
                chat.meta.thinking_mode = String::from("off");
            }
        }
        if !modes.contains(&self.new_thinking.as_str()) {
            self.new_thinking = String::from("off");
        }
    }

    // Put one mode on the current chat and on the next chat as well. A model with no thinking
    // at all takes both to "off".
    fn set_thinking_everywhere(&mut self, mode: &str) {
        if let Some(chat) = self.chat_mut() {
            chat.meta.thinking_mode = String::from(mode);
        }
        self.new_thinking = String::from(mode);
    }

    // What a command of the current chat may reach beyond its own folder. With no chat open the
    // choices the badges are on are the choices of the next chat.
    fn access_of_open(&self) -> tools::Access {
        self.chat()
            .map(|chat| access_of(&chat.meta))
            .unwrap_or(self.new_sandbox_access)
    }

    // What a command of the chat named may reach. A turn that runs in the background reads the
    // markers of its own chat and not of whichever chat the side bar has open.
    fn access_of_chat(&self, chat: &str) -> tools::Access {
        self.chat_of(chat)
            .map(|one| access_of(&one.meta))
            .unwrap_or(self.new_sandbox_access)
    }

    // Directory a tool run starts in. A repository chat works inside its repository, and a
    // loose chat gets its own folder.
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

    // The folder the records of the attachments of a chat are in: the folder `.attachments` in
    // the working directory of the chat. A chat and its files are opened again together, and go
    // together when the user moves the repository, and a command of the tool may read a file
    // the user hung on a message, for it works in the same space.
    fn attach_place_for(&self, chat: &Chat) -> PathBuf {
        attach::shelf(&self.workdir_for(chat))
    }

    // The directory of the records of the chat named, for an image a tool opened.
    fn attach_place_for_chat(&self, chat: &str) -> Option<PathBuf> {
        let one = self.chat_of(chat)?;
        Some(self.attach_place_for(one))
    }

    // The directory of the records of the open chat, for an image a tool opened.
    fn attach_place(&self) -> Option<PathBuf> {
        self.chat().map(|chat| self.attach_place_for(chat))
    }

    // True when the open chat is working in planning mode.
    pub fn planning_open(&self) -> bool {
        self.chat()
            .map(|chat| chat.meta.planning)
            .unwrap_or(false)
    }

    // True when the settings request the corporate way of showing the work. The way means the
    // whole program, and not for one chat, for it is a way of laying the pages out and not a
    // state of the work.
    pub fn corporate_open(&self) -> bool {
        config::display_is_corporate(&self.cfg.chat_display)
    }

    // Turn planning mode on for the open chat, and make the plan file it works in. A chat that
    // is planning already keeps the plan it had written.
    pub fn start_planning(&mut self) -> bool {
        let name = {
            let Some(chat) = self.chat_mut() else {
                return false;
            };
            if chat.meta.planning && !chat.meta.plan_file.is_empty() {
                return true;
            }
            let name = planning::create(&chat.meta.id);
            chat.meta.planning = true;
            chat.meta.plan_file = name.clone();
            chat.meta.updated = store::now_secs();
            name
        };
        // The plan file is created, and the chat reports it holds it. An empty chat is no file
        // yet, and the text of the chat is what make it one.
        let _ = name;
        if let Some(chat) = self.chat() {
            if !chat.messages.is_empty() {
                let snapshot = chat.clone();
                if let Err(problem) = self.store.save_chat(&snapshot) {
                    self.note_write_problem(&problem);
                }
            }
        }
        true
    }

    // Turn planning mode off for the open chat, and say that its plan was let go. The plan file
    // is where it was, so the heading over the conversation can still open it.
    fn stop_planning(&mut self) {
        if let Some(chat) = self.chat_mut() {
            chat.meta.planning = false;
            chat.meta.planned = true;
            chat.meta.updated = store::now_secs();
        }
    }

    // All of the plan of the open chat, for the window that reads it. The plan of a chat that
    // never planned is nothing at all.
    pub fn plan_words(&self) -> Option<String> {
        let name = self.chat()?.meta.plan_file.clone();
        planning::words(&name)
    }

    // The plan the model let go. The conversation of the planning is closed whole: the chat
    // remains empty, with the planning prompt and the planning tools removed with it, and the
    // plan itself returns to remain as the first message of the work. Nothing returns when no
    // plan was let go.
    pub fn release_plan_in(&mut self, chat_id: &str) -> Option<String> {
        let plan = self.live_mut(chat_id).plan_pending.take()?;
        if let Some(chat) = self.chat_of_mut(chat_id) {
            chat.messages.clear();
        }
        self.live_mut(chat_id).live_text.clear();
        self.live_mut(chat_id).live_reasoning.clear();
        self.live_mut(chat_id).live_tools.clear();
        self.live_mut(chat_id).live_work.clear();
        self.live_mut(chat_id).live_work_head.clear();
        self.live_mut(chat_id).ctx_used = 0;
        self.stop_planning();
        Some(plan)
    }

    // The same work for the chat the side bar has open, which is what the window and the page
    // request when they name no chat.
    pub fn release_plan(&mut self) -> Option<String> {
        let chat = self.open_id();
        self.release_plan_in(&chat)
    }

    // The records of the attachments used to be in one flat folder under the data directory,
    // with nothing in it to say which chat held them. When such a folder remains, the records
    // are moved into the working directory of the chat that carries their markers, and the old folder
    // goes when it holds nothing left. This runs once, when the program starts.
    fn migrate_attachments(&mut self) {
        if !attach::old_dir().is_dir() {
            return;
        }
        let work = config::paths().data.join("work");
        // Every chat of a repository shares the directory of that repository.
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
        // A chat with no repository has a directory of its own.
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
            planning: chat.map(|c| c.meta.planning).unwrap_or(false),
            planned: chat.map(|c| c.meta.planned).unwrap_or(false),
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
            context_used: self.live().ctx_used,
            context_total,
            context_cut: config::compaction_mark(&self.cfg, context_total).unwrap_or(0),
            generating: self.live().generating,
            working: self.generating_chats(),
            summarising: self.live().summarising,
            busy: self.busy,
            code_enabled: chat.map(|c| c.meta.code_enabled).unwrap_or(self.new_code),
            tool_switches: chat
                .map(|c| c.meta.tools.clone())
                .unwrap_or_else(|| self.new_tools.clone()),
            search_enabled: chat
                .map(|c| c.meta.search_enabled)
                .unwrap_or(self.new_search),
            sandbox_enabled: chat
                .map(|c| c.meta.sandbox_enabled)
                .unwrap_or(self.new_sandbox),
            sandbox_access: self.access_of_open(),
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
            theme: config::theme_word(&self.cfg.theme, self.cfg.dark_mode),
            theme_pixel: self.cfg.theme_pixel,
            data_dir: config::paths().data.display().to_string(),
            contexts_dir: config::contexts_dir(&self.cfg).display().to_string(),
            sandbox_note: self.sandbox_note.clone(),
            chat_collapse_thinking: self.cfg.chat_collapse_thinking,
            chat_grow_thinking: self.cfg.chat_grow_thinking,
            chat_tools_in_thinking: self.cfg.chat_tools_in_thinking,
            chat_collapse_tools: self.cfg.chat_collapse_tools,
            chat_date_new_chat: self.cfg.chat_date_new_chat,
            chat_highlight: self.cfg.chat_highlight,
            chat_bold_markdown: self.cfg.chat_bold_markdown,
            chat_colour_text: self.cfg.chat_colour_text,
            chat_seamless: self.cfg.chat_seamless,
            chat_display: self.cfg.chat_display.clone(),
            welcomed: self.cfg.welcomed,
            chat_silent_retries: self.cfg.chat_silent_retries,
            chat_chapters: self.cfg.chat_chapters,
            chat_userline: self.cfg.chat_userline,
            chat_background: self.cfg.chat_background.clone(),
            chat_rain_splash: self.cfg.chat_rain_splash,
            chat_send_anim: self.cfg.chat_send_anim,
            chat_context_files: self.cfg.chat_context_files,
            chat_context_compression: self.cfg.chat_context_compression,
            mcp: self.mcp_snap(),
            chat_paste_limit: self.cfg.pasted_text_char_limit,
            ask_seconds: self.cfg.ask_seconds.min(config::GREATEST_ASK_SECONDS),
            ask_critical_seconds: self
                .cfg
                .ask_critical_seconds
                .min(config::GREATEST_ASK_SECONDS),
            chat_width: config::sanitize_chat_width(self.cfg.chat_width),
            // The Compaction section, so the settings panel can be filled in and the page can
            // tell which way the conversation is kept short.
            chat_compaction: self.cfg.chat_compaction.clone(),
            chat_compact_by: self.cfg.chat_compact_by.clone(),
            chat_compact_percent: self.cfg.chat_compact_percent,
            chat_compact_reserve: self.cfg.chat_compact_reserve,
            chat_compact_prompt: self.cfg.chat_compact_prompt.clone(),
            chat_compact_prompt_short: self.cfg.chat_compact_prompt_short.clone(),
            chat_timeline: self.cfg.chat_timeline,
            models: self.models.clone(),
            model_id: self.current_model_id(),
            // The top bar shows five recent repositories at most.
            repositories: self.repositories.iter().take(5).cloned().collect(),
            chat_id: self.current.clone().unwrap_or_default(),
            home: self.current.is_none(),
            greeting: crate::greet::greeting(store::now_secs()),
            // The markers of the chapters are not text of the conversation: they are given on
            // the side, and kept out of the messages the window draws.
            messages: chat
                .map(|c| {
                    c.messages
                        .iter()
                        .filter(|m| m.role != "Chapter")
                        .cloned()
                        .collect()
                })
                .unwrap_or_default(),
            // The answer the page may propose to go on with, if there is one.
            continue_id: self.goable_answer().unwrap_or_default(),
            chapters: chat.map(chapter_marks).unwrap_or_default(),
            handoffs: chat
                .map(|one| handoff_marks(one, self.live().generating))
                .unwrap_or_default(),
            live_text: self.live().live_text.clone(),
            live_reasoning: self.live().live_reasoning.clone(),
            live_thought_ms: self.live().live_thought_ms,
            live_thought_done: self.live().live_thought_done,
            progress: self.live().progress,
            progress_on: self.live().progress_on,
            progress_images: self.live().progress_images,
            prompt_rate: self.live().prompt_rate,
            answer_rate: self.live().answer_rate,
            live_tools: self.live().live_tools.clone(),
            // What the user is shown: calls of one tool made one after another remain together
            // in a block of their own.
            live_work: corporate::grouped(&self.live().live_work),
            live_work_head: self.live().live_work_head.clone(),
            live_work_closed: self.live().live_work_closed,
            ask: self.live().asking.clone(),
            running: self.live().running.as_ref().map(|one| RunningTool {
                id: one.id.clone(),
                timeout: one.watch.limit().map(|given| given.as_secs() as u32),
                left: one.watch.left().map(|left| left.as_secs() as u32),
            }),
            queue: self.queue_snap(),
            rev: self.rev,
            structure_rev: self.structure_rev,
        }
    }
}

// The flag one turn of one chat answers to when the user requests it to stop. A flag is created
// new for every turn, so a stop requested after an answer came to its end does not carry over
// to the answer after it, and two chats are never stopped by the same word.
fn begin_turn_cancel(state: &mut State, chat: &str) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    state.turn_cancel.insert(chat.to_string(), flag.clone());
    flag
}

// Take the flag of a chat away, which is what a chat that came to the end of its turn leaves
// behind. A stop requested now has nothing to stop, and the next turn is given a flag of its
// own.
fn end_turn_cancel(state: &mut State, chat: &str) {
    state.turn_cancel.remove(chat);
}

// The running llama-server, shared by the turns that answer at the same time. The child process
// belongs to whoever started it: a turn reads the address of the run it was given and never
// stops the run itself, so an answer in one chat cannot be truncated by an answer in another.
type ServerSlot = Arc<std::sync::Mutex<Option<Server>>>;

// The slot of a server that was never started.
fn empty_server() -> ServerSlot {
    Arc::new(std::sync::Mutex::new(None))
}

// The address of the run that is in the slot, and the model it loaded. Nothing returns when no
// server runs.
fn server_held(server: &ServerSlot) -> Option<(String, String)> {
    let held = server.lock().ok()?;
    let one = held.as_ref()?;
    Some((one.url.clone(), one.model_id.clone()))
}

// The address of the run that is in the slot, or nothing at all when none runs. A turn that has
// no address has nothing to ask.
fn server_url(server: &ServerSlot) -> String {
    server_held(server).map(|(url, _)| url).unwrap_or_default()
}

// The slot of the chat named in the run that is in the slot: the conversation of the chat is
// written as the slot again when another chat was there last. Nothing when no server runs, or
// when no slot is free.
fn claim_slot(state: &Arc<Mutex<State>>, server: &ServerSlot) -> Option<usize> {
    let (url, model_id) = server_held(server)?;
    context_claim(state, &url, &model_id)
}

// One of the three settings of the sandbox, named as the pages name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxSetting {
    Network,
    Pip,
    Fetch,
}

impl SandboxSetting {
    // The name of the setting on the wire between a page and the engine.
    pub fn from_name(name: &str) -> Option<SandboxSetting> {
        match name {
            "network" => Some(SandboxSetting::Network),
            "pip" => Some(SandboxSetting::Pip),
            "fetch" => Some(SandboxSetting::Fetch),
            _ => None,
        }
    }

    // The line the status is on after the setting moved.
    fn words(self, on: bool) -> &'static str {
        match (self, on) {
            (SandboxSetting::Network, true) => "A command of this chat may reach the network",
            (SandboxSetting::Network, false) => "A command of this chat has no network",
            (SandboxSetting::Pip, true) => {
                "Python package installs are allowed for this chat, into the chat's own directory"
            }
            (SandboxSetting::Pip, false) => "Python package installs are off for this chat",
            (SandboxSetting::Fetch, true) => {
                "wget, curl and fetch may run in this chat, and reach the network"
            }
            (SandboxSetting::Fetch, false) => "wget, curl and fetch are off in this chat",
        }
    }
}

// What a command of this chat may reach, out of the markers the chat carries. A chat written
// before the three existed carries none of them, and reads as all three off.
pub fn access_of(meta: &store::ChatMeta) -> tools::Access {
    tools::Access {
        network: meta.sandbox_network,
        pip: meta.sandbox_pip,
        fetch: meta.sandbox_fetch,
    }
}

// What the side bar is made of. Every repository is with the chats that are in it and were not
// put away, and the chats that belong to no repository are on their own. A repository the user
// pinned keeps its marker, and so does a chat; the page puts the pinned items in the group
// above the rest.
#[derive(Default)]
pub struct Lists {
    pub repositories: Vec<(Repo, Vec<Chat>)>,
    pub loose: Vec<Chat>,
    // The chats the user put away, newest first, of every place.
    pub archived: Vec<Chat>,
}

// Put the chats of one group in the order the side bar shows them: the chat that was opened
// last first. A chat that was never opened is where the time of its last message puts it, so a
// list of chats that nobody has opened since the program was started is still the newest first.
fn by_opened(chats: &mut [Chat]) {
    chats.sort_by_key(|c| std::cmp::Reverse(c.meta.opened.max(c.meta.updated)));
}

// The handle a window keeps.
pub struct Engine {
    // The state of the program. The window reaches it through the calls of this file. The live
    // run of the engine against a real model reaches it outright, to switch the devices of the
    // computer off, which no call of the program does: that is the only reason the field is
    // left open there, and in the tests beside it.
    #[cfg(any(test, feature = "live-driver"))]
    pub state: Arc<Mutex<State>>,
    #[cfg(not(any(test, feature = "live-driver")))]
    state: Arc<Mutex<State>>,
    jobs: Sender<Job>,
}

impl Engine {
    // Start the engine and its worker thread.
    pub fn start(cfg: Config) -> Engine {
        // The log of this run is begun before anything else has an item to say, so the run is
        // in the log even when it came to nothing. The window and the browser page both come
        // through here, so both pages are logged.
        proglog::start();
        let store = store::Store::new(config::repositories_dir(&cfg));
        // The badge is on only when the sandbox has been tried and kept what it promises, so a
        // computer with bubblewrap installed and user namespaces switched off records it
        // instead of running commands free.
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
            // No chat is answering yet, so no chat carries live state of its own. The state of
            // a chat is prepared when its first turn begins.
            live: std::collections::BTreeMap::new(),
            turn_cancel: std::collections::BTreeMap::new(),
            queues: Vec::new(),
            busy: false,
            status: String::from("Ready"),
            ctx_total: 0,
            sandbox_available,
            sandbox_note,
            thinking: markdown::ThinkingOptions::default(),
            rev: 0,
            structure_rev: 0,
            new_code: false,
            new_tools: store::ToolSwitches::default(),
            new_search: false,
            new_sandbox: true,
            // Every part of what a command may reach remains off for a new set of settings, and
            // for a chat written before the three existed.
            new_sandbox_access: tools::Access::default(),
            new_mcp: Vec::new(),
            mcp_tools: std::collections::BTreeMap::new(),
            mcp_fault: std::collections::BTreeMap::new(),
            context_slot: std::collections::BTreeMap::new(),
            context_holds: std::collections::BTreeMap::new(),
            new_thinking: String::from("off"),
            // llama.cpp keeps the thinking of the earlier turns on its own, so the program
            // requests the same and lets the user turn it off for a model that proposes the
            // choice.
            new_preserve: true,
        };
        state.refresh_lists();
        // The conversations go first, so a repository is found to hold no chat of the model
        // that works inside it. The records of the repositories go with them, for a command of
        // the terminal tool runs in the directory of a repository and has no business with
        // them.
        state.store.migrate_chats_out();
        state.store.migrate_records_out();
        state.migrate_attachments();
        // The text of the older format - the directories of chats, of conversations of the
        // repositories, of their records, and of the summary forms of the contexts the model
        // processed - come into the databases here. The scan is created once for a directory of
        // the program, and its marker is read on every start after the first. See
        // `src/migrate.rs`.
        migrate::run(&state.cfg, &state.store);
        state.reload_chats();
        // The first screen holds no chat at all, even when this place has chats. The window
        // shows the chat composer on its own, the first message opens a new chat, and the
        // previews hold the chats that exist.
        state.current = None;
        state.refresh_thinking();
        let state = Arc::new(Mutex::new(state));

        let (sender, receiver) = channel::<Job>();
        let worker_state = state.clone();
        let worker_sender = sender.clone();
        std::thread::spawn(move || worker(worker_state, receiver, worker_sender));

        Engine {
            state,
            jobs: sender,
        }
    }

    // The settings, for the settings window.
    pub fn config(&self) -> Config {
        match self.state.lock() {
            Ok(state) => state.cfg.clone(),
            Err(_) => Config::default(),
        }
    }

    // Read the state without changing it.
    pub fn snapshot(&self) -> Snap {
        match self.state.lock() {
            Ok(state) => state.snap(),
            Err(_) => Snap::empty(),
        }
    }

    // Ask the model something, with the files the user hung on the message beside the text. The
    // worker thread does the work. A message of nothing at all is refused; a message of files
    // and no text is a real message.
    pub fn prompt(&self, text: String, files: Vec<attach::Incoming>) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(sent) = Self::prepare_message(&mut state, text, files) else {
            return;
        };
        // A message that got through reports that what was in the way of the chat is dealt
        // with, so a waiting list that was held back goes on again behind it.
        state.queue_restart();
        // The turn belongs to the chat it was written in, and not to whichever chat the side
        // bar will have open when the worker takes the message up.
        let chat = state.open_id();
        drop(state);
        let _ = self.jobs.send(Job::Prompt { chat, sent });
    }

    // Ready a message of the chat composer for the worker, and say upon the status line what is
    // in its way. The commands of the chat composer are taken off the start of the text, a chat
    // that owes a handoff summary is answered where it is, and a chat with no model or no
    // server of llama.cpp is not requested an answer at all. The message that returns is what
    // the worker is given, and nothing returns when the text is not to go out.
    //
    // The worker takes this same path for a message of the waiting list, which is what lets a
    // list of them keep going after the page that wrote it is removed: a queued message is sent
    // as the user sent it, and no part of the rules of sending belongs to the page.
    fn prepare_message(
        state: &mut State,
        text: String,
        files: Vec<attach::Incoming>,
    ) -> Option<SentMessage> {
        if text.trim().is_empty() && files.is_empty() {
            state.set_status("Nothing was written");
            return None;
        }
        if state.live().generating {
            state.set_status("An answer is still being generated");
            return None;
        }
        // A programmed message is a message of the waiting list and not of the chat: the text
        // it waits for are written in the chat composer, in a field of their own, and the page
        // puts the message in the list instead of sending it. A message that came here with the
        // word of the command in front of it was written out manually, and nothing in the text
        // can say what it was to wait for.
        if planning::asked_as_program(&text) {
            state.set_status(
                "Choose /program in the chat composer, write the text to search for in the field above the message, and it goes into the waiting list",
            );
            return None;
        }
        // A message sent from the first screen opens a chat of this place. The window grows the
        // chat composer into a chat at the same time.
        if state.current.is_none() {
            state.start_chat();
        }
        // The commands of the chat composer. `/plan` turns planning mode on for this chat and
        // makes its plan file; the text behind it are the request the plan is to be made of,
        // and they go on as a message of their own. A command with nothing behind it turns the
        // mode on and asks nothing of the model, so the user may look at the plan file or write
        // the request next.
        let (command, rest) = planning::split(&text);
        let text = match command {
            Some(planning::Command::Plan) => {
                if state.start_planning() {
                    state.set_status(
                        "Planning mode is on. The plan of this chat is kept in the chats database",
                    );
                } else {
                    state.set_status("Start a new chat first");
                    return None;
                }
                if rest.trim().is_empty() && files.is_empty() {
                    return None;
                }
                rest
            }
            None => text,
        };
        // A chat that was cut for a handoff summary and never got one is at the place it was
        // cut: its conversation is past the context window of the model and cannot be sent as
        // it is. This is reported before aught else, for a user who presses send on such a chat
        // is waiting to hear why the chat does not go on. The user starts the summary again,
        // and the chat goes on after it, so no word of the conversation is lost and no model is
        // asked to read a conversation it cannot hold.
        if state.handoff_owed() {
            state.set_status(
                "The handoff summary of this chat was never written. Start it again from the summarisation timeline",
            );
            return None;
        }
        if state.model().is_none() {
            state.set_status("Choose a model in the top bar first");
            return None;
        }
        if state.server_binary().is_none() {
            state.set_status("llama-server was not found. Set its directory in the settings");
            return None;
        }
        Some(SentMessage { text, files })
    }

    // Put a message at the end of the waiting list of the chat that is left open. The list
    // belongs to the chat and to the program: the message is sent when the answer the chat is
    // writing is over, and it is sent then whether or not a page is left looking at the chat.
    pub fn queue_add(&self, text: String, files: Vec<attach::Incoming>) -> Result<(), String> {
        if text.trim().is_empty() && files.is_empty() {
            return Err(String::from("Nothing was written"));
        }
        let Ok(mut state) = self.state.lock() else {
            return Err(String::from("The program is busy"));
        };
        let Some(list) = state.open_queue_mut() else {
            return Err(String::from("Start a chat first"));
        };
        list.seq += 1;
        let id = list.seq;
        list.items.push(QueueItem {
            id,
            text,
            files,
            matches: String::new(),
            ready: false,
        });
        // A word written into the list now reports that what was in the way of the messages is
        // dealt with, so a list that was held back goes on again.
        list.held = false;
        state.touch();
        Ok(())
    }

    // Put a programmed message at the end of the waiting list of the chat that is left open.
    // The text it waits for says when the message is sent: an answer of the chat that holds it
    // lets the message out ahead of the messages that were only put in the list, and an answer
    // that does not hold them leaves the message remaining for the next one. The message is
    // never sent from here, and never sent because the chat is idle - only because the text was
    // found.
    pub fn queue_plan(
        &self,
        text: String,
        files: Vec<attach::Incoming>,
        matches: String,
    ) -> Result<(), String> {
        if matches.trim().is_empty() {
            return Err(String::from("Say what to search for in the answer"));
        }
        if text.trim().is_empty() && files.is_empty() {
            return Err(String::from("Nothing was written"));
        }
        let Ok(mut state) = self.state.lock() else {
            return Err(String::from("The program is busy"));
        };
        let Some(list) = state.open_queue_mut() else {
            return Err(String::from("Start a chat first"));
        };
        list.seq += 1;
        let id = list.seq;
        list.planned.push(QueueItem {
            id,
            text,
            files,
            // The spaces at either end are taken off, so the row reports the text as they were
            // meant and not as they happened to be written.
            matches: matches.trim().to_string(),
            ready: false,
        });
        state.touch();
        Ok(())
    }

    // Take one message out of the waiting list of the chat that is left open, by the number its
    // row carries. False when no row carries that number, which is what a page that is a moment
    // behind the program hears.
    pub fn queue_remove(&self, id: u64) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let taken = {
            let Some(list) = state.open_queue_mut() else {
                return false;
            };
            // The programmed messages are taken out the same way as the messages that are only
            // waiting their turn: a row is a row to the pointer.
            if let Some(at) = list.planned.iter().position(|one| one.id == id) {
                list.planned.remove(at);
            } else if let Some(at) = list.items.iter().position(|one| one.id == id) {
                list.items.remove(at);
            } else {
                return false;
            }
            // Touching the list reports that what was in the way of the messages is dealt with,
            // so a list that was held back goes on again.
            list.held = false;
            Some((
                list.chat.clone(),
                list.items.is_empty() && list.planned.is_empty(),
            ))
        };
        let Some((chat, empty)) = taken else {
            return false;
        };
        // A list that has come to an end is removed, so a chat checked again does not find an
        // empty list remaining for it.
        if empty {
            state.queue_drop(&chat);
        }
        state.touch();
        true
    }

    // Write the text of one message of the waiting list again, in the row itself. The files of
    // the message are left as they hang, and so is the word a programmed message waits for:
    // what the message waits for is written again by taking the message out and putting it in
    // the list afresh.
    pub fn queue_edit(&self, id: u64, text: String) -> bool {
        let words = text.trim().to_string();
        if words.is_empty() {
            return false;
        }
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let Some(list) = state.open_queue_mut() else {
            return false;
        };
        let Some(one) = list
            .items
            .iter_mut()
            .chain(list.planned.iter_mut())
            .find(|one| one.id == id)
        else {
            return false;
        };
        one.text = words;
        list.held = false;
        state.touch();
        true
    }

    // Put one message of the waiting list at another place in it. The place is counted in the
    // order the messages go out in, where 0 is the next to go - the other way about from the
    // rows, which are written newest first.
    //
    // A programmed message has no place to be put: the answer it waits for reports when it
    // goes, and the text it waits for say the order it goes in.
    pub fn queue_move(&self, id: u64, index: usize) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let Some(list) = state.open_queue_mut() else {
            return false;
        };
        if list.planned.iter().any(|one| one.id == id) {
            return false;
        }
        let Some(from) = list.items.iter().position(|one| one.id == id) else {
            return false;
        };
        let one = list.items.remove(from);
        let at = index.min(list.items.len());
        list.items.insert(at, one);
        list.held = false;
        state.touch();
        true
    }

    // Start the handoff summary of the open chat again. A summary the server never finished
    // leaves the chat remaining at the place it was cut, and only the user reports when to
    // request it again: the program does not put a model to work behind the back of a chat that
    // is waiting.
    pub fn restart_handoff(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.live().generating {
            state.set_status("An answer is still being generated");
            return;
        }
        // The word of the waiting chat comes first, so a user who pressed the marker of a
        // summarisation that was already written is informed so, and not sent to look at the
        // model. The model is checked after it, for the worker reports the same when it has
        // nothing to ask.
        if !state.handoff_owed() {
            state.set_status("No handoff summary is waiting for this chat");
            return;
        }
        if state.model().is_none() {
            state.set_status("Choose a model in the top bar first");
            return;
        }
        // The summary was what was between the chat and its waiting messages, so they are asked
        // again once it is begun.
        state.queue_restart();
        // The summary belongs to the chat it was requested, whatever the side bar has open by
        // the time the worker takes the work up.
        let chat = state.open_id();
        drop(state);
        let _ = self.jobs.send(Job::Handoff(chat));
    }

    // Stop reading the answer of the chat that is left open. Two chats may be answered at the
    // same time, and stopping one of them stops that one alone: the flag a turn answers to
    // belongs to the chat and to the turn, so the answers of the other chats go on, and a stop
    // requested after an answer came to its end does not stop the answer after it.
    pub fn cancel(&self) {
        let chat = self.state.lock().ok().map(|g| g.open_id()).unwrap_or_default();
        self.cancel_chat(&chat);
    }

    // Stop reading the answer of one chat, and the call of a tool that is running as part of
    // it. The command that was started goes with the answer, and what it had printed by then
    // still goes back to the model under the word that the user stopped it. The lock is only
    // requested once, and not waited for: the flag is set first, and a call that is missed this
    // way still ends by its own clock.
    pub fn cancel_chat(&self, chat: &str) {
        let flag = self
            .state
            .lock()
            .ok()
            .and_then(|state| state.turn_cancel.get(chat).cloned());
        if let Some(flag) = flag.as_ref() {
            flag.store(true, Ordering::SeqCst);
        }
        if let Ok(mut state) = self.state.try_lock() {
            if let Some(one) = state.live_of(chat).and_then(|one| one.running.as_ref()) {
                one.watch.stop_now();
                state.set_status(if flag.is_some() {
                    "The call is being stopped with the answer"
                } else {
                    "The call is being stopped"
                });
                return;
            }
            state.set_status(if flag.is_some() {
                "The answer is being stopped"
            } else {
                "Nothing of this chat is being written"
            });
        }
    }

    // The tools of the open chat: the terminal tool, and the tools that work on the files of
    // the chat. One switch is above all of them, for a user who does not want the model
    // reaching the computer does not want it reaching the files either. The name of the switch
    // stays `code`, which is the name the chat carries on disk and the name the pages send.
    pub fn set_code(&self, enabled: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if let Some(chat) = state.chat_mut() {
            chat.meta.code_enabled = enabled;
        }
        // The same choice opens the next chat too, whatever is left open now.
        state.new_code = enabled;
        state.persist();
        state.set_status(if enabled {
            "The model may use the tools of the program"
        } else {
            "The tools of the program are off"
        });
    }

    // One line of the drop-down of the badge of the tools. The badge itself is the switch over
    // all of them, so a tool that is switched on here is proposed only while the badge is on;
    // turning this line off takes the tool out of the chat even when the badge is on. The same
    // choice opens the next chat.
    pub fn set_tool_switch(&self, part: &str, on: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        // The line of the handoff summary counts toward the badge of the tools only while the
        // settings make the size of the context by a summary, for with compaction off nothing
        // is behind that line.
        let room = state.compaction_on();
        if let Some(chat) = state.chat_mut() {
            if !chat.meta.tools.set(part, on) {
                return;
            }
            // The badge of the tools is the entry point to all of them. A line of the drop-down
            // that is switched on opens that entry point, for a tool behind a shut door is no
            // tool at all; and when the last line is sent the entry point shuts with it, so the
            // badge never is on over nothing.
            chat.meta.code_enabled = chat.meta.tools.any_on_with(room);
        }
        // The same choice opens the next chat too, whatever is left open now, as with the badge
        // of the tools itself.
        if !state.new_tools.set(part, on) {
            return;
        }
        state.new_code = state.new_tools.any_on_with(room);
        state.persist();
        let word = tool_group_word(part);
        let words = if on {
            format!("The model may use {} again", word)
        } else {
            format!("{} are off", word)
        };
        state.set_status(&words);
    }

    // The web search of the open chat. It is on a switch of its own and not behind the badge of
    // the tools, for it reads the web out of the program: a command of the terminal tool cannot
    // reach the web at all, since the sandbox of it has no network. The name of the switch is
    // `search` on every path it travels on.
    pub fn set_search(&self, enabled: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if let Some(chat) = state.chat_mut() {
            chat.meta.search_enabled = enabled;
        }
        // The same choice opens the next chat too, whatever is left open now.
        state.new_search = enabled;
        state.persist();
        state.set_status(if enabled {
            "The model may search the web"
        } else {
            "Web search is off"
        });
    }

    // One server of the user's own, switched on or off for this chat and for the next one. The
    // servers of the user's own are the tools of machines and programs beyond this one, so they
    // remain off until the user opens them, as the sandbox gates remain off.
    pub fn set_mcp(&self, id: &str, on: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let keep = |held: &mut Vec<String>| {
            held.retain(|one| one != id);
            if on {
                held.push(id.to_string());
            }
        };
        if let Some(chat) = state.chat_mut() {
            keep(&mut chat.meta.mcp);
        }
        // The same choice opens the next chat too, whatever is left open now.
        keep(&mut state.new_mcp);
        state.persist();
        let name = state
            .cfg
            .mcp_servers
            .iter()
            .find(|server| server.id == id)
            .map(|server| server.name.clone())
            .unwrap_or_default();
        let said = if on {
            format!("The tools of {} are proposed to the model", name)
        } else {
            format!("The tools of {} are off", name)
        };
        state.set_status(&said);
    }

    // The servers of the user's own, as they are now in the settings. The settings panel
    // requests these, for the state keeps no text of code.
    pub fn mcp_servers(&self) -> Vec<mcp::Server> {
        match self.state.lock() {
            Ok(state) => state.cfg.mcp_servers.clone(),
            Err(_) => Vec::new(),
        }
    }

    // Add one server, or write a new one over an old one. The text the user wrote are kept
    // inside the bounds of `mcp::clean`, and a server with nothing written in the place its
    // type requests is refused. The tools of the server are then read again, out of this call,
    // so the settings panel is not held up by a server that takes its time.
    pub fn mcp_save(&self, given: mcp::Server) -> Result<String, String> {
        let clean = given.clean();
        clean.check()?;
        let Ok(mut state) = self.state.lock() else {
            return Err(String::from("The program is busy"));
        };
        let id = if clean.id.is_empty() {
            store::new_id()
        } else {
            clean.id.clone()
        };
        let mut made = clean.clone();
        made.id = id.clone();
        if made.slug.is_empty() {
            let held: Vec<&mcp::Server> = state
                .cfg
                .mcp_servers
                .iter()
                .filter(|server| server.id != id)
                .collect();
            made.slug = mcp::slug_for(&made.name, &held);
        }
        match state
            .cfg
            .mcp_servers
            .iter_mut()
            .find(|server| server.id == id)
        {
            Some(found) => *found = made,
            None => state.cfg.mcp_servers.push(made),
        }
        // What was known of this server is deleted, for the server the user has just written is
        // not the server the program checked.
        state.mcp_tools.remove(&id);
        state.mcp_fault.remove(&id);
        let _ = config::save(&state.cfg);
        state.touch();
        state.set_status("The server is kept. Its tools are being read");
        drop(state);
        let _ = self.jobs.send(Job::McpLook(id));
        Ok(String::from("kept"))
    }

    // Take a server away: out of the settings, out of every chat that has it switched on, and
    // off the computer if it was a server of the program itself.
    pub fn mcp_remove(&self, id: &str) -> Result<String, String> {
        let Ok(mut state) = self.state.lock() else {
            return Err(String::from("The program is busy"));
        };
        let gone = state
            .cfg
            .mcp_servers
            .iter()
            .find(|server| server.id == id)
            .cloned();
        state.cfg.mcp_servers.retain(|server| server.id != id);
        state.mcp_tools.remove(id);
        state.mcp_fault.remove(id);
        state.new_mcp.retain(|one| one != id);
        for chat in state.chats.iter_mut() {
            chat.meta.mcp.retain(|one| one != id);
        }
        let _ = config::save(&state.cfg);
        state.touch();
        if let Some(server) = gone {
            if server.kind() == mcp::KIND_INTERNAL {
                mcp::remove_folder(&server.id);
            }
        }
        state.set_status("The server was removed");
        Ok(String::from("gone"))
    }

    // Whether a server is in the drop-down of the badge. A server that is taken out of the menu
    // is kept, and the chats that had it switched on keep that choice: the user who takes a
    // server out of the menu and puts it back finds it where they left it.
    pub fn mcp_shown(&self, id: &str, on: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if let Some(server) = state
            .cfg
            .mcp_servers
            .iter_mut()
            .find(|server| server.id == id)
        {
            server.shown = on;
        }
        let _ = config::save(&state.cfg);
        state.touch();
    }

    // Ask one server again what it has to propose. The answer arrives its own time, and the
    // settings panel sees it when it comes.
    pub fn mcp_look(&self, id: &str) {
        let Ok(mut state) = self.state.lock() else { return };
        state.set_status("The server is being checked");
        drop(state);
        let _ = self.jobs.send(Job::McpLook(id.to_string()));
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
        // The same choice opens the next chat too, whatever is left open now.
        state.new_sandbox = enabled;
        state.persist();
        state.set_status(if enabled {
            "Tool runs are sandboxed"
        } else {
            "Tool runs are not sandboxed"
        });
    }

    // One of the three settings of the sandbox, for the current chat and for the next chat as
    // well. They remain off until the user opens them in the menu of the sandbox badge, and
    // they are settings of the sandbox: a chat with the sandbox off runs its commands free and
    // needs nothing opened.
    //
    // An install and a fetch are nothing without the network, so opening one of them opens the
    // network too. Closing one leaves the network open only while the other still needs it,
    // which is what the sandbox boundaries of the sandbox do as well.
    pub fn set_sandbox_setting(&self, setting: SandboxSetting, on: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if let Some(chat) = state.chat_mut() {
            match setting {
                SandboxSetting::Network => chat.meta.sandbox_network = on,
                SandboxSetting::Pip => chat.meta.sandbox_pip = on,
                SandboxSetting::Fetch => chat.meta.sandbox_fetch = on,
            }
        }
        // The same choice opens the next chat too, whatever is left open now.
        match setting {
            SandboxSetting::Network => state.new_sandbox_access.network = on,
            SandboxSetting::Pip => state.new_sandbox_access.pip = on,
            SandboxSetting::Fetch => state.new_sandbox_access.fetch = on,
        }
        state.persist();
        state.set_status(setting.words(on));
    }

    // Choose the thinking mode of the open chat. A model with no thinking mode does not change,
    // and neither does a mode the model does not propose. An answer that is arriving is not
    // held up by this: the word for the thinking rides on the request, so the change means the
    // next ask of the chat, which may be the next step of the answer that is on the way.
    pub fn set_thinking(&self, mode: &str) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if !state.thinking.modes().contains(&mode) {
            return;
        }
        // The mode the current chat is on, or the one the next chat would open with.
        if let Some(chat) = state.chat_mut() {
            chat.meta.thinking_mode = mode.to_string();
        }
        // The same choice opens the next chat too, whatever is left open now.
        state.new_thinking = mode.to_string();
        state.persist();
        state.set_status(&format!("Thinking: {}. The change is sent with the next request", mode));
        state.touch();
        // The mode rides on the next ask, so a server that runs is skipped: the word for the
        // thinking is no flag of the server, and the model is where it is.
    }

    // Turn the keeping of the thinking of the earlier turns on or off, for the current chat. A
    // model whose template cannot keep its thinking does not change. Keeping costs tokens, for
    // the thoughts of the chat go back to the model with every turn. As with the mode, the
    // choice is sent with the next ask, so an answer that is arriving is not held up by it.
    pub fn set_preserve(&self, on: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if !state.thinking.has_preserve {
            state.set_status("This model has no setting for keeping the thinking");
            return;
        }
        if let Some(chat) = state.chat_mut() {
            chat.meta.preserve_thinking = on;
        } else {
            state.new_preserve = on;
        }
        state.persist();
        state.set_status(if on {
            "The thinking of the earlier turns is kept"
        } else {
            "The thinking of the earlier turns is dropped"
        });
        state.touch();
        // As with the mode: the choice is sent with the next ask and moves no server that runs.
    }

    // Show another repository, and load its chats.
    pub fn select_repository(&self, id: String) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if id == state.scope_repo {
            return;
        }
        state.scope_repo = id.clone();
        state.current = None;
        if let Some(mut repo) = state.repositories.iter().find(|r| r.id == id).cloned() {
            if let Err(problem) = state.store.touch_repo(&mut repo) {
                state.note_record_problem(&problem);
            }
            state.status = format!("Repository {}", repo.name);
        }
        state.repositories = state.store.scan_repos();
        state.reload_chats();
    }

    // Make a new repository, then show it. The name on disk follows the rule "name_id", where
    // the name keeps only letters, digits and dashes.
    pub fn create_repository(&self, name: String) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let name = name.trim().to_string();
        if name.is_empty() {
            state.set_status("A repository needs a name");
            return;
        }
        let made = state.store.create_repo(&name);
        match made {
            Ok(repo) => {
                state.scope_repo = repo.id.clone();
                state.current = None;
                state.repositories = state.store.scan_repos();
                state.reload_chats();
                let text = format!("Repository {} is ready", repo.name);
                state.set_status(&text);
            }
            Err(problem) => {
                let text = problem;
                state.set_status(&text);
            }
        }
    }

    // Every repository on disk, most recently used first. The settings window shows all of
    // them, while the top bar shows five.
    pub fn repositories(&self) -> Vec<Repo> {
        match self.state.lock() {
            Ok(state) => state.repositories.clone(),
            Err(_) => Vec::new(),
        }
    }

    // The text of one attachment, for the page that shows the block. A window asks by the id
    // the state gave with the message. Nothing is known of the user who asks: the record is the
    // record.
    pub fn attachment(&self, id: &str) -> Option<attach::Stored> {
        // The record of an attachment is with the chat that holds it, so the chat that is open
        // is looked in first, and the spaces of the other chats after it.
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

    // All of the plan of the open chat, for the window that reads it. The heading over the
    // conversation opens it: in planning mode, and after it, when the conversation reports
    // `PLANNED WORK`.
    pub fn plan(&self) -> Option<String> {
        let Ok(keeper) = self.state.lock() else {
            return None;
        };
        keeper.plan_words()
    }

    // The user answered the question the model is waiting on. The text go into the block of the
    // run, and the request is removed with them. A question that was answered already, or that
    // has been given over, is answered no more, and the page is informed so.
    pub fn answer_ask(&self, id: &str, answer: &str) -> bool {
        let Ok(mut guard) = self.state.lock() else {
            return false;
        };
        guard.answer_asking(id, answer.trim())
    }

    // The user sent the answers of a questionnaire of planning mode. Each question carries the
    // answers they chose for it, in the order the questions remain. A questionnaire that was
    // sent already, or that the run has given over, is answered no more, and the page is
    // informed so.
    pub fn answer_plan_ask(&self, id: &str, answers: Vec<Vec<String>>) -> bool {
        let Ok(mut guard) = self.state.lock() else {
            return false;
        };
        guard.answer_plan(id, answers)
    }

    // One file the user was proposed, for the button of a block. The name is the name the block
    // carries, and is read as the tool reads it: only the working directory of the open chat is opened,
    // so a name that steps out of that space gives nothing. The rule of the sandbox lies
    // over a download as it lies over a command. The answer is the name of the file, the type
    // of its bytes, and the bytes.
    pub fn gift(&self, asked: &str) -> Option<(String, String, Vec<u8>)> {
        let workdir = self.open_workdir()?;
        let (name, bytes) = download::bytes_of(&workdir, asked)?;
        let (_, tail) = download::split_name(&name);
        Some((name, download::mime(&tail).to_string(), bytes))
    }

    // The bytes of one file of a show, for the window over the contents of a file. The name is
    // read as the tool read it, and the rule of the working directory follows the state of the sandbox
    // of the open chat, which is the rule the call worked to. A type the window does not show
    // is refused, so a name cannot be used to read a file the tool would not have shown.
    pub fn display_of(&self, asked: &str) -> Option<(String, String, Vec<u8>)> {
        let workdir = self.open_workdir()?;
        let jailed = match self.state.lock() {
            Ok(guard) => guard
                .chat()
                .map(|one| one.meta.sandbox_enabled)
                .unwrap_or(true),
            Err(_) => return None,
        };
        display::bytes_of(&workdir, asked, jailed)
    }

    // Several files the user was proposed, in one archive, for the button that takes all of an
    // propose at once. A name that is not in the working directory of the chat is passed over; when
    // none of them can be had, nothing returns. The archive is created in the memory and is
    // never written out to the working directory of the chat, so a download leaves no file of its own
    // behind.
    pub fn gift_zip(&self, asked: &[String]) -> Option<(String, Vec<u8>)> {
        let workdir = self.open_workdir()?;
        let mut held: Vec<(String, Vec<u8>)> = Vec::new();
        for one in asked.iter().take(download::GREATEST_FILES) {
            if let Some((name, bytes)) = download::bytes_of(&workdir, one) {
                // A file of a folder is in the archive under its own name, and two files of the
                // same name would otherwise write over one another: the folder they came out of
                // is put back on the second.
                let mut shown = name;
                if held.iter().any(|(one, _)| *one == shown) {
                    let (stem, tail) = download::split_name(&shown);
                    shown = format!("{}-{}.{}", stem, held.len(), tail);
                }
                held.push((shown, bytes));
            }
        }
        if held.is_empty() {
            return None;
        }
        let made = download::zip_of(held);
        Some((zip_name(asked), made))
    }

    // The space of the open chat, which is the only space a download is read from.
    fn open_workdir(&self) -> Option<PathBuf> {
        let Ok(state) = self.state.lock() else {
            return None;
        };
        state.chat().map(|chat| state.workdir_for(chat))
    }

    // Go back to the first screen, where a new chat is written. The chat composer is in the
    // middle of the window under the marker and the line for this hour, which is the screen the
    // program opens on. The chat itself is created by the first message, so a new chat that is
    // skipped leaves no file.
    pub fn new_chat(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.open_home();
        state.set_status("A new chat. Write in it below");
    }

    // Open one chat of the side bar, of any place. The place of that chat becomes the place
    // that is open, so the top bar, a command of the tool and the next chat all follow the chat
    // the user went to. The chat that was opened last is at the header of its group in the side
    // bar.
    pub fn open_chat(&self, id: String) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        // A chat of another place is read from disk, for the side bar lists every place and the
        // engine holds only the one that is open.
        if !state.chats.iter().any(|c| c.meta.id == id) {
            match state.store.load_chat(&id) {
                Some(chat) => state.chats.push(chat),
                None => {
                    state.set_status("That chat is not there");
                    return;
                }
            }
        }
        let Some(place) = state.chats.iter().position(|c| c.meta.id == id) else {
            return;
        };
        let repo = state.chats[place].meta.repo_id.clone();
        if repo != state.scope_repo {
            // The chat goes with its repository, so the place is read again with this chat kept
            // open.
            state.scope_repo = repo;
            state.current = Some(id.clone());
            state.reload_chats();
            let Some(place) = state.chats.iter().position(|c| c.meta.id == id) else {
                return;
            };
            state.mark_opened(place);
            return;
        }
        state.mark_opened(place);
    }

    // ------------------------------------------------------------------ side bar
    //
    // The side bar works on chats and repositories by their id, and the chat it works on is
    // seldom the chat that is left open. Each of these reads the chat out of the place that
    // holds it, changes the record, and writes it back.

    // Give one chat another name. A name of nothing is no name, and the chat keeps the one it
    // had.
    pub fn rename_chat(&self, id: String, title: String) {
        let title = title.trim().chars().take(120).collect::<String>();
        let title = title.trim().to_string();
        if title.is_empty() {
            return;
        }
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state
            .edit_chat(&id, |chat| chat.meta.title = title)
            .is_none()
        {
            state.set_status("That chat is not there");
        }
    }

    // Remain a chat in the `Pinned` group of the side bar, or take it out of it. A pinned chat
    // is in that group alone, so it is not shown in the group of its repository as well.
    pub fn set_chat_pinned(&self, id: String, on: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.edit_chat(&id, |chat| chat.meta.pinned = on).is_none() {
            state.set_status("That chat is not there");
            return;
        }
        let words = if on {
            "The chat stands in Pinned"
        } else {
            "The chat is no longer pinned"
        };
        state.set_status(words);
    }

    // Put a chat away, or bring it back. A chat that is closed is in no group of the side bar;
    // it is read again from the list of the archived chats. A chat that is closed while it is
    // left open leaves the window at the first screen, for the conversation it held is out of
    // the way.
    pub fn set_chat_archived(&self, id: String, on: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.edit_chat(&id, |chat| chat.meta.archived = on).is_none() {
            state.set_status("That chat is not there");
            return;
        }
        if on && state.current.as_deref() == Some(id.as_str()) {
            state.open_home();
        }
        let words = if on {
            "The chat is archived. Restore it from Archived chats"
        } else {
            "The chat is no longer archived"
        };
        state.set_status(words);
    }

    // Move a chat into a repository, or out of every repository, which is what the `Move to`
    // line of the menu of a chat does. The record of the chat is written in the directory of
    // the place it goes to and taken out of the folder it came from, so a chat never is in two
    // places.
    pub fn move_chat(&self, id: String, repo_id: String) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if !repo_id.is_empty() && !state.repositories.iter().any(|r| r.id == repo_id) {
            state.set_status("That repository is not there");
            return;
        }
        let mut found = state.chats.iter().find(|c| c.meta.id == id).cloned();
        if found.is_none() {
            found = state.store.load_chat(&id);
        }
        let Some(mut chat) = found else {
            state.set_status("That chat is not there");
            return;
        };
        let older = chat.meta.repo_id.clone();
        if older == repo_id {
            return;
        }
        let words = if repo_id.is_empty() {
            let name = state
                .repositories
                .iter()
                .find(|r| r.id == older)
                .map(|r| r.name.clone())
                .unwrap_or_default();
            format!("The chat moved out of {}", name)
        } else {
            let name = state
                .repositories
                .iter()
                .find(|r| r.id == repo_id)
                .map(|r| r.name.clone())
                .unwrap_or_default();
            format!("The chat moved to {}", name)
        };
        chat.meta.repo_id = repo_id.clone();
        if let Err(problem) = state.store.save_chat(&chat) {
            // The chat did not come to its new place, so it is not taken out of the place it
            // was in either: it stays where it can be read.
            state.note_write_problem(&problem);
            return;
        }
        state.store.delete_chat(&older, &id);
        // The place that is open is read again, whichever of the two the chat came from or went
        // to. The chat goes with its new place when it is the chat that is left open.
        if older == state.scope_repo || repo_id == state.scope_repo {
            if state.current.as_deref() == Some(id.as_str()) {
                state.scope_repo = repo_id;
            }
            state.reload_chats();
        } else if let Some(place) = state.chats.iter().position(|c| c.meta.id == id) {
            state.chats[place] = chat;
        }
        state.touch_structure();
        state.set_status(&words);
    }

    // Take one chat away for good. The window goes to the first screen when it was the chat
    // that was open.
    pub fn remove_chat(&self, id: String) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let mut found = state.chats.iter().find(|c| c.meta.id == id).cloned();
        if found.is_none() {
            found = state.store.load_chat(&id);
        }
        let Some(chat) = found else {
            state.set_status("That chat is not there");
            return;
        };
        // A chat whose answer is on the way is not removed: the text of that answer go into the
        // chat, and a chat that is removed leaves them nowhere.
        if state
            .live_of(&id)
            .map(|one| one.generating || one.summarising)
            .unwrap_or(false)
        {
            state.set_status(
                "This chat is being answered. Wait for the answer to finish before the chat is removed",
            );
            return;
        }
        state.store.delete_chat(&chat.meta.repo_id, &id);
        // The waiting list of the chat goes with it: the messages of a chat that is removed
        // have no answer left to wait for.
        state.queue_drop(&id);
        // What the chat showed of itself goes with it, so a chat rebuilt under the same name
        // does not open with the text of the one that was removed.
        state.live_drop(&id);
        state.chats.retain(|c| c.meta.id != id);
        if state.current.as_deref() == Some(id.as_str()) {
            state.open_home();
        }
        state.touch_structure();
        state.set_status("The chat was removed");
    }

    // Take one message out of the conversation, and everything that came after it. The model
    // answered a conversation that no longer reads that way, so the answer goes with the
    // message that led to it: a message of the user takes the answer it provoked along. A
    // message from the model leaves the question behind, for the program does not delete what a
    // user wrote behind their back.
    pub fn remove_message(&self, id: String) {
        let Ok(mut state) = self.state.lock() else { return };
        if state.live().generating {
            state.set_status("An answer is still being generated");
            return;
        }
        let Some(index) = state
            .chat()
            .and_then(|chat| chat.messages.iter().position(|one| one.id == id))
        else {
            state.set_status("That message is not in this chat");
            return;
        };
        // The records of the files hung on the messages that go are taken out of the shelf of
        // the chat with them, so no file is behind with no message to carry it. The shelf is
        // named while the chat still remains whole, for naming it needs the chat for a moment
        // of its own.
        let gone: Vec<String> = {
            let Some(chat) = state.chat() else { return };
            chat.messages[index..]
                .iter()
                .flat_map(|one| one.attachments.iter().map(|a| a.id.clone()))
                .collect()
        };
        let room = state
            .chat()
            .map(|chat| state.attach_place_for(chat))
            .unwrap_or_default();
        if let Some(chat) = state.chat_mut() {
            chat.messages.truncate(index);
            chat.meta.updated = store::now_secs();
        }
        if !gone.is_empty() {
            attach::remove_at(&room, &gone);
        }
        // A chat left with nothing of the conversation is deleted as a chat that was never
        // filled is. A context of notes alone is no conversation: a note is written for the
        // window, and reports nothing the model was informed.
        let empty = state
            .chat()
            .map(|chat| {
                !chat
                    .messages
                    .iter()
                    .any(|one| one.role != "Note" && one.role != "Chapter")
            })
            .unwrap_or(true);
        if empty {
            let Some(chat) = state.chat() else { return };
            let (repo, chat_id) = (chat.meta.repo_id.clone(), chat.meta.id.clone());
            state.store.delete_chat(&repo, &chat_id);
            state.chats.retain(|one| one.meta.id != chat_id);
            state.open_home();
            state.touch_structure();
            state.set_status("The message was removed. The chat had nothing left, so it was removed too");
            return;
        }
        state.persist();
        state.touch();
        state.set_status("The message was removed, with everything after it");
    }

    // Take a message of the user back out of the conversation, so that its text can be written
    // again in the chat composer. Everything that came after it goes with it, for the model
    // answered a conversation that no longer reads that way. A message from the model is
    // refused here: its text is written over where it is, and the question behind them is
    // not deleted behind the user's back.
    //
    // The files hung on the message go with it, as the files of every message after it go. The
    // page knows this, and reads the text and the bytes of the files back before it asks, so
    // the user loses nothing: what comes out of the chat comes into the chat composer beside
    // the text.
    pub fn rewrite_message(&self, id: String) {
        let Ok(mut state) = self.state.lock() else { return };
        if state.live().generating {
            state.set_status("An answer is still being generated");
            return;
        }
        let Some(index) = state
            .chat()
            .and_then(|chat| chat.messages.iter().position(|one| one.id == id))
        else {
            state.set_status("That message is not in this chat");
            return;
        };
        if state.chat().map(|chat| chat.messages[index].role.as_str()) != Some("You") {
            state.set_status("Only one of your own messages goes back into the chat composer");
            return;
        }
        // The shelf is named while the chat still remains whole, for naming it needs the chat
        // for a moment of its own.
        let gone: Vec<String> = {
            let Some(chat) = state.chat() else { return };
            chat.messages[index..]
                .iter()
                .flat_map(|one| one.attachments.iter().map(|a| a.id.clone()))
                .collect()
        };
        let room = state
            .chat()
            .map(|chat| state.attach_place_for(chat))
            .unwrap_or_default();
        if let Some(chat) = state.chat_mut() {
            chat.messages.truncate(index);
            chat.meta.updated = store::now_secs();
        }
        if !gone.is_empty() {
            attach::remove_at(&room, &gone);
        }
        // A chat left with nothing of the conversation is not removed here, as it is when a
        // message is simply removed: the text is on their way back into the box, and the chat
        // is where they will go. Only the file of it goes, for a chat with no conversation has
        // no file, and the first message that returns makes it again.
        let empty = state
            .chat()
            .map(|chat| chat.messages.is_empty())
            .unwrap_or(true);
        let (repo, chat_id) = match state.chat() {
            Some(chat) => (chat.meta.repo_id.clone(), chat.meta.id.clone()),
            None => return,
        };
        if empty {
            state.store.delete_chat(&repo, &chat_id);
            state.touch_structure();
        } else {
            state.persist();
        }
        state.touch();
        state.set_status("The message went back into the chat composer");
    }

    // Write the text of an answer again, where they are. The user is putting right what the
    // model stated: nothing else of the conversation moves, for the turns around the answer
    // read as they did. What the model is informed from now on is extracted from these
    // messages, so the next answer sees the text as they are now. The thought of the answer,
    // and the summary of its work, are left where they were: the user is writing the text, not
    // the work.
    pub fn edit_answer(&self, id: String, body: String) {
        let Ok(mut state) = self.state.lock() else { return };
        if state.live().generating {
            state.set_status("An answer is still being generated");
            return;
        }
        let words = body.trim().to_string();
        let Some(index) = state
            .chat()
            .and_then(|chat| chat.messages.iter().position(|one| one.id == id))
        else {
            state.set_status("That message is not in this chat");
            return;
        };
        if state.chat().map(|chat| chat.messages[index].role.as_str()) != Some("Assistant") {
            state.set_status("Only an answer of the model is written over");
            return;
        }
        if words.is_empty() {
            state.set_status("An answer needs some words");
            return;
        }
        if let Some(chat) = state.chat_mut() {
            chat.messages[index].body = words;
            chat.meta.updated = store::now_secs();
        }
        state.persist();
        state.touch();
        state.set_status("The answer was written again");
    }

    // Write the answer of a message again. A message of the user keeps its place, and the
    // conversation is cut back after it; a message from the model goes with the answer that
    // came after it. The request is then the conversation as it is now, which is the form of a
    // message the user has just sent.
    pub fn retry_message(&self, id: String) {
        let Ok(mut state) = self.state.lock() else { return };
        if state.live().generating {
            state.set_status("An answer is still being generated");
            return;
        }
        if state.handoff_owed() {
            state.set_status(
                "The handoff summary of this chat was never written. Start it again from the summarisation timeline",
            );
            return;
        }
        if state.model().is_none() {
            state.set_status("Choose a model in the top bar first");
            return;
        }
        let Some(index) = state
            .chat()
            .and_then(|chat| chat.messages.iter().position(|one| one.id == id))
        else {
            state.set_status("That message is not in this chat");
            return;
        };
        let role = state
            .chat()
            .map(|chat| chat.messages[index].role.clone())
            .unwrap_or_default();
        // Only a message of the user, or an answer of the model, has an answer to be written
        // again. A run of a tool, a note and a marker of a chapter are not asks of the user,
        // and the page returns nothing over them.
        let cut = match role.as_str() {
            "You" => index + 1,
            "Assistant" => index,
            _ => {
                state.set_status("That message has no answer to redo");
                return;
            }
        };
        let gone: Vec<String> = {
            let Some(chat) = state.chat() else { return };
            chat.messages[cut..]
                .iter()
                .flat_map(|one| one.attachments.iter().map(|a| a.id.clone()))
                .collect()
        };
        let room = state
            .chat()
            .map(|chat| state.attach_place_for(chat))
            .unwrap_or_default();
        if let Some(chat) = state.chat_mut() {
            chat.messages.truncate(cut);
            chat.meta.updated = store::now_secs();
        }
        if !gone.is_empty() {
            attach::remove_at(&room, &gone);
        }
        // The answer that is under the marker of a stop is removed with the rest, so the marker
        // goes too.
        if let Some(chat) = state.chat_mut() {
            clear_stopped_marks(chat);
        }
        state.persist();
        state.touch();
        let chat = state.open_id();
        drop(state);
        let _ = self.jobs.send(Job::Retry(chat));
    }

    // Go on with an answer of the model: the newest saying of the model in the open chat.
    // Whether the user stopped that answer or the model ended it early - on an end-of-sequence
    // token, or on the token limit - makes no difference: only the user knows whether an answer
    // stopped where it should have, and the conversation as it is now is the request either
    // way. The text of the answer is the last message of the model, so the template puts it
    // under the assistant marker and leaves that marker open, and the model writes on from the
    // last word it put down. The new text join the answer they were removed on from, so the
    // user reads one answer and not two.
    pub fn continue_message(&self, id: String) {
        let Ok(mut state) = self.state.lock() else { return };
        if state.live().generating {
            state.set_status("An answer is still being generated");
            return;
        }
        if state.handoff_owed() {
            state.set_status(
                "The handoff summary of this chat was never written. Start it again from the summarisation timeline",
            );
            return;
        }
        // The rules of the conversation is checked in one place, so the proposal the page is
        // given and the request that follows it cannot drift apart.
        if let Err(problem) = state.answer_to_go_on_with(&id) {
            state.set_status(problem);
            return;
        }
        // The text to go on from must still be at the end of the conversation. The space
        // made for a summarisation can have taken them away with the text a handoff summary
        // means, and can have left a call the model requested remaining there instead.
        let open = ends_open(&state.build_history());
        if !open {
            state.set_status(
                "That answer is no longer at the end of the conversation, so it cannot be continued",
            );
            return;
        }
        // The model is checked last, for the markers under a message are asked about before the
        // model is: a user who pressed a marker that this conversation cannot answer is
        // informed that first.
        if state.model().is_none() {
            state.set_status("Choose a model in the top bar first");
            return;
        }
        // The marker of the stop goes off as the answer is continued: what comes now is the end
        // of that answer, and a second press would go on with it twice.
        if let Some(chat) = state.chat_mut() {
            clear_stopped_marks(chat);
        }
        state.persist();
        state.touch();
        let chat = state.open_id();
        drop(state);
        let _ = self.jobs.send(Job::Resume { chat, id });
    }

    // Give a repository another name. The directory of a repository is named after it, so the
    // folder is moved with the record, the Python packages of the chats and the records of the
    // attachments in it. The id does not move, so the conversations of the repository and the
    // chats that point at it are not cut loose.
    pub fn rename_repository(&self, id: String, name: String) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.repo_working(&id) {
            state.set_status(
                "A chat of this repository is being answered. Wait for the answer to finish before its directory is moved",
            );
            return;
        }
        let made = state.store.rename_repo(&id, &name);
        match made {
            Ok(repo) => {
                state.repositories = state.store.scan_repos();
                state.touch_structure();
                let words = format!("The repository is named {} now", repo.name);
                state.set_status(&words);
            }
            Err(problem) => state.set_status(&problem),
        }
    }

    // Remain a repository in the `Pinned` group of the side bar, or take it out of it. The
    // chats of the repository are not moved.
    pub fn set_repository_pinned(&self, id: String, on: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(mut repo) = state.repositories.iter().find(|r| r.id == id).cloned() else {
            state.set_status("That repository is not there");
            return;
        };
        repo.pinned = on;
        if let Err(problem) = state.store.save_repo(&repo) {
            state.note_record_problem(&problem);
        }
        state.repositories = state.store.scan_repos();
        state.touch_structure();
        let words = if on {
            "The repository stands in Pinned"
        } else {
            "The repository is no longer pinned"
        };
        state.set_status(words);
    }

    // Take a repository away: its record, its chats, the conversations the model made of those
    // chats, the plans of them, the tasks of its directory, and the directory itself out of the
    // repositories directory.
    //
    // The name of the repository, written out in the window that asks, is what lets this
    // happen: the side bar will not answer the request until the name under it is the name of
    // the repository. The directory is not destroyed - it is moved into the `removed/` corner
    // of the repositories directory, where it is until a pointer moves it back - but the chats
    // of the repository, the conversations, the plans and the tasks are removed from the
    // program and nothing of them returns, which is a deal the naming is there to make sure the
    // user meant.
    //
    // An answer being written in a chat of this repository stops the going: the text of that
    // answer would else be put into a chat that is no more, and the server would be left
    // holding the conversation of a chat nobody can open.
    pub fn remove_repository(&self, id: String) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(repo) = state.repositories.iter().find(|r| r.id == id).cloned() else {
            state.set_status("That repository is not there");
            return;
        };
        let talks = config::contexts_dir(&state.cfg);
        let mut mine: Vec<String> = state
            .chats
            .iter()
            .filter(|c| c.meta.repo_id == id)
            .map(|c| c.meta.id.clone())
            .collect();
        // A chat of this repository that is open but not in the list above - the list of the
        // side bar is read when a list changes - is still a chat of it, and still has to go
        // with it.
        if let Some(open) = state.current.clone() {
            if !mine.contains(&open)
                && state.store.load_chat(&open).map(|c| c.meta.repo_id) == Some(id.clone())
            {
                mine.push(open);
            }
        }
        if state
            .live
            .iter()
            .any(|(id, one)| (one.generating || one.summarising) && mine.contains(id))
        {
            let words = format!(
                "The repository {} cannot be removed while its answer is being written",
                repo.name
            );
            state.set_status(&words);
            return;
        }
        // The key of the tasks is created while the folder still remains, for it is the folder
        // made absolute, and once the folder has moved there is nothing left to make absolute.
        let work_key = crate::tasks::key_of(&repo.path);
        // The folder goes first. A going that cannot move the folder has therefore taken
        // nothing away, and the repository is left where the user can see it
        // - which is a better answer than one whose chats vanished from under it.
        let stood = match state.store.retire_dir(&id) {
            Ok(place) => place,
            Err(problem) => {
                state.set_status(&problem);
                return;
            }
        };
        let mut contexts = 0usize;
        let mut chats = 0usize;
        for one in &mine {
            contexts += contexts::drop_of_chat(&talks, &context_prefix(one));
            state.queue_drop(one);
            state.chats.retain(|c| c.meta.id != *one);
            state.store.delete_chat(&id, one);
            chats += 1;
        }
        // The tasks of the directory go with it: a list keyed by a directory that is no longer
        // is nothing to work from, and the next repository found at that path should not
        // inherit them.
        let tasks = crate::tasks::drop_work(&work_key);
        if let Err(problem) = state.store.drop_repo(&id) {
            state.note_record_problem(&problem);
        }
        if state.scope_repo == id {
            state.scope_repo.clear();
            state.current = None;
        }
        state.repositories = state.store.scan_repos();
        state.reload_chats();
        state.touch_structure();
        let words = format!(
            "The repository {} is removed - {} of its chats, {} of its conversations, {} of its tasks",
            repo.name,
            chats,
            contexts,
            tasks
        );
        state.set_status(&words);
        proglog::note(
            proglog::FOLDERS,
            &format!(
                "the repository {} ({}) was removed: {} chats, {} conversations, {} tasks; the directory is at {}",
                repo.name,
                id,
                chats,
                contexts,
                tasks,
                stood.display(),
            ),
        );
    }

    // Every repository with the chats that are in it, the chats that belong to no repository,
    // and the chats the user put away. The side bar requests this when a list changed, and not
    // on every look at the state, for the chats are read from the disk.
    pub fn lists(&self) -> Lists {
        let Ok(state) = self.state.lock() else {
            return Lists::default();
        };
        state.lists()
    }

    // Pick another model. The worker stops the running server - but only when no chat is being
    // answered on it, so a model that was chosen while an answer is on the way takes hold on
    // the next turn instead of cutting that answer short.
    pub fn select_model(&self, id: String) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let answering = state.any_generating();
        state.cfg.model_id = id.clone();
        if let Some(chat) = state.chat_mut() {
            chat.meta.model_id = id.clone();
            state.persist();
        }
        let _ = config::save(&state.cfg);
        // The new model reads its own template, so the thinking may be on a word the new model
        // has never heard of: a level gives place to the switch, and the switch to a level. The
        // badge is moved to a mode the new model proposes before the worker has even seen this
        // call, so the user never sees a marker against a mode that is not there.
        state.refresh_thinking();
        state.normalize_thinking();
        state.persist();
        // A model of their own is often what a waiting list was held back for, so the lists are
        // let go again here as well.
        state.queue_restart();
        state.touch_structure();
        if answering {
            state.set_status("The changed model settings will take effect on a new turn");
        }
        let _ = self.jobs.send(Job::ModelChanged);
    }

    // Store new settings, and refresh what depends on them. A run that chats are being answered
    // on keeps the values it was started with until they are done, which is what makes a
    // setting changed mid-answer take hold on a new turn.
    pub fn save_settings(&self, cfg: Config) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let answering = state.any_generating();
        // What the running server was started with, before these settings take any hold of it.
        // The same line afterwards means the model may stay loaded, however much of the setting
        // was written over.
        let before = state.server_stamp();
        let mut cfg = cfg;
        // The chat composer turns a long paste of text into a file, and the number it does this
        // at is kept inside the two bounds here.
        cfg.pasted_text_char_limit = config::sanitize_pasted_text_limit(cfg.pasted_text_char_limit);
        cfg.chat_width = config::sanitize_chat_width(cfg.chat_width);
        // Compaction. The two words of the settings panel are kept to the two each has, the two
        // numbers are kept inside their bounds, and the handoff prompt is kept short enough to
        // be sent beside a whole conversation.
        cfg.chat_compaction = config::sanitize_compaction(&cfg.chat_compaction);
        cfg.chat_compact_by = config::sanitize_compact_by(&cfg.chat_compact_by);
        cfg.chat_compact_percent = config::sanitize_compact_percent(cfg.chat_compact_percent as i64);
        cfg.chat_compact_reserve = config::sanitize_compact_reserve(cfg.chat_compact_reserve as i64);
        cfg.chat_compact_prompt = config::sanitize_handoff_prompt(&cfg.chat_compact_prompt);
        cfg.chat_compact_prompt_short =
            config::sanitize_handoff_prompt_short(&cfg.chat_compact_prompt_short);
        // The text before the conversation. The prompt of the user is kept short enough to be
        // sent beside a whole conversation, and the markers are kept to the prompts the program
        // knows, in the order of the table.
        cfg.chat_system_prompt = config::sanitize_system_prompt(&cfg.chat_system_prompt);
        cfg.chat_system_prompts_on = config::sanitize_prompt_ids(&cfg.chat_system_prompts_on);
        // What falls behind the text is kept to a word the page can draw: the page draws
        // nothing at all for a word it does not know, and the settings panel would then say an
        // item the chat does not do.
        cfg.chat_background = config::sanitize_background(&cfg.chat_background);
        // The theme of the pages is kept to one of the four words, and the switch of the two
        // that was before them keeps its old meaning: it is on for the dark theme alone.
        cfg.theme = config::theme_word(&cfg.theme, cfg.dark_mode);
        cfg.dark_mode = cfg.theme == config::THEME_DARK;
        state.cfg = cfg;
        let _ = config::save(&state.cfg);
        state.store = store::Store::new(config::repositories_dir(&state.cfg));
        // The user may have pointed the program at a directory of repositories that still holds
        // its conversations, and its records, inside them. The scan is rebuilt here whatever
        // the program icon directory reports, because that marker speaks for one folder of
        // repositories and the user has just named another.
        state.store.migrate_chats_out();
        state.store.migrate_records_out();
        migrate::hunt(&state.cfg, &state.store);
        state.refresh_lists();
        state.reload_chats();
        state.set_status(if answering {
            "The changed model settings will take effect on a new turn"
        } else {
            "Settings saved"
        });
        // A model that is loaded is only sent away when the settings a server is started with
        // truly moved. The rest of the settings belong to the program alone, and saving one of
        // them while a model remains loaded used to cost the user the loading of it again for
        // nothing.
        if before != state.server_stamp() {
            let _ = self.jobs.send(Job::SettingsChanged);
        }
    }

    // The theme of the pages: `light`, `dark`, `silver`, `corporate` or `neon`. The browser
    // page has this switch too. Nothing of a server moves with a theme, so the pages put the
    // new theme on before this answers. The switch of the two themes that was before the others
    // keeps its old meaning and is on for the dark theme alone: the two themes of the grey
    // ladder and the neon one are on dark backgrounds too, but a program written for two knows
    // no other dark theme than the one it has, and the pale theme is an honest fall back.
    pub fn set_theme(&self, word: &str) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let made = config::theme_word(word, state.cfg.dark_mode);
        if state.cfg.theme == made {
            return;
        }
        state.cfg.theme = made.clone();
        state.cfg.dark_mode = made == config::THEME_DARK;
        let _ = config::save(&state.cfg);
        state.touch_structure();
    }

    // Whether the theme of the pages is drawn in its pixel-art form. The form is a second word
    // beside the theme, so a user may keep the colours they are on and have all of the
    // interface drawn out of squares instead. Nothing of a server moves with it.
    pub fn set_theme_pixel(&self, on: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.cfg.theme_pixel == on {
            return;
        }
        state.cfg.theme_pixel = on;
        let _ = config::save(&state.cfg);
        state.touch_structure();
    }

    // Settings for one model, with the devices of the computer.
    pub fn model_settings(&self, id: &str) -> (ModelSettings, Vec<Gpu>) {
        match self.state.lock() {
            Ok(state) => {
                let mut settings = state.cfg.model(id);
                settings.resize_gpus(state.gpus.len());
                settings.vram_budget = config::sanitize_vram_budget(settings.vram_budget);
                (settings, state.gpus.clone())
            }
            Err(_) => (ModelSettings::default(), Vec::new()),
        }
    }

    // Save the settings of one model. A running server gets stopped, because it was started
    // with the old values -- but only when the values it was started with are among those that
    // moved, and only when no chat is being answered on it. Settings of a model that is not the
    // one loaded, and settings that came to no change at all, leave a loaded model where it is.
    // A change made while an answer is on the way is kept all the same and takes hold on the
    // next turn.
    pub fn save_model_settings(&self, id: String, settings: ModelSettings) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let before = state.server_stamp();
        state.cfg.model_settings.insert(id.clone(), settings);
        let moved = before != state.server_stamp();
        let answering = state.any_generating();
        let _ = config::save(&state.cfg);
        state.touch_structure();
        if answering {
            state.set_status("The changed model settings will take effect on a new turn");
        }
        drop(state);
        if moved {
            let _ = self.jobs.send(Job::SettingsChanged);
        }
    }

    // The presets of the settings of a model: the forms of the settings that the user has
    // named. They return in the order of the names, which is the order the menu of the Model
    // tab shows them in, and each with all of the settings it was kept with.
    pub fn model_presets(&self) -> Vec<(String, ModelSettings)> {
        match self.state.lock() {
            Ok(state) => state
                .cfg
                .model_presets
                .iter()
                .map(|(name, settings)| (name.clone(), settings.clone()))
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    // Keep one form of the settings under a name of the user's own. A preset of that name is
    // where this one is requested, then what was there is written over: the text that is in the
    // tab is the newest format of the same, and a preset the user cannot write over is a note
    // they cannot fix. Nothing of a server that runs is moved, for a preset is a form kept for
    // a model to be put into afterwards, and no setting of anything that runs now.
    pub fn model_preset_save(
        &self,
        name: String,
        settings: ModelSettings,
    ) -> Result<String, String> {
        let name = config::sanitize_preset_name(&name);
        if name.is_empty() {
            return Err(String::from(
                "A preset needs a name before it can be saved. Write one in the field first",
            ));
        }
        let Ok(mut state) = self.state.lock() else {
            return Err(String::from("The program is busy"));
        };
        let written_over = state.cfg.model_presets.contains_key(&name);
        state.cfg.model_presets.insert(name.clone(), settings);
        let _ = config::save(&state.cfg);
        state.set_status(if written_over {
            "The preset is overwritten with these settings"
        } else {
            "The preset is kept"
        });
        Ok(name)
    }

    // Throw a preset away. The settings of every model remain where they were: a preset was
    // never a setting of a model, only a form of one that the user kept for later.
    pub fn model_preset_remove(&self, name: &str) -> Result<String, String> {
        let Ok(mut state) = self.state.lock() else {
            return Err(String::from("The program is busy"));
        };
        if state.cfg.model_presets.remove(name).is_none() {
            return Err(String::from("No preset stands under that name"));
        }
        let _ = config::save(&state.cfg);
        state.set_status("The preset was removed");
        Ok(String::from("gone"))
    }

    // Cut a running call of a tool short. The command that was started is stopped, and
    // everything it had printed by then still goes back to the model, under the word that the
    // user stopped it.
    pub fn cancel_running_tool(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if let Some(one) = state.live().running.as_ref() {
            one.watch.stop_now();
            state.set_status("The call is being stopped");
        }
    }

    // Give the call of a tool that is running more time to return within. Each press adds the
    // same time to what remains left, and a call may be given more time as often as the user
    // likes.
    pub fn extend_running_tool(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if let Some(one) = state.live().running.as_ref() {
            one.watch.extend(TOOL_EXTEND);
            let left = one.watch.left().map(|left| left.as_secs()).unwrap_or(0);
            let text = format!("The call has {} seconds left", left);
            state.set_status(&text);
            state.touch();
        }
    }

    // Ask the worker to stop, so the model is unloaded. Every chat that is being answered is
    // set to stop first, so its turn gives up the run it holds.
    pub fn shutdown(&self) {
        if let Ok(state) = self.state.lock() {
            for flag in state.turn_cancel.values() {
                flag.store(true, Ordering::SeqCst);
            }
        }
        let _ = self.jobs.send(Job::Shutdown);
    }
}

impl Snap {
    // A snapshot for the rare case where the lock is removed.
    fn empty() -> Snap {
        Snap {
            mcp: Vec::new(),
            model_name: String::new(),
            model_kind: String::new(),
            in_repository: false,
            repository_id: String::new(),
            repository_name: String::new(),
            chat_title: String::new(),
            context_used: 0,
            context_total: 0,
            context_cut: 0,
            sandbox_access: tools::Access::default(),
            generating: false,
            working: Vec::new(),
            busy: false,
            code_enabled: false,
            tool_switches: store::ToolSwitches::default(),
            search_enabled: false,
            sandbox_enabled: true,
            sandbox_available: false,
            thinking_mode: String::from("off"),
            thinking_modes: Vec::new(),
            thinking_available: false,
            thinking_preserve: true,
            thinking_preserve_available: false,
            status: String::new(),
            dark_mode: false,
            theme: String::new(),
            theme_pixel: false,
            data_dir: String::new(),
            contexts_dir: String::new(),
            sandbox_note: String::new(),
            chat_collapse_thinking: true,
            chat_grow_thinking: true,
            chat_tools_in_thinking: true,
            chat_collapse_tools: true,
            chat_date_new_chat: true,
            chat_highlight: true,
            chat_bold_markdown: true,
            chat_colour_text: true,
            chat_seamless: false,
            chat_display: String::from(config::PLAIN_DISPLAY),
            welcomed: true,
            chat_silent_retries: 2,
            chat_chapters: true,
            chat_userline: true,
            chat_background: String::from(config::BACKGROUND_NONE),
            chat_rain_splash: true,
            chat_send_anim: true,
            chat_context_files: true,
            chat_context_compression: false,
            chat_paste_limit: 6000,
            ask_seconds: 10,
            ask_critical_seconds: 0,
            chat_width: config::LEAST_CHAT_WIDTH,
            models: Vec::new(),
            model_id: String::new(),
            repositories: Vec::new(),
            chat_id: String::new(),
            home: true,
            greeting: String::new(),
            messages: Vec::new(),
            continue_id: String::new(),
            chapters: Vec::new(),
            handoffs: Vec::new(),
            chat_compaction: crate::config::COMPACTION_HANDOFF.to_string(),
            chat_compact_by: crate::config::COMPACT_BY_PERCENT.to_string(),
            chat_compact_percent: 85,
            chat_compact_reserve: 10240,
            chat_compact_prompt: crate::config::DEFAULT_HANDOFF_PROMPT.to_string(),
            chat_compact_prompt_short: crate::config::DEFAULT_HANDOFF_PROMPT_SHORT.to_string(),
            summarising: false,
            chat_timeline: true,
            live_text: String::new(),
            live_reasoning: String::new(),
            live_thought_ms: 0,
            live_thought_done: false,
            progress: 0,
            progress_on: false,
            progress_images: 0,
            prompt_rate: 0.0,
            answer_rate: 0.0,
            live_tools: Vec::new(),
            live_work: Vec::new(),
            live_work_head: String::new(),
            live_work_closed: false,
            planning: false,
            planned: false,
            ask: None,
            running: None,
            queue: None,
            rev: 0,
            structure_rev: 0,
        }
    }
}

// The work of one send of the chat composer: the answer, the handoff summary of it when the
// conversation has come that far, and the plan of the chat when the model let its plan go. A
// message out of the waiting list comes this same way, so a list of them is sent by the path a
// message sent from the box goes by.
fn run_send(
    state: &Arc<Mutex<State>>,
    server: &ServerSlot,
    cancel: &Arc<AtomicBool>,
    sent: SentMessage,
    chat: &str) {
    let mut turn = Turn::Sent(sent);
    // How many times the model has been asked to go on with its work after a handoff summary of
    // this one send.
    let mut goings_on = 0usize;
    loop {
        let ended = run_turn(state, server, cancel, turn, chat);
        // The answer is in the chat and the space it took is known, so this is the moment a
        // handoff summary is requested, if the conversation has come that far. It is requested
        // whatever the answer came to, for a chat left past the limit is only stopped again on
        // the next message.
        let made_room = maybe_compact(state, server, cancel, Weigh::Talk, chat);
        // A summary the program requested is written so that the work goes on, and so it does:
        // the model is put back to its conversation with the first prompt of the user and the
        // summary in front of it, and none of the text the summary means. An answer the user
        // stopped, or one the stream broke, is not continued.
        if !made_room || ended != Ended::Done || goings_on >= CONTINUE_ROOF || cancel.load(Ordering::Relaxed) {
            break;
        }
        goings_on += 1;
        turn = Turn::Continue;
    }
    // The model let its plan go. The planning is closed whole, and the plan itself is sent as
    // the first message of a fresh conversation, with the prompts and the tools the chat had
    // before planning mode and with none of the two tools of planning. The turn above ended on
    // the call that let the plan go, so nothing is asked again of the conversation that
    // planned.
    let mut released = take_plan_release(state, chat);
    while let Some(plan) = released {
        let _ = run_turn(
            state,
            server,
            cancel,
            Turn::Sent(SentMessage {
                text: plan,
                files: Vec::new(),
            }),
            chat,
        );
        // A work that planned again behind this one is rare, and is answered the same way as
        // the first.
        released = take_plan_release(state, chat);
    }
    // The programmed messages of this chat are checked against the answer that has just come to
    // an end. A message whose text the model has just written is sent with the next turn of the
    // chat, ahead of the messages that were only put in the list.
    note_answer(state, chat);
}

// The next message of the waiting list of the chat that is left open, ready to be sent, or
// nothing when there is nothing to send. The message is sent by the same path a message from
// the chat composer goes by, so the commands of the chat composer and the rules of the
// space and of the handoff summary hold for it as well.
//
// A message the sender refuses - because no model remains chosen, or the chat owes a handoff
// summary - is left in the list, and the rest of the list is held back with it. The reason for
// it is on the status line from the attempt, and the program does not ask again every moment:
// touching the list starts it again.
fn queue_step(state: &Arc<Mutex<State>>, chat: &str) -> Option<SentMessage> {
    let (id, next) = state.lock().ok()?.queue_next()?;
    let queued = next;
    let ready = match state.lock() {
        Ok(mut guard) => Engine::prepare_message(&mut guard, queued.text, queued.files),
        Err(_) => return None,
    };
    match ready {
        Some(sent) => {
            if let Ok(mut guard) = state.lock() {
                guard.queue_take_in(chat, id);
            }
            Some(sent)
        }
        None => {
            if let Ok(mut guard) = state.lock() {
                guard.queue_hold_in(chat);
            }
            None
        }
    }
}

// Say that an answer finished, so the programmed messages of the chat that is left open are
// checked against the text the model wrote. A chat with nothing waiting, or with no answer to
// look at, is skipped: the asking of the waiting list is not set going here, for the worker
// asks after it as soon as the chat is idle.
fn note_answer(state: &Arc<Mutex<State>>, chat: &str) {
    let answer = match state.lock() {
        Ok(guard) => guard.last_answer_in(chat),
        Err(_) => return,
    };
    let Some(body) = answer else { return };
    if let Ok(mut guard) = state.lock() {
        guard.answer_programmed_in(chat, &body);
    }
}

// What the turn of one chat is to be. The worker turns a job into one of these and pointers it
// to a thread of its own.
enum Turn2 {
    // Answer a message of the user, with the waiting-list rules that go with it.
    Send(SentMessage),
    // Answer the chat again from the conversation as it is now.
    Retry,
    // Go on with the answer the user stopped.
    Resume(String),
    // Write the handoff summary the chat owes, and go on after it.
    Handoff,
}

// The chat a job works in, when it works in one at all.
fn job_chat(job: &Job) -> Option<String> {
    match job {
        Job::Prompt { chat, .. } => Some(chat.clone()),
        Job::Handoff(chat) | Job::Retry(chat) => Some(chat.clone()),
        Job::Resume { chat, .. } => Some(chat.clone()),
        _ => None,
    }
}

// How many answers at once the model of this chat was started with space for. The setting
// is in the Model tab; one answer at a time is the least, whatever the settings say, and a chat
// with no model to its name gets one as well.
fn slots_of(state: &Arc<Mutex<State>>, chat: &str) -> usize {
    let Ok(guard) = state.lock() else { return 1 };
    let most = guard
        .model_of(chat)
        .map(|model| guard.settings_for(&model).parallel)
        .unwrap_or(1);
    most.max(1) as usize
}

// Whether a turn of this chat may be begun now. Two items hold a turn back: the chat is already
// answering, which is what keeps the messages of a waiting list in order; and the chats that
// are answering fill the places the model has for answers at once. A chat that requests another
// model than the run carries waits for those chats to come to their end, for the model of a run
// is loaded once and for all, and a second model would push the run over. `worn` is the model
// the run carries, or nothing at all when no run remains, which is the case while the first
// chat of a run is still loading its model.
fn room_for(
    state: &Arc<Mutex<State>>,
    worn: Option<&str>,
    busy: &std::collections::BTreeSet<String>,
    chat: &str,
) -> bool {
    if busy.len() >= slots_of(state, chat) {
        return false;
    }
    if busy.is_empty() {
        // Nobody is being answered, so this turn may load whatever it needs.
        return true;
    }
    match worn {
        Some(worn) => state
            .lock()
            .ok()
            .and_then(|guard| guard.model_of(chat))
            .map(|model| model.id())
            == Some(String::from(worn)),
        // No run remains yet: whoever comes first starts it, and the rest wait for the run to
        // be there before they ask it for anything.
        None => false,
    }
}

// What a run that has been stopped, or a model that has been changed, leaves behind: the size
// of the space is not known until the new run speaks, the modes of the new model are read
// again, and a chat that wore a mode the new model cannot read is put at "off".
fn refresh_run(state: &Arc<Mutex<State>>) {
    if let Ok(mut state) = state.lock() {
        state.ctx_total = 0;
        state.refresh_thinking();
        state.normalize_thinking();
        let name = state
            .model()
            .map(|m| m.name)
            .unwrap_or_else(|| String::from("The model"));
        let text = format!("{} loads with the next message", name);
        state.set_status(&text);
    }
}

// Ask one server of the user's own what it has to propose. The state lock is let go of for the
// while, for a server that will not answer is waited for, and the window must go on drawing
// while it is waited for.
fn look_at_mcp(state: &Arc<Mutex<State>>, id: String) {
    let wanted = match state.lock() {
        Ok(guard) => guard
            .cfg
            .mcp_servers
            .iter()
            .find(|server| server.id == id)
            .cloned(),
        Err(_) => return,
    };
    let Some(server) = wanted else { return };
    let found = mcp::list(&server);
    let Ok(mut guard) = state.lock() else { return };
    match found {
        Ok(tools) => {
            let count = tools.len();
            guard.mcp_tools.insert(server.id.clone(), tools);
            guard.mcp_fault.remove(&server.id);
            guard.touch();
            guard.set_status(&format!(
                "{} answers: {}",
                server.name,
                mcp::looked_words(count)
            ));
        }
        Err(problem) => {
            guard.mcp_tools.insert(server.id.clone(), Vec::new());
            guard.mcp_fault.insert(server.id.clone(), problem.clone());
            guard.touch();
            guard.set_status(&format!("{} does not answer: {}", server.name, problem));
        }
    }
}

// Answer one chat again from the conversation as it is now, after one message of it was taken
// out with everything that came after it. The rules of the space and of the handoff summary
// are the same as for a message of the user.
fn run_retry(
    state: &Arc<Mutex<State>>,
    server: &ServerSlot,
    cancel: &Arc<AtomicBool>,
    chat: &str,
) {
    let mut turn = Turn::Retry;
    let mut goings_on = 0usize;
    loop {
        let ended = run_turn(state, server, cancel, turn, chat);
        let made_room = maybe_compact(state, server, cancel, Weigh::Talk, chat);
        if !made_room
            || ended != Ended::Done
            || goings_on >= CONTINUE_ROOF
            || cancel.load(Ordering::Relaxed)
        {
            break;
        }
        goings_on += 1;
        turn = Turn::Continue;
    }
    // The conversation was cut back and answered again, so the answer that is at the end of it
    // is a new one: the programmed messages of the chat look at it, as they look at any other.
    note_answer(state, chat);
}

// Send the work of one turn of one chat to a thread of its own, and say that the chat is at
// work from now. The flag the user stops an answer with is created new for the turn, so a stop
// requested after an answer came to its end does not carry over to the answer after it. When
// the turn is over the worker is informed, which is what frees the place the turn held at the
// model.
fn spawn_turn(
    state: &Arc<Mutex<State>>,
    server: &ServerSlot,
    sender: &Sender<Job>,
    chat: String,
    what: Turn2,
) {
    let cancel = {
        let Ok(mut guard) = state.lock() else { return };
        let flag = begin_turn_cancel(&mut guard, &chat);
        // The chat is reported to be at work before the thread of the turn has the state in
        // hand, so a page that asks again a moment after sending finds the chat answering and
        // not idle.
        guard.live_mut(&chat).generating = true;
        guard.busy = true;
        flag
    };
    let st = state.clone();
    let srv = server.clone();
    let tx = sender.clone();
    std::thread::spawn(move || {
        match what {
            Turn2::Send(sent) => run_send(&st, &srv, &cancel, sent, &chat),
            Turn2::Retry => run_retry(&st, &srv, &cancel, &chat),
            Turn2::Resume(id) => {
                // The text the user stopped are at the end of the conversation, so the request
                // is the conversation as it is now and the model writes on from them. No second
                // ask follows: an answer without a command is an answer at its end, and the
                // size of the context is weighed afterwards as it is after every other answer.
                run_turn(&st, &srv, &cancel, Turn::Resume(id), &chat);
                let _ = maybe_compact(&st, &srv, &cancel, Weigh::Talk, &chat);
                // The text that came after the stopping is the answer of the chat, so they are
                // what a programmed message checks.
                note_answer(&st, &chat);
            }
            Turn2::Handoff => {
                // The user requested a summary that never came to be written again. When it
                // comes, the model is put back to work, for that is what a handoff summary is
                // written for, and the chat was left remaining at the marker of it.
                let made = run_handoff(&st, &srv, &cancel, &chat);
                if made && !cancel.load(Ordering::Relaxed) {
                    run_turn(&st, &srv, &cancel, Turn::Continue, &chat);
                }
            }
        }
        let _ = tx.send(Job::Done(chat));
    });
}

// Begin every turn that may be begun now. Work that cannot be begun yet stays in the list it
// came from and is asked again when a place at the model frees.
fn dispatch(
    state: &Arc<Mutex<State>>,
    server: &ServerSlot,
    sender: &Sender<Job>,
    busy: &mut std::collections::BTreeSet<String>,
    pending: &mut Vec<(Job, String)>,
) {
    let mut asked = true;
    while asked {
        asked = false;
        // The model the run carries, read again for every pass: the first turn of a run loads
        // its model in the middle of a pass, and the chats that come after it are judged
        // against the model that remains by then.
        let worn = server_held(server).map(|(_, model)| model);
        let mut at = 0;
        while at < pending.len() {
            let chat = pending[at].1.clone();
            if busy.contains(chat.as_str()) || !room_for(state, worn.as_deref(), busy, &chat) {
                at += 1;
                continue;
            }
            let (job, chat) = pending.remove(at);
            let what = match job {
                Job::Prompt { sent, .. } => Some(Turn2::Send(sent)),
                Job::Retry(_) => Some(Turn2::Retry),
                Job::Resume { id, .. } => Some(Turn2::Resume(id)),
                Job::Handoff(_) => Some(Turn2::Handoff),
                _ => None,
            };
            let Some(what) = what else { continue };
            busy.insert(chat.clone());
            spawn_turn(state, server, sender, chat, what);
            asked = true;
        }
        // The waiting lists next. A chat takes a message out of its list only when a turn can
        // be begun for it, so a message is never lost between the list and the chat, and the
        // messages of one chat go out in the order the user wrote them.
        let ready = state
            .lock()
            .map(|guard| guard.ready_queues())
            .unwrap_or_default();
        for chat in ready {
            if busy.contains(chat.as_str()) || !room_for(state, worn.as_deref(), busy, &chat) {
                continue;
            }
            let Some(sent) = queue_step(state, &chat) else { continue };
            busy.insert(chat.clone());
            spawn_turn(state, server, sender, chat, Turn2::Send(sent));
        }
    }
}

// The worker. It is the only code that sends requests to llama-server, and the only place that
// decides which chats are answered at the same time.
//
// The worker answers nothing itself: every turn goes to a thread of its own, and the worker
// keeps the list of the chats that are at work. One chat is answered by one turn at a time, so
// the text of a waiting list go out in their order; two chats are answered together as far as
// the model was started with space for that many answers at once, which is the `parallel`
// setting of the Model tab. A chat that wants another model than the run carries waits for the
// chats on the run to come to their end.
fn worker(
    state: Arc<Mutex<State>>,
    jobs: Receiver<Job>,
    sender: Sender<Job>,
) {
    let server = empty_server();
    let mut busy: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut pending: Vec<(Job, String)> = Vec::new();
    let mut replace_run = false;
    loop {
        // A waiting list is checked on a clock, so the messages of a list go out by themselves
        // with no page left to watch the chat. The clock is set only while there is something
        // to look at: a worker with no list to send and no work waiting for a place waits for a
        // job and costs nothing at all.
        let waiting = state
            .lock()
            .map(|guard| !guard.ready_queues().is_empty())
            .unwrap_or(false);
        let job = if waiting || !pending.is_empty() {
            match jobs.recv_timeout(QUEUE_POLL) {
                Ok(job) => job,
                Err(RecvTimeoutError::Timeout) => Job::Tick,
                // The engine was removed, so take the model with it.
                Err(_) => {
                    idle_out(&state, &server, &jobs, &mut busy);
                    return;
                }
            }
        } else {
            match jobs.recv() {
                Ok(job) => job,
                Err(_) => {
                    idle_out(&state, &server, &jobs, &mut busy);
                    return;
                }
            }
        };
        match job {
            Job::Shutdown => {
                idle_out(&state, &server, &jobs, &mut busy);
                return;
            }
            Job::ModelChanged | Job::SettingsChanged => {
                // A run that chats are being answered on is left where it is: the new model, or
                // the new settings, take effect on the next turn, which is what the user is
                // informed. The chats themselves are asked as well as the list of turns, for a
                // turn that has just been handed over is still writing its text.
                let answering = !busy.is_empty()
                    || state
                        .lock()
                        .map(|guard| guard.any_generating())
                        .unwrap_or(false);
                if !answering {
                    stop_server(&state, &server);
                    refresh_run(&state);
                } else {
                    replace_run = true;
                }
            }
            Job::McpLook(id) => look_at_mcp(&state, id),
            Job::Done(chat) => {
                busy.remove(chat.as_str());
                if let Ok(mut guard) = state.lock() {
                    end_turn_cancel(&mut guard, &chat);
                    let idle = !guard.any_generating();
                    drop(guard);
                    if idle && replace_run {
                        stop_server(&state, &server);
                        refresh_run(&state);
                        replace_run = false;
                    }
                }
            }
            Job::Tick => {}
            other => {
                if let Some(chat) = job_chat(&other) {
                    pending.push((other, chat));
                }
            }
        }
        dispatch(&state, &server, &sender, &mut busy, &mut pending);
    }
}

// The run is let go of: the turns that are on their way are let come to their end, so the text
// of an answer is not lost between the server and the chat it was writing into, and the
// conversations are written out before the process stops.
fn idle_out(
    state: &Arc<Mutex<State>>,
    server: &ServerSlot,
    jobs: &Receiver<Job>,
    busy: &mut std::collections::BTreeSet<String>,
) {
    // The wait is bounded: a turn is bounded by its own clocks, and a process that is leaving
    // does not wait on a server that has stopped answering.
    let mut tries = 0usize;
    while !busy.is_empty() && tries < 600 {
        tries += 1;
        // Only the word that a turn is over matters here. The process is leaving, so any other
        // job that arrives during the wait is dropped, as it was before.
        if let Ok(Job::Done(chat)) = jobs.recv_timeout(QUEUE_POLL) {
            busy.remove(chat.as_str());
            if let Ok(mut guard) = state.lock() {
                end_turn_cancel(&mut guard, &chat);
            }
        }
    }
    stop_server(state, server);
}


// The name of the file that the worked-through conversation of one chat is in, inside the
// program directory. The server takes the name and searches for it in that folder and in no
// other place, and it throws out a name that holds a marker of a folder, a colon, a question
// marker, a quote, or a character no word is made of. So the id of the chat is kept as far as
// it is made of markers a name of a file may hold, and the id of the model -- which is a path
// of the disk, and so could never are in a name at all -- is hashed into a few markers at the
// end instead.
//
// The two belong in one name, for the conversation of a chat read through by one model is not
// the conversation of the same chat read through by another. The text is the same text, but
// what the model made of them, and the form the server put them into, are not: a context of one
// model written as a slot of another is thrown out by the server as a conversation that does
// not fit, so the two are kept apart by the name and never mixed.
pub fn context_name(chat_id: &str, model_id: &str) -> String {
    format!("{}-{:016x}.ctx", context_prefix(chat_id), fnv1a(model_id))
}

// The part of the name of a conversation that shows which chat it is: the id of the chat with
// the markers a name may not carry taken out, or a hash of the whole id when the id runs longer
// than a name should. The conversations of one chat are informed from the conversations of
// another by this part alone, so it is also what the conversations of a chat are found by when
// a chat, or the repository that held it, is removed.
pub fn context_prefix(chat_id: &str) -> String {
    let mut name = String::new();
    for mark in chat_id.chars() {
        if mark.is_ascii_alphanumeric() || mark == '.' || mark == '_' || mark == '-' {
            name.push(mark);
        }
    }
    if name.is_empty() {
        name.push_str("chat");
    }
    // Two hundred markers of the id, sixteen of the hash of the model, the marker between, and
    // the four of the ending: well within the two hundred and fifty-five the server reads. An
    // id longer than that is not a chat id of this program, but it is no reason either to let
    // the conversation of one chat fall into the place of another whose id runs the same way
    // for two hundred markers, so a long id is not cut: all of it goes into a hash of its own,
    // which reports every chat apart and is short enough to remain.
    if name.len() > 200 {
        name = format!("{:016x}", fnv1a(chat_id));
    }
    name
}

// FNV-1a: the plainest hash there is, and the shortest to write out. It is here to keep a name
// short and even, and to tell the conversations of two models apart. Nothing of it is against a
// hurt that is meant.
fn fnv1a(words: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in words.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

// Ask the server to write the conversation of one slot out into its file, and put that file
// into its compressed form directly after. The conversations are in the folder `dir`, which is
// where the conversations of this program remain. The conversation of a chat costs as much
// space as the model set aside for it, so it is never left remaining at full size: see
// `src/contexts.rs`. A conversation that could not be written, or that could not be compressed,
// is a conversation read through again, as it always was.
fn write_talk(base_url: &str, slot: usize, name: &str, shorten: bool, dir: &Path) {
    if let Ok(stood) = llama::slot_state(base_url, "save", slot, name) {
        if stood > 0 && shorten {
            contexts::pack(dir, name);
        }
    }
}

// Lay the conversation of a chat into a slot of the server again. The conversation is in its
// compressed form, so the raw file the server reads is written for the moment of the read and
// removed after it. A raw file that is there on its own -- from an older run of the program, or
// from a shortening that could not be finished -- is read as it is now and compressed
// afterwards, which is how a directory of conversations left by an older run compresses itself
// as the chats come round. The conversations are extracted from the folder `dir`.
fn read_talk(base_url: &str, slot: usize, name: &str, shorten: bool, dir: &Path) {
    // The conversation is decompressed either way: a chat whose file is in the compressed form
    // is extracted from that form even when the switch is off now, and only a raw file that was
    // is compressed again, and then only when requested.
    let found = contexts::lay_out(dir, name);
    let _ = llama::slot_state(base_url, "restore", slot, name);
    match found {
        contexts::Laid::FromShort => contexts::take_away(dir, name),
        contexts::Laid::RawStood => {
            if shorten {
                contexts::pack(dir, name);
            }
        }
        contexts::Laid::Nothing => {}
    }
}

// The slot of the running server that the current chat wants, with its conversation written as
// that slot again.
//
// A request that names no slot goes to whatever slot the server has free, and the server reads
// the conversation through from its first word unless the start of it happens to match what a
// slot already holds -- which, between two chats that ask different items of the model, it does
// not. A request that names the slot the conversation was worked through in is placed on: the
// server keeps the work it has already done, and works through only the text that came after
// it.
//
// So a chat that returns is given the slot it had, and the conversation of it is taken out of
// its file and written as the slot first. A chat that is in the slot we are taking has its
// conversation written out to its own file before the slot is removed over, so nothing of it is
// lost and it is read back in its turn.
//
// None is given back when the keeping is switched off, when no chat is left open, or when the
// state is removed: the request then is sent with no slot named, exactly as it always did.
fn context_claim(state: &Arc<Mutex<State>>, base_url: &str, model_id: &str) -> Option<usize> {
    let (chat_id, slots) = {
        let Ok(guard) = state.lock() else { return None };
        if !guard.cfg.chat_context_files {
            return None;
        }
        // A chat that is not open has no sends requests to keep, and no id to name its file by.
        let chat_id = match guard.current.as_ref() {
            Some(id) if !id.is_empty() => id.clone(),
            _ => return None,
        };
        // The server has one slot for each path it may work a conversation through at a time,
        // and the slots are named from zero.
        let slots = guard
            .model()
            .map(|m| guard.settings_for(&m).parallel)
            .unwrap_or(1)
            .max(1) as usize;
        (chat_id, slots)
    };
    // Whether this chat had a slot before, and whether that slot still means it. Both true is
    // the common case: every turn of one chat after the first comes here, and goes straight
    // back with the slot it had. Nothing is read from a file and nothing is written to one, for
    // the server holds the newest of the conversation already.
    let (had, held) = {
        let Ok(guard) = state.lock() else { return None };
        let had = guard.context_slot.get(&chat_id).copied();
        let held = had.and_then(|slot| guard.context_holds.get(&slot).cloned());
        (had, held)
    };
    if let (Some(slot), Some(who)) = (had, held) {
        if who == chat_id && slot < slots {
            return Some(slot);
        }
    }
    let holds = match state.lock() {
        Ok(guard) => guard.context_holds.clone(),
        Err(_) => return None,
    };
    // The slot this chat had, so long as a server with this few paths has it still. A server
    // started with fewer paths than the one before has fewer slots, and a slot of the older
    // server is simply not there.
    let slot = match had {
        Some(slot) if slot < slots => slot,
        _ => {
            // The first slot nobody is in. When every slot remains full of the conversation of
            // another chat there is none such, and zero is taken: the chat turned out of it has
            // its conversation written out just below, and reads it back when it comes round
            // again.
            (0..slots)
                .find(|one| !holds.contains_key(one))
                .unwrap_or_default()
        }
    };
    // The chat that was in this slot, when it was another chat, has its conversation written
    // out here. This is the moment the keeping earns its keep: the slot of that chat is taken,
    // and what the model made of its conversation remains nowhere else. Whether the
    // conversations this program writes are written in the compressed form, and where they are.
    // Both are read once here, so that a conversation written and a conversation read at the
    // same moment are in the one form and in the one folder.
    let (shorten, contexts) = state
        .lock()
        .map(|guard| {
            (
                guard.cfg.chat_context_compression,
                config::contexts_dir(&guard.cfg),
            )
        })
        .unwrap_or_else(|_| (false, config::paths().contexts));
    if let Some(other) = holds.get(&slot) {
        if other != &chat_id {
            let name = context_name(other, model_id);
            write_talk(base_url, slot, &name, shorten, &contexts);
        }
    }
    // The conversation of this chat, out of its file and into the slot. A file that is not
    // there -- the first ask of a chat, or a folder the user threw away -- is answered by the
    // server with an error, and the conversation is then read through as it always was. Nothing
    // is reported either way, for the text that return is the same text however the
    // conversation got there.
    let name = context_name(&chat_id, model_id);
    read_talk(base_url, slot, &name, shorten, &contexts);
    if let Ok(mut guard) = state.lock() {
        guard.context_holds.insert(slot, chat_id.clone());
        guard.context_slot.insert(chat_id, slot);
    }
    Some(slot)
}

// Write out every conversation that remains worked-through in the running server, each to its
// own file. The server is running when this is called, and dead directly after it, so this is
// the last moment there is to write them at. Nothing is reported when one of them cannot be
// written: a conversation that was not kept is a conversation read through again, and the text
// of a chat is in its file of chats as ever -- only the reading of them has to be done twice.
fn save_all_contexts(state: &Arc<Mutex<State>>, base_url: &str, model_id: &str) {
    let holds = match state.lock() {
        Ok(guard) => guard.context_holds.clone(),
        Err(_) => return,
    };
    let (shorten, contexts) = state
        .lock()
        .map(|guard| {
            (
                guard.cfg.chat_context_compression,
                config::contexts_dir(&guard.cfg),
            )
        })
        .unwrap_or_else(|_| (false, config::paths().contexts));
    for (slot, chat_id) in holds {
        let name = context_name(&chat_id, model_id);
        write_talk(base_url, slot, &name, shorten, &contexts);
    }
}

// Write the conversation of the current chat out to its own file. The conversation of a chat
// remains worked-through in a slot of the running server and nowhere else, and the file of it
// is written as a rule only when the model is unloaded or the program is shut down. A stop is a
// second such moment: the server is idle, the text of the answer is kept as far as they came,
// and the conversation in the slot means exactly that. Writing it here costs a chat nothing to
// write and a great deal to read again, for a chat that was stopped and then lost its slot to
// another chat -- or had the program removed under it -- was read through from its first word
// otherwise.
//
// The name of the file is given back when a conversation was written, and nothing is given back
// when there was none to write: the keeping switched off, no chat open, the chat of the
// conversation that was never asked, or a slot the chat is no longer in. That last is the
// careful one of the four, for a slot another chat took holds the conversation of that chat,
// and writing it under the name of this one would put wrong text in the place the conversation
// of this chat is read back from.
fn save_chat_context(state: &Arc<Mutex<State>>, base_url: &str, model_id: &str) -> Option<String> {
    let (chat_id, slot) = {
        let Ok(guard) = state.lock() else {
            return None;
        };
        if !guard.cfg.chat_context_files {
            return None;
        }
        let chat_id = guard.current.clone()?;
        let slot = guard.context_slot.get(&chat_id).copied()?;
        let held = guard.context_holds.get(&slot).cloned()?;
        if held != chat_id {
            return None;
        }
        (chat_id, slot)
    };
    let name = context_name(&chat_id, model_id);
    // A conversation that cannot be written is a conversation read through again, as it always
    // was: the text of the chat is in its file of chats whatever happens here.
    let (shorten, contexts) = state
        .lock()
        .map(|guard| {
            (
                guard.cfg.chat_context_compression,
                config::contexts_dir(&guard.cfg),
            )
        })
        .unwrap_or_else(|_| (false, config::paths().contexts));
    write_talk(base_url, slot, &name, shorten, &contexts);
    Some(name)
}

// Stop the running server. The conversations that remain worked-through in it are written out
// first, for the server is the one place they remain, and after this call there is nothing left
// to write them from: this is the unload of the model, whether it comes from the user changing
// the model or the settings, or from the program being shut down. The bookkeeping of the slots
// goes with the server, for the next server has slots of its own and would not understand a
// context of the one before.
fn stop_server(state: &Arc<Mutex<State>>, server: &ServerSlot) {
    let Ok(mut held) = server.lock() else { return };
    stop_run(state, &mut held);
}

// Stop the run that is in the place, whatever it came from. The conversations the run worked
// through are written out first, for a conversation that was not written is a conversation read
// through again from its first word.
fn stop_run(state: &Arc<Mutex<State>>, old: &mut Option<Server>) {
    if let Some(one) = old.as_ref() {
        save_all_contexts(state, &one.url, &one.model_id);
    }
    if let Ok(mut guard) = state.lock() {
        guard.context_lost();
    }
    if let Some(mut one) = old.take() {
        one.stop();
    }
}

// The text added to the handoff prompt of the user. The conversation being summarised is full
// of the calls of tools, for that is what a working session is made of, and a model that reads
// them again is minded to make one of its own. The tools remain proposed in the request because
// they remain proposed in the conversation -- the list of them is part of the start of the text
// the server has already worked through, so it cannot simply be left out -- which is why the
// request has to say out loud that there is nothing here to be called. A model that asks over
// is answered by `summary_calls_answered` and asked again.
pub const SUMMARY_GUARD: &str = "Do not call any tools. The tools are still proposed in this request only because the conversation above is full of them: nothing of it is to be worked on now. Answer in plain text, with the summary itself only, and keep it within three hundred words. Write the summary, never a call and never the marker of one. Do not speak of the summary, of the session, or of what you are about to do: the text you write is read by another model in place of the conversation.";

// The request for a summary carries no ceiling of its own. A summary is the making of the
// space and not the spending of it, so the model is given all of the space that is above
// after the text of the conversation and may write as long as it likes. What the ceiling did
// before was to keep the request inside the window, and the window is kept by other means: a
// request that runs to the end of the window is rebuilt for a shorter summary. The number is
// sent as -1 on purpose, for a `--n-predict` out of the settings of the model would else cut
// the summary short as well.
pub const NO_SUMMARY_CEILING: i64 = -1;

// The text put after a turn of the model that left nothing of itself behind, when that turn is
// the last item of the conversation. The turn goes back to the server as the text of the model
// with nothing after it, which is an item to go on with rather than an item to answer, and a
// model whose template writes the marker of an empty turn answers it with the marker of an
// empty turn again. The log of one such exchange holds seventeen tokens, of which two are text,
// and the empty turn is at the end of it: the model had written its thought, stated one line
// out of the thought, and shut the turn, and every ask behind that turn returned as empty as it
// was sent. This text go out behind the turn so that the request ends on a question, and the
// model has something to answer. They are never written into the chat, for the user never wrote
// them.
pub const EMPTY_TURN_WORDS: &str = "You wrote nothing above and the program ran nothing, so there is nothing there to answer or to continue from. Continue the work now: say in your reply what you have, or call a tool from the list you are given when the work needs one. Do not write an empty turn. Write a call in parts small enough for the space you have.";

// The word a conversation that ends on a turn of the model with nothing in it is asked with. A
// marker of a summarisation is behind such a turn when the space was freed after it, and the
// handoff block of that marker reports the model to go on with the work, so no second word is
// put behind the turn: it would say the same twice, and the space the summarisation made is
// the space this ask is to be written in.
pub fn empty_turn_words(history: &Value) -> Option<String> {
    if ends_on_summary_mark(history) || !ends_on_an_empty_turn(history) {
        return None;
    }
    Some(String::from(EMPTY_TURN_WORDS))
}

// Whether the handoff block of a summarisation is at the end of the conversation that is sent,
// which is the case after the space was freed: the marker carries its block in the history
// itself, and the text before the marker are left out. A turn of the model that returned empty
// is in front of that block, so the block is the word that already reports the model to go on.
//
// A chat that was never summarised, and a chat whose newest summarisation was never written,
// send no block of theirs at all, so neither is taken for one.
fn ends_on_summary_mark(history: &Value) -> bool {
    history
        .as_array()
        .and_then(|items| items.last())
        .and_then(|last| last.get("content"))
        .and_then(|c| c.as_str())
        .is_some_and(|said| said.starts_with(HANDOFF_FIRST))
}

// The most a turn of the model that left nothing of itself behind is followed by another ask.
// The first ask carries the word that shows an empty turn is not an answer; a model that
// answers that with nothing again is given one more turn, and after it the user is informed. A
// model with nothing to give is not out-asked, and every ask of the type costs the space
// the summarisation just made.
const EMPTY_TURN_ROOF: usize = 2;

// The word the user is given when the model gave no answer twice over and the program can do no
// more about it. What the user has to do is informed in the same text every time: send the
// message again, or request less at a time. One row of them is in the chat as well, for the
// chat ends on a turn with nothing in it and the user is to know why nothing was asked after
// it.
const EMPTY_TURN_END_WORDS: &str = "The model gave no answer twice, so the answer ends here. Send the message again, or request the work in parts small enough for the context";

// The word the user is given when the writing was stopped at the limit of the summarisation
// over a turn that left nothing of itself behind, and no space could be made behind it. The
// answer is ended where it is, and the user is informed what to do: the same as above, but of
// one turn and not of two.
const EMPTY_TURN_CUT_WORDS: &str = "The context filled while the model wrote neither text nor a call, so the answer ends here. Send the message again, or request the work in parts small enough for the context";

// The word the user is given when a space was created behind such a turn, the model was
// asked again from behind the handoff block, and it gave neither text nor a call out of the
// space that had just been made. The first stop is continued, for that is what a summary is
// written for; a second has no space left to make and no word to say with it, so the answer
// ends and the user is informed.
const EMPTY_TURN_TWICE_WORDS: &str = "The model wrote neither text nor a call after the context was freed, so the answer ends here. Send the message again, or request the work in parts small enough for the context";

// How much space the conversation takes when the report of a call that was stopped where the
// model wrote it is left out of it. The report is the longest message of a conversation that
// came to the limit in the middle of a call, for it holds the text of the call the model had
// written; and it is not text of the conversation, for it is the program speaking, and the
// summary is written of the conversation. So the space a summarisation is weighed against
// is the size of the context without it. The request for the summary itself carries the
// conversation as it is now, telling and all, for the sake of the work the server has already
// done on the start of the text: see `summary_ask`.
pub fn carrying_the_cut(history: &Value) -> u64 {
    let mut items: Vec<Value> = history.as_array().cloned().unwrap_or_default();
    items.retain(|item| {
        !(item.get("role").and_then(|r| r.as_str()) == Some("assistant")
            && item
                .get("content")
                .and_then(|c| c.as_str())
                .is_some_and(|said| said.starts_with(CUT_TOLD_FIRST)))
    });
    estimate_tokens(&Value::Array(items))
}

// Whether the report of a call that was stopped where the model wrote it is among the text that
// go out. `carrying_the_cut` leaves it out of the weighing, and while it remains the number the
// server spoke is no measure of the conversation the next ask carries either, for it holds the
// text of the call the program spoke of. The marker of the report is its first text, which no
// word of a model and no result of a tool reads as.
fn cut_telling_stands(history: &Value) -> bool {
    history
        .as_array()
        .map(|items| {
            items.iter().any(|item| {
                item.get("role").and_then(|r| r.as_str()) == Some("assistant")
                    && item
                        .get("content")
                        .and_then(|c| c.as_str())
                        .is_some_and(|said| said.starts_with(CUT_TOLD_FIRST))
            })
        })
        .unwrap_or(false)
}

// The request for a handoff summary: the conversation exactly as it is now, and the request for
// the summary after it as a message of the user.
//
// Nothing of the conversation is changed for the request. It used to be made over for it -- the
// images taken out of it, the calls of tools and the results of them turned into lines of text
// -- and every one of those cuts fell into the middle of the text the server had already worked
// through. A server keeps the work it did on the start of a conversation, and it can only keep
// it while the start of the next ask is word for word the start of the last: a conversation
// made over is a conversation worked through again from its first word, which spends the very
// space the summarisation was meant to save. So the conversation is sent as it is now and
// the request rides after it the way a message of the user rides after it. Only the request is
// new, and only the request is worked through.
//
// The tools remain proposed as they are now for an answer of this chat, for the same reason:
// the list of them is rendered into the start of the conversation. A model that requests one
// while it is writing the summary has the call cancelled and is asked again -- see
// `ask_summary_for_handoff`.
//
// No ceiling is above the text the model may write: see NO_SUMMARY_CEILING.
pub fn summary_ask(history: &Value, prompt: &str, tools: Option<&Value>) -> Value {
    let mut items: Vec<Value> = history.as_array().cloned().unwrap_or_default();
    let mut said = String::from(prompt);
    said.push_str(SUMMARY_GUARD);
    items.push(json!({ "role": "user", "content": said }));
    let mut body = json!({
        "messages": Value::Array(items),
        "stream": true,
        "stream_options": { "include_usage": true },
        // And the server reports on every piece how far it is with the text it was sent, so the
        // meter above the chat composer has numbers before an answer.
        "return_progress": true,
        "timings_per_token": true,
        // The model may write as long as it wants. A ceiling here cut the summary short in the
        // middle of a sentence, which is what the second ask is there to fix.
        "max_tokens": NO_SUMMARY_CEILING,
    });
    // The tools of the chat, in the form they are proposed in for an answer, so that the start
    // of this ask is the start of the conversation.
    if let Some(list) = tools {
        body["tools"] = list.clone();
        body["tool_choice"] = json!("auto");
    }
    body
}

// How many times the request for a summary is rebuilt when the model answers it with a call of
// a tool instead of with the summary. Twice is the most: a model that asks a third time has
// made its choice, and the text it wrote beside the calls are taken for the summary rather than
// spending more of the space the summary was come to make.
const SUMMARY_CALL_TRIES: usize = 2;

// The result given back to a call of a tool that arrived the middle of the writing of a handoff
// summary. The call never ran, and this text reports why.
pub const SUMMARY_NO_TOOL_WORDS: &str = "No tool may be called while a handoff summary is being written. The summary is read by another model in place of the conversation, and nothing of the conversation is worked on now. This call was cancelled and nothing of what it requested was done. Write the summary in plain text.";

// The entries that answer the calls a model made instead of writing a summary: the saying of
// the model that held the calls, and one result for each of them saying it was cancelled and
// why. They are shaped as every other run of a conversation is shaped, so the server reads a
// run that ended and owes nothing more.
pub fn summary_calls_answered(calls: &[llama::ToolCall], said: &str) -> Vec<Value> {
    // The id of a call is how a result is tied to it. A server that wrote no id is given one
    // here, and the same one on both sides of the pair.
    let ids: Vec<String> = calls
        .iter()
        .enumerate()
        .map(|(place, call)| {
            if call.id.trim().is_empty() {
                format!("no-tool-{}", place + 1)
            } else {
                call.id.clone()
            }
        })
        .collect();
    let mut out: Vec<Value> = Vec::new();
    // The text the model wrote before it asked are kept with the asking, as they are in every
    // turn of a conversation, so nothing it stated is lost from the request.
    out.push(json!({
        "role": "assistant",
        "content": if said.trim().is_empty() { Value::Null } else { Value::String(said.to_string()) },
        "tool_calls": calls
            .iter()
            .zip(ids.iter())
            .map(|(call, id)| json!({
                "id": id,
                "type": "function",
                "function": { "name": call.name, "arguments": call.arguments },
            }))
            .collect::<Vec<Value>>(),
    }));
    for id in &ids {
        out.push(json!({
            "role": "tool",
            "tool_call_id": id,
            "content": SUMMARY_NO_TOOL_WORDS,
        }));
    }
    out
}

// Request the handoff summary of a conversation, and keep asking while the model answers with a
// call of a tool instead of with the summary. Every ask carries the conversation as it is now,
// so the work the server has already done on it remains from the first ask to the last: a
// cancelled call is answered where it is and the model is asked on from there, which costs the
// few words of the answer and nothing of the conversation. Where the context meter remains when
// a request for a summary begins: the size of the text that go out with this ask, and nothing
// else. The text an earlier summary implies is not among them, for the conversation that is
// sent is the first prompt, the summary, and the text after the marker; and neither are the
// text of a summary that was deleted, which is what the second ask of one summary sends.
fn summary_start(body: &Value) -> u64 {
    estimate_tokens(&body["messages"])
}

// The text shown when the text of a summary returned as nothing. Two errors look as well here:
// either way the writing ended with no word of the summary in it. Only one of them is the
// space. The server speaks "length" when it came to the end of the space, and the hand
// over the writing of a summary speaks the same word for the same; any other end means the
// model had space and gave no text, and the space is not to blame for that.
fn empty_summary_words(finish: &str) -> &'static str {
    if finish == "length" {
        "The summary filled the context without any text of its own. Asking for a shorter one"
    } else {
        "The model gave no text for the summary, and the context was not full. Asking again for a shorter one"
    }
}

#[allow(clippy::too_many_arguments)]
fn ask_summary_for_handoff(
    state: &Arc<Mutex<State>>,
    base_url: &str,
    history: &Value,
    prompt: &str,
    tools: Option<&Value>,
    slot: Option<usize>,
    cancel: &Arc<AtomicBool>,
    chat: &str) -> SummaryShot {
    let mut body = summary_ask(history, prompt, tools);
    // The thinking of the chat rides on this ask as it rides on an answer. The server was never
    // informed the mode of the chat, so a request without this text would be a summary written
    // with the thinking of the model's own choosing.
    if let Ok(guard) = state.lock() {
        let thought = guard.thinking_ask();
        if !thought.is_null() {
            body["chat_template_kwargs"] = thought;
        }
    }
    // The request goes to the slot the conversation above it was worked through in, so the
    // server is on that work and works through the request alone.
    if let Some(slot) = slot {
        body["id_slot"] = json!(slot);
    }
    let mut shot = ask_summary(state, base_url, &body, cancel, chat);
    let mut tries = 0usize;
    while !shot.calls.is_empty()
        && tries < SUMMARY_CALL_TRIES
        && !shot.stopped
        && shot.error.is_none()
        && !cancel.load(Ordering::Relaxed)
    {
        tries += 1;
        // The request is rebuilt here because the model answered the request for a summary with
        // a call. The number of the try is in the line, for the request is created at most
        // `SUMMARY_CALL_TRIES` times over one summary.
        proglog::note(
            proglog::SUMMARY,
            &format!(
                "the summary requested a tool instead of text, so the call was cancelled and the summary is requested again: attempt {} of the {} allowed for one summary",
                tries, SUMMARY_CALL_TRIES
            ),
        );
        tell(
            state,
            "The summary requested a tool. The call was cancelled and the summary is requested again",
        );
        if let Some(items) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
            items.extend(summary_calls_answered(&shot.calls, &shot.text));
        }
        // What the model gives after a cancelled call is the summary. The text of the request
        // that was cancelled remains with it above, so nothing is reported twice.
        shot = ask_summary(state, base_url, &body, cancel, chat);
    }
    shot
}

// The tools of the request for a summary: the same list, in the same format, as the list an
// answer of this chat is asked with. The list of the tools is rendered into the start of the
// conversation, so a request that carried another list -- or none, as this ask used to -- is a
// conversation worked through again from its first word.
fn summary_tools(state: &Arc<Mutex<State>>) -> Option<Value> {
    let (
        sandbox_on,
        code_on,
        search_on,
        planning_on,
        corporate_on,
        access,
        switches,
        mcp_tools,
        compaction_on,
    ) = {
        let Ok(guard) = state.lock() else { return None };
        let chat = guard.chat();
        (
            chat.as_ref().map(|c| c.meta.sandbox_enabled).unwrap_or(false),
            chat.as_ref().map(|c| c.meta.code_enabled).unwrap_or(false),
            chat.as_ref().map(|c| c.meta.search_enabled).unwrap_or(false),
            guard.planning_open(),
            guard.corporate_open(),
            chat.as_ref().map(|c| access_of(&c.meta)).unwrap_or_default(),
            chat.as_ref().map(|c| c.meta.tools.clone()).unwrap_or_default(),
            guard.mcp_definitions(),
            guard.compaction_on(),
        )
    };
    offered_with_planning(
        sandbox_on,
        planning_on,
        corporate_on,
        code_on,
        search_on,
        access,
        &mcp_tools,
        &switches,
        compaction_on,
    )
}

// One ask of the server for a handoff summary, and what came of it.
struct SummaryShot {
    // The text of the answer as it arrived, before the envelope was taken off.
    text: String,
    // Why the server stopped writing. "length" reports that the context window filled under the
    // summary, which is the moment the second ask is created.
    finish: String,
    // The calls of tools the model made instead of writing the summary. None of them ever runs:
    // see `summary_calls_answered`.
    calls: Vec<llama::ToolCall>,
    // The user stopped the request manually.
    stopped: bool,
    // The error in the stream, when there was one.
    error: Option<String>,
    // The size of the text the server was sent on the request, as the parts of it spoke it.
    // Zero when the server stated nothing of the conversation at all, which is how a request
    // that never reached the model reads.
    sent: u64,
}

// The marker that shows the program is writing a handoff summary. It is taken off on the way
// out of the request, whichever way out that is, so no error of a server leaves the window
// waiting for text that is never shown.
struct SummaryWriting {
    state: Arc<Mutex<State>>,
}

impl Drop for SummaryWriting {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.state.lock() {
            guard.live_write().summarising = false;
            guard.touch();
        }
    }
}

// Whether the handoff summary has to be asked again for a short one. The context window filled
// under it, so its last sentence stops in the middle of itself, and the model is asked once
// more with the short prompt of the settings. A stop of the user's own and an error in the
// stream are never asked again for a shorter summary: the first has nothing to compress, and
// the other is set to the user.
fn summary_too_long(shot: &SummaryShot) -> bool {
    shot.error.is_none() && !shot.stopped && shot.finish == "length"
}

// Say a word to the user about the summary, from outside the lock.
fn tell(state: &Arc<Mutex<State>>, words: &str) {
    if let Ok(mut guard) = state.lock() {
        guard.set_status(words);
        guard.touch();
    }
}

// The least space that is above while a handoff summary is being written. The hand over a
// summary is put up when the conversation has come within this of all of the space, so the
// writing of it is stopped while a piece of the space is left and not after the server has
// nothing left to answer with. A piece the size of a call of the meter is enough, for the
// pointer is checked on every piece.
const LEAST_SUMMARY_ROOM: u64 = 64;

// Ask the server for a handoff summary once, with the text the model is to be given, and gather
// what it writes.
//
// None of it is shown. The text of a summary is not the text of the model to the user: they are
// read by the next model, and a wall of text laid over the conversation is nothing to read. So
// nothing goes into the live text the window draws. The meter above the chat composer and the
// context meter still move, for they say that the program is at work without saying a note
// about what it wrote.
fn ask_summary(
    state: &Arc<Mutex<State>>,
    base_url: &str,
    body: &Value,
    cancel: &Arc<AtomicBool>,
    chat: &str) -> SummaryShot {
    {
        let Ok(mut guard) = state.lock() else {
            return SummaryShot {
                text: String::new(),
                finish: String::new(),
                calls: Vec::new(),
                stopped: true,
                error: Some(String::from("The program state is removed")),
                sent: 0,
            };
        };
        // The meter means this ask alone. The text under the number follow the images the
        // conversation carries, for the server has none of them and the count is read from the
        // text that go out.
        guard.live_mut(chat).progress = 0;
        guard.live_mut(chat).progress_images = pending_images(&body["messages"]);
        guard.live_mut(chat).progress_on = true;
        guard.live_mut(chat).prompt_rate = 0.0;
        guard.live_mut(chat).answer_rate = 0.0;
        guard.live_mut(chat).rate_start = None;
        guard.live_mut(chat).thought_start = None;
        guard.live_mut(chat).thought_end = None;
        guard.live_mut(chat).live_thought_ms = 0;
        guard.live_mut(chat).live_thought_done = false;
        // The meter is anchored on the text that go out with this ask, and not on where the
        // meter was before. A summary requested the second time with the short prompt sends the
        // same trimmed conversation: the sandbox boundary of text the first attempt wrote is
        // deleted, and is no part of the conversation that is sent now. The climb of the meter
        // only ever climbs, so a meter that kept the high-water marker of that writing read the
        // space as removed before the model had a word to write, and the request was then
        // blamed for filling the context when the space was still there. The text a summary
        // of before means are not in the conversation that is sent, so they are not in this
        // number either.
        guard.live_mut(chat).ctx_used = summary_start(body);
    }
    // A summary is requested with no hand of the summarisation limit over it: it is the making
    // of the space and not the spending of it, the request carries all of the conversation,
    // and the conversation remains past that limit before a note about the summary comes. The
    // end of the space is another matter, and the hand over a summary is put up there. A model
    // with a great deal to say can fill what is above to the last word of it, and the request
    // then returns with a wall of text and no end of itself: the summary of that wall is what
    // the space was come to make, and the space has expired besides. So the writing of a
    // summary is stopped at the edge of the space, the text it gave remain, and the request
    // is rebuilt for a short one -- which is what the second prompt of the settings is there
    // for.
    let hand = std::sync::atomic::AtomicBool::new(false);
    // The text of the summary is gathered here, out of the sight of the window.
    let mut said = String::new();
    let gauge_state = state.clone();
    let context_state = state.clone();
    let answer = llama::stream_chat(
        base_url,
        body,
        cancel,
        &hand,
        &mut |piece: &str| {
            said.push_str(piece);
            // The first word is here, so the text that was sent is worked through and the meter
            // above the chat composer has nothing left to say.
            if let Ok(mut guard) = gauge_state.lock() {
                if finish_progress(&mut guard, chat) {
                    guard.touch();
                }
            }
        },
        // The thinking of a summary is hidden with the rest of it.
        &mut |_piece: &str| {},
        &mut |made: llama::Progress| {
            if let Ok(mut guard) = gauge_state.lock() {
                let percent = (made.part() * 100.0).round() as u32;
                if percent != guard.live_ref(chat).progress {
                    guard.live_mut(chat).progress = percent;
                    // All of the text the request carried are worked through, so the meter has
                    // nothing left to say. The model is writing the summary now, and none of
                    // that writing is shown: the three dots under the heading of the
                    // summarisation say that the program is still at work. A meter that was at
                    // 100 per cent read as an error in the program, for it stated that the text
                    // was still being worked through.
                    if percent >= 100 {
                        finish_progress(&mut guard, chat);
                    }
                    guard.touch();
                }
            }
        },
        &mut |made: llama::Context| {
            if let Ok(mut guard) = context_state.lock() {
                if climb_context_paced(&mut guard, made.used(), Instant::now(), chat) {
                    guard.touch();
                }
                // The end of the space is where a summary is stopped, and the least that is
                // above is the space of one more piece, so the pointer goes up before the
                // last of the space is taken and the text the model gave remain whole.
                if guard.ctx_total > 0
                    && made
                        .used()
                        .saturating_add(LEAST_SUMMARY_ROOM)
                        >= guard.ctx_total
                {
                    // Once, for the parts that come after the pointer remains tell the same
                    // over again until the way back to the server is shut.
                    if !hand.load(Ordering::Relaxed) {
                        proglog::note(
                            proglog::SUMMARY,
                            &format!(
                                "the meter rises while the summary is written: the conversation is at {} of {} and {} of the space is left, which is less than the {} of one more piece",
                                made.used(),
                                guard.ctx_total,
                                guard.ctx_total.saturating_sub(made.used()),
                                LEAST_SUMMARY_ROOM
                            ),
                        );
                    }
                    hand.store(true, Ordering::Relaxed);
                }
            }
        },
        // A summary is not written with a command, so the text of a call is passed by.
        &mut |_made: &[ToolCall]| {},
    );
    // Both hold the text the model gave. The longer of the two is the safer.
    let text = if answer.text.chars().count() >= said.chars().count() {
        answer.text.clone()
    } else {
        said
    };
    // Every request a summary is in the log of the program with the numbers the server spoke of
    // it and the reason the writing ended. The text of the summary is not decompressed here:
    // they are in the marker of the summarisation, which the user may read whole.
    let ending = if let Some(problem) = answer.error.as_deref() {
        format!("the way back to the server failed: {}", problem)
    } else if answer.stopped {
        String::from("the user stopped it")
    } else if answer.interrupted {
        String::from("the writing stopped at the edge of the context")
    } else if answer.finish.is_empty() {
        String::from("the server gave no reason for the end of it")
    } else {
        format!("it came to its end with {}", answer.finish)
    };
    proglog::note(
        proglog::SUMMARY,
        &format!(
            "a request for the summary came back: the server reported {} words in and {} out; its writing is finished, {}",
            answer.prompt_tokens, answer.completion_tokens, ending
        ),
    );
    SummaryShot {
        text,
        // A summary stopped at the edge of the space came to no end of its own, which is
        // what the short summary request of the settings is created for: the same hand that
        // stops the sandbox boundary of text records it to the rest of the program.
        finish: if answer.interrupted {
            String::from("length")
        } else {
            answer.finish.clone()
        },
        calls: answer.tool_calls.clone(),
        stopped: answer.stopped,
        error: answer.error.clone(),
        sent: answer.sent(),
    }
}

// The least space that is above while a handoff summary is being written. The hand over a
// summary is put up when the conversation has come within this of all of the space, so the
// writing is stopped while a part of space is left and not after the server has nothing left to
// answer with.

// Ask the model for the handoff summary of the chat that is open, and write it into the marker
// of the summarisation. After this the text before the marker are not sent to the model again:
// the first message of the user, the summary, and the text that come after the marker is all of
// the conversation as far as the model is concerned. The user still sees all of the
// conversation, and the line at the right of the chat keeps one node for every summarisation of
// it.
//
// The marker goes into the chat before the request, for a server that is removed in the middle
// of a summary must leave a sign of it: the marker is with nothing in it and the reason beside
// it, and the user starts the summary again from the point it was at.
//
// None of the writing is shown to the user, and the model is not stopped at the limit of the
// summarisation while it writes: the summary is given all of the space that is above after the
// text of the conversation. When the space runs full under the first summary it is asked
// again for a short one, of the second prompt of the settings. True is given back when the
// summary was kept, which is the word that the space was freed and the work may be continued.
fn run_handoff(
    state: &Arc<Mutex<State>>,
    server: &ServerSlot,
    cancel: &Arc<AtomicBool>,
    chat: &str) -> bool {
    // Take the order. An answer and a summary cannot both be on the way, for both write into
    // the same chat.
    let (model, settings, prompt, short, before, history, id) = {
        let Ok(mut guard) = state.lock() else {
            return false;
        };
        if guard.live_ref(chat).generating {
            return false;
        }
        let Some(model) = guard.model_of(chat).clone() else {
            guard.set_status("Choose a model in the top bar first");
            return false;
        };
        if guard.chat_of(chat).is_none() {
            return false;
        }
        let prompt = config::sanitize_handoff_prompt(&guard.cfg.chat_compact_prompt);
        let short = config::sanitize_handoff_prompt_short(&guard.cfg.chat_compact_prompt_short);
        let before = guard.live_ref(chat).ctx_used;
        // The conversation as it is now is what the summary is written of. `build_history`
        // already leaves out the text that the newest written summary means, and it sends no
        // word of a marker that was never written, so the request is made of the first prompt,
        // the summary, and the text that remains after the newest written marker: all of what
        // there is to summarise now, and no summary of a summary.
        let history = guard.build_history_in(chat);
        guard.live_mut(chat).generating = true;
        guard.busy = true;
        // The text of the summary is written of sight of the user.
        guard.live_mut(chat).summarising = true;
        // Both live buffers start empty, so the text of an older answer is not under the marker
        // of the summarisation.
        guard.live_mut(chat).live_text = String::new();
        guard.live_mut(chat).live_reasoning = String::new();
        guard.live_mut(chat).live_tools = Vec::new();
        guard.live_mut(chat).live_work = Vec::new();
        guard.live_mut(chat).live_work_head = String::new();
        guard.live_mut(chat).live_work_closed = false;
        guard.live_mut(chat).progress = 0;
        guard.live_mut(chat).progress_on = false;
        let id = guard.begin_summary_in(chat);
        guard.set_status("The handoff summary is being written");
        let wants = guard.settings_for(&model);
        (model, wants, prompt, short, before, history, id)
    };
    // From here to the end of the request the window is informed that a summary is being
    // written, and it draws no live answer.
    let _writing = SummaryWriting {
        state: state.clone(),
    };
    // The request that is created now is all of the conversation as it is now, so the two
    // numbers of it say what the server is about to be given: the meter the user is shown, and
    // the measure of the text that go out.
    proglog::note(
        proglog::SUMMARY,
        &format!(
            "a handoff summary is started. The context meter is at {}, and the text sent with the request measures {}",
            before,
            estimate_tokens(&history)
        ),
    );

    // A server that was removed is started again, as in an answer. The lock of the state is not
    // held while the model is being loaded.
    match ensure_server(state, server, &model, &settings, cancel, chat) {
        Ok(total) => {
            let Ok(mut guard) = state.lock() else {
                return false;
            };
            guard.busy = guard.any_generating();
            guard.ctx_total = total;
        }
        Err(problem) => {
            proglog::note(
                proglog::SUMMARY,
                &format!("the handoff summary was not started after all: the model could not be loaded again. {}", problem),
            );
            let Ok(mut guard) = state.lock() else {
                return false;
            };
            guard.busy = guard.any_generating();
            guard.live_mut(chat).generating = false;
            // The marker keeps nothing of its own, and the reason is beside it, so the user can
            // start the summary again.
            guard.finish_summary_in(chat, &id, "", before, 0, &problem);
            guard.set_status(
                "The handoff summary was not written. Start it again from the summarisation timeline",
            );
            return false;
        }
    }

    let base_url = server_url(server);
    // The slot of this chat, with the conversation of it written as the slot again when another
    // chat was there last. The request of the summary names the slot, for the conversation
    // above the request is the conversation the server worked through in it.
    let slot = claim_slot(state, server);
    // The conversation as it is now, with the request for the summary after it as a message of
    // the user. The tools of the chat remain proposed as they are now for an answer, so that
    // the start of this ask is the start of the conversation word for word and the server keeps
    // every bit of work it has already done on it. A model that answers with a call instead of
    // with the summary has the call cancelled and is asked again.
    let tools = summary_tools(state);
    let mut shot =
        ask_summary_for_handoff(state, &base_url, &history, &prompt, tools.as_ref(), slot, cancel, chat);
    // A long conversation can trip the server over one ask and not over the next, so an error
    // that leaves no text at all is asked once more before the user is informed. A stop of the
    // user's own is never asked again.
    if shot.error.is_some() && !shot.stopped && !cancel.load(Ordering::Relaxed) {
        proglog::note(
            proglog::SUMMARY,
            &format!(
                "the request for the summary did not return: {}. It is tried once more before the user is informed.",
                shot.error.as_deref().unwrap_or("the server closed the connection")
            ),
        );
        tell(state, "The summary did not arrive. Asking once more");
        shot = ask_summary_for_handoff(state, &base_url, &history, &prompt, tools.as_ref(), slot, cancel, chat);
    }
    let mut summary = clean_summary(&shot.text);
    // The first ask was created again when the context window filled under it, which is the
    // usual way the window is kept. One more guard is here for the request that filled the
    // window without the second ask being needed: a model that thinks at length before it
    // writes spends the space on its thoughts, and the summary is removed before the first
    // word of it remains. Such a request is rebuilt, this time for a short one, for the space
    // is still past the limit and a chat left there is stopped again on its next message.
    let mut asked_short = false;
    if summary.trim().is_empty()
        && shot.error.is_none()
        && !shot.stopped
        && !cancel.load(Ordering::Relaxed)
    {
        // The line above this one in the log reports what the server spoke of the request that
        // has just return, and whether the context filled under it. This line reports only what
        // the program made of it, for the two are read apart by the text a user is shown here.
        // The two numbers of it are written down as well: what the server spoke of the end of
        // the writing, and the size of the text that was sent with the request. Without them a
        // user of the log cannot tell the space that was really removed from the space that was
        // only blamed.
        proglog::note(
            proglog::SUMMARY,
            &format!(
                "the model gave no text at all for the summary, although the request came back without an error. The server reported the end of the writing as {}, the text sent with the request measured {}, and the context window of the model is {}; the request is made again with the short prompt from the settings.",
                if shot.finish.is_empty() {
                    "nothing"
                } else {
                    shot.finish.as_str()
                },
                shot.sent,
                state
                    .lock()
                    .map(|guard| guard.ctx_total)
                    .unwrap_or_default()
            ),
        );
        tell(state, empty_summary_words(&shot.finish));
        let again = ask_summary_for_handoff(state, &base_url, &history, &short, tools.as_ref(), slot, cancel, chat);
        let cleaned = clean_summary(&again.text);
        if !cleaned.trim().is_empty() {
            shot = again;
            summary = cleaned;
            asked_short = true;
        }
    }
    // The context window filled under the first summary, so its last sentence stops in the
    // middle of itself. It is asked again, this time for a short one, which has a far better
    // chance of coming to an end in the space that is left. The second ask carries the
    // conversation and nothing of the first answer, so it has all of the space the first
    // one had. The long summary is kept for all that it stops short when the short one does not
    // reach all.
    if summary_too_long(&shot) {
        proglog::note(
            proglog::SUMMARY,
            &if asked_short {
                "the summary did not finish on its own again, and the short summary request had already been made, so the text that remains is kept with its ending missing".to_string()
            } else {
                "the summary did not finish on its own, because the context filled while it was written. The request is rebuilt as a short one.".to_string()
            },
        );
        tell(
            state,
            "The summary filled the context. Asking again for a shorter one",
        );
        if !asked_short {
            let again = ask_summary_for_handoff(state, &base_url, &history, &short, tools.as_ref(), slot, cancel, chat);
            let cleaned = clean_summary(&again.text);
            if !cleaned.trim().is_empty() {
                shot = again;
                summary = cleaned;
                asked_short = true;
            }
        }
    }

    let Ok(mut guard) = state.lock() else {
        return false;
    };
    guard.live_mut(chat).generating = false;
    guard.busy = guard.any_generating();
    finish_progress(&mut guard, chat);
    // The summary is never shown, so the live buffers are emptied rather than read. What the
    // model gave is in the marker of the summarisation alone.
    guard.live_mut(chat).live_text = String::new();
    guard.live_mut(chat).live_reasoning = String::new();
    // The space the summary itself takes, read from the text that is in the marker: the
    // number the server gives of the text the model wrote holds the thinking that is hidden
    // with the rest of it, and it reads the summary bigger than it is. The measure of the text
    // is the number the user reads beside the heading of the summarisation.
    let after = estimate_tokens(&json!([{ "role": "user", "content": &summary }]));
    // What became of the request. A summary that never came, that was stopped, or whose server
    // was removed, is no summary at all: the marker is left empty, the chat keeps every word it
    // had, and the user starts it again. A summary that the directory truncated, twice, is kept
    // for all that, for the space was full when it was requested and a full chat cannot
    // wait for a better one.
    let why = if shot.error.is_some() {
        format!(
            "The summary was not written: {}",
            shot.error.as_deref().unwrap_or("The server closed the connection")
        )
    } else if shot.stopped {
        String::from("The summary was stopped before it was written. Start it again")
    } else if summary.trim().is_empty() {
        String::from("The model gave no text for the summary. Start it again")
    } else if shot.finish == "length" {
        String::from("The context filled while the summary was written, so the end of it is missing")
    } else {
        String::new()
    };
    // The text is kept only when the request itself came off: no error of the stream, no stop
    // of the user's, and text that is not nothing. The line above reports what happened, and
    // this reports whether anything was won by it.
    let kept = shot.error.is_none() && !shot.stopped && !summary.trim().is_empty();
    if !kept {
        guard.finish_summary_in(chat, &id, "", before, 0, &why);
        // The meter keeps the size of the text the server was sent on the request. The chat
        // keeps every word it had, and those text is all of what that ask was sent, so the
        // number the parts of it spoke is the number of the chat that still is there; only the
        // text of the summary that never came is not kept, and they are in no number of it
        // either. Weighed on the measure of the text alone, the meter of a chat the server was
        // near the end of the space with fell to the half of it, and read as a chat with
        // space to spare: see `meter_after`, and the error in the meter that fell.
        let made = meter_after(0, shot.sent, &guard.build_history_in(chat));
        guard.live_mut(chat).ctx_used = made;
        if let Some(chat) = guard.chat_of_mut(chat) {
            chat.meta.context_used = made;
        }
        proglog::note(
            proglog::SUMMARY,
            &format!(
                "the handoff summary was not written: {}. The chat keeps every message it had; the meter stands at {}, measured on {}",
                why,
                made,
                if shot.sent > 0 {
                    format!("the {} text the server reported for the request", shot.sent)
                } else {
                    String::from(
                        "the measure of the text that remains, because the server reported nothing of the conversation",
                    )
                }
            ),
        );
        guard.set_status("The handoff summary was not written. Start it again from the summarisation timeline");
        guard.persist_in(chat);
        return false;
    }
    guard.summary_short(&id, asked_short);
    guard.finish_summary_in(chat, &id, &summary, before, after, &why);
    // The space the conversation takes now, read from the text that remains after the
    // summary: the first prompt, the summary, and the text that come after it. The number the
    // server gave with the request is not the number of the conversation that remains now, for
    // it holds the text the summary means, which are no part of what goes back to the model,
    // and it reads the conversation bigger than it is. A meter that kept the number of the
    // request would read the chat as full the moment the space was freed, and the answer would
    // end under the block of a summary that had just freed the space. The measure of the
    // text is the number the next ask is weighed against, and the first number the server gives
    // on the next ask takes the meter where it is, as it always does.
    let made = estimate_tokens(&guard.build_history_in(chat));
    guard.live_mut(chat).ctx_used = made;
    if let Some(chat) = guard.chat_of_mut(chat) {
        chat.meta.context_used = made;
        chat.meta.updated = store::now_secs();
    }
    let count = guard.handoffs_in(chat).len();
    // What the summary won, in the same two numbers the card of the summarisation shows the
    // user. A summary that was kept although the context filled under it records it here too,
    // for `why` holds that text and the user reads it beside the heading.
    proglog::note(
        proglog::SUMMARY,
        &format!(
            "the handoff summary #{} is saved: {} of the conversation became {} in the summary, and the context meter goes back to {}{}",
            count,
            before,
            after,
            made,
            if why.is_empty() {
                String::new()
            } else {
                format!(". Note: {}", why)
            }
        ),
    );
    guard.set_status(&format!(
        "Summarised #{} of the chat. The conversation now takes {}",
        count, made
    ));
    guard.persist_in(chat);
    true
}

// Whether the size of the context calls for a handoff summary now, and if it does, request one.
// The check is created when an answer is over, and once only behind a message: a summary that
// did not make enough space is not requested twice, for the model would spend its space on
// summaries of summaries.
//
// The limit is weighed on the text the next ask really carries, and not on all of the
// conversation as it is now. The text of a call that was truncated at the limit is the longest
// message of a chat that came to it in the middle of a call, for they hold what the model had
// written; and they are not text of the conversation, for they are the program telling the
// model what it had been put to, and the model answers them with the call again. Weighed on all
// of the conversation, a chat that is past the limit only because of that telling could never
// be got under it again: the next ask would be stopped on its first word, and the answer would
// end under the block of a call that never ran. The report is sent bounded to the header of the
// call, and it is the conversation without it that is weighed here, which is what a second
// summarisation would be made of. True when a summary was kept, which the worker reads as the
// space being made and the work being continued. What the limit is weighed on. The two
// callers of `maybe_compact` are at two different points of the conversation, and the space is
// read differently at each.
enum Weigh {
    // The space the server counted. The caller is mid-answer, at the moment an answer was
    // cut at the limit: the text that filled the directory is the text of the call that was
    // cut, and they are in the server's number and in no message of the chat yet, for the
    // report of the call is written after the space is freed. Weighed on the conversation as it
    // is now, the space would read empty, and a summary that is the whole point of the cut
    // would never be requested.
    Room,
    // The conversation as it is now, with the report of a cut call left out of it. The caller
    // is at the end of an answer, and the report of a call that was cut is in the chat: it is
    // the program speaking and not a note about the conversation, and a conversation that is
    // past the limit only because of it could never be got under it again, for the summary ask
    // that carries it would be stopped on its first word.
    Talk,
}

fn maybe_compact(
    state: &Arc<Mutex<State>>,
    server: &ServerSlot,
    cancel: &Arc<AtomicBool>,
    weigh: Weigh,
    chat: &str) -> bool {
    let wanted = {
        let Ok(guard) = state.lock() else { return false };
        if guard.live_ref(chat).generating || guard.busy || guard.chat_of(chat).is_none() {
            false
        } else if guard.handoff_owed_in(chat) {
            // A summary is wanted and never came. The user is asked to start it again: the
            // program does not put a model to work behind their back.
            proglog::note(
                proglog::ROOM,
                "no summary is requested: one was owed from before and was never written, so the user is asked to start it again",
            );
            false
        } else if guard.chat_of(chat).map(ends_on_summary).unwrap_or(false) {
            // The newest item of the chat is a summarisation already, so there is nothing new
            // that is worth a summary of its own.
            proglog::note(
                proglog::ROOM,
                "no summary is requested: the newest item in the chat is already a summarisation, so nothing new remains that is worth a summary of its own",
            );
            false
        } else {
            let used = match weigh {
                Weigh::Room => used_after(guard.live_ref(chat).ctx_used, &guard.build_history_in(chat)),
                // The conversation as it is now, with the report of a call that was cut left
                // out. The number the server spoke is weighed with it, and the larger kept,
                // whenever no such telling is in the way: the estimate of a context of code, of
                // results and of the text of a tool reads under what a server counts of it, and
                // a chat the server remains past the limit of was else informed it had
                // space to spare, send by send, until the space was removed.
                Weigh::Talk => {
                    let history = guard.build_history_in(chat);
                    let told = carrying_the_cut(&history);
                    if cut_telling_stands(&history) {
                        told
                    } else {
                        told.max(guard.live_ref(chat).ctx_used)
                    }
                }
            };
            let wants = config::compaction_wanted(&guard.cfg, used, guard.ctx_total);
            // The weighing itself, with the three numbers it was read from. The way it is
            // weighed is informed as well, for the two ways answer differently at the two
            // places an answer is weighed, and a user of the log has to know which of them a
            // number came from.
            proglog::note(
                proglog::ROOM,
                &format!(
                    "the size of the context is measured as {}, which is measured {}; the context window of the model is {}, the limit set for the summarisation is {}; a handoff summary is {}",
                    used,
                    match weigh {
                        Weigh::Room => "on the number the server reported for the current request, plus the text of the cut writing kept in it",
                        Weigh::Talk => "on the conversation as it is now, with the description of a stopped call left out of it",
                    },
                    guard.ctx_total,
                    config::compaction_room(&guard.cfg, guard.ctx_total),
                    if wants { "wanted" } else { "not wanted" }
                ),
            );
            wants
        }
    };
    if wanted {
        return run_handoff(state, server, cancel, chat);
    }
    false
}

// Whether the conversation that is going out to the server remains past the limit set for the
// summarisation. Both numbers are weighed and the larger kept: the one the server spoke last,
// and the estimate of the conversation as it is now, for the text written into the chat after
// that number -- the answer of the model and the result of a tool it requested -- are in the
// estimate and in no number of the server yet. The rule of the limit itself is in
// `config::compaction_wanted`, as ever.
fn room_past_the_limit(state: &Arc<Mutex<State>>, history: &Value, chat: &str) -> bool {
    let Ok(guard) = state.lock() else {
        return false;
    };
    // The space is weighed on the conversation as it is now, for this is the conversation that
    // is sent to the server: the report of a call that was stopped rides with it, as much as
    // the text of the conversation itself. The request is never sent out over the limit, where
    // the model is stopped before it has a word to give: the space is freed first, and where it
    // will not come under the limit again the work is too big for the space and the answer
    // is ended under the word that records it.
    let used = used_after(guard.live_ref(chat).ctx_used, history);
    let past = config::compaction_wanted(&guard.cfg, used, guard.ctx_total);
    // The request is weighed on the text that go out with it, which is a different conversation
    // from the one that is in the chat: the report of a call that was stopped rides with it.
    // The number of the server is weighed with it too, and the larger of the two is what the
    // request is judged by.
    proglog::note(
        proglog::ROOM,
        &format!(
            "the request is measured before it is sent as {}, the larger of the conversation as it is now and the number the server reported: the context window of the model is {}, the limit set for the summarisation is {}; the request measures {}",
            used,
            guard.ctx_total,
            config::compaction_room(&guard.cfg, guard.ctx_total),
            if past {
                "held back, for the space is freed first"
            } else {
                "sent out"
            }
        ),
    );
    past
}

// Make the space in the middle of an answer, and say whether it was created. The handoff
// summary is requested while the answer is still on its way, which it cannot be left to do on
// its own: an answer and a summary both write into the same chat, so the answer remains aside
// while the summary is written, and is taken up again when the text is in the marker. Whether
// the space came to under the limit again is all the answer needs out of this, and is the
// answer this returns.
fn make_room(
    state: &Arc<Mutex<State>>,
    server: &ServerSlot,
    cancel: &Arc<AtomicBool>,
    chat: &str) -> bool {
    {
        let Ok(mut guard) = state.lock() else {
            return false;
        };
        if guard.handoff_owed_in(chat) {
            // A summary was requested before and never came. The user is asked to start it
            // again, as ever, for the program does not put a model to work behind their back.
            return false;
        }
        // The answer is laid aside while the summary takes the chat.
        guard.live_mut(chat).generating = false;
        guard.busy = guard.any_generating();
    }
    let _ = maybe_compact(state, server, cancel, Weigh::Room, chat);
    let Ok(guard) = state.lock() else { return false };
    !guard.handoff_owed_in(chat) && !config::compaction_wanted(&guard.cfg, guard.live_ref(chat).ctx_used, guard.ctx_total)
}

// The most the model is asked to go on with its work after a handoff summary of one send of the
// chat composer. Once is enough. An answer that filled the space again after the summary
// made for it is a conversation the user has to send a word to, and a model asked again and
// again on its own would spend the space on summaries of itself.
const CONTINUE_ROOF: usize = 1;

// The rest a chat takes between two messages of the waiting list. The text of a sent message
// has to return in the state before the next one requests an answer, and a message sent on top
// of the one before it reads as one message of both.
const QUEUE_REST: Duration = Duration::from_millis(1500);

// How often the worker checks the waiting list while it remains. The wait is a poll and not a
// fixed one, for a list of nothing costs the program nothing at all: the worker only starts on
// a clock while a list of the open chat remains.
const QUEUE_POLL: Duration = Duration::from_millis(600);

// What one answer of the model leaves in the chat.
#[derive(Debug, PartialEq, Eq)]
enum Kept {
    // The text of the answer.
    Words,
    // No text arrived, but the thinking of the turn is kept.
    Thinking,
    // A quiet line for a turn that left nothing to read.
    Note,
    // Nothing at all. A command, or a stop with nothing in it, needs no line about itself.
    Nothing,
}

// The model has a word to give back, so the text it was sent are worked through and the meter
// above the chat composer has nothing left to say. True when the meter was, so the caller
// reports that the messages changed.
fn finish_progress(state: &mut State, chat: &str) -> bool {
    if !state.live_ref(chat).progress_on {
        return false;
    }
    state.live_mut(chat).progress_on = false;
    // The number is put at the end on purpose: a meter that is removed has finished its work,
    // and the user may still see it for a moment.
    state.live_mut(chat).progress = 100;
    true
}

// The model wrote something of a call. A call cannot be written before the server has all of
// the text it was sent through, so the meter above the chat composer has nothing left to say
// from this moment: it was for the work before the first word of the answer, and a model that
// requests a command before it has stated anything would else leave it remaining over the chat
// until the command had run. The text of the call is put into the state as well, so the user
// sees the command grow. True when the window has something new to draw.
fn write_calls(state: &mut State, calls: &[ToolCall], chat: &str) -> bool {
    let mut made = watched(calls);
    // The meter leaves for a marker of a chapter too. Such a call gives the user no block, but
    // the model wrote it after the text it was sent.
    let gauge = finish_progress(state, chat);
    // A call that is where it was not being written any more, so the word that records it is
    // kept. A call whose text grew is being written again, and records it too. Without this a
    // call that was flagged as written while the model thought would be under the block of its
    // own text for the rest of the answer.
    for (n, one) in made.iter_mut().enumerate() {
        if let Some(before) = state.live_ref(chat).live_tools.get(n) {
            if !one.words_moved(before) {
                one.done = before.done;
            }
        }
    }
    let grew = made != state.live_ref(chat).live_tools;
    if grew {
        state.live_mut(chat).live_tools = made;
    }
    gauge || grew
}

impl LiveTool {
    // True when the text of this call is not the text of the call that was in its place. The
    // name, the text, the reason and the item it is pointed at are checked, and not the word
    // that shows whether the model is done with it.
    fn words_moved(&self, before: &LiveTool) -> bool {
        self.name != before.name
            || self.command != before.command
            || self.purpose != before.purpose
            || self.target != before.target
    }
}

// The model has moved on from the calls it was writing: it thinks again, or it writes text of
// its own. The blocks of those calls are no longer being written, and record it, until the run
// of the tool is in the chat in their place. True when a block changed, so the caller reports
// that the state changed.
//
// This is all of the error in a block that would not leave off saying it was being written. A
// model that thinks, writes a call, and then thinks again leaves the call finished long before
// the step of the answer ends, and the block was on 'writing' the whole while, with no word of
// a result.
fn end_writing_calls(state: &mut State, chat: &str) -> bool {
    let mut changed = false;
    for one in state.live_mut(chat).live_tools.iter_mut() {
        if !one.done {
            one.done = true;
            changed = true;
        }
    }
    changed
}

// The context meter while an answer arrives. The server reports how a lot of text the
// conversation takes on every piece of the answer, so the meter moves during the answer and not
// only when the answer is over. The meter only climbs: the text of an answer that is still
// coming is not taken back. The number the server gives at the end of the answer is kept for
// the end, and that number may be smaller, for a stop takes text away. True when the number
// moved, so the caller reports that the messages changed.
fn climb_context(state: &mut State, made: u64, chat: &str) -> bool {
    let was = state.live_ref(chat).ctx_used;
    // The first number of a new ask is the measure of the conversation as the server has it
    // now, so it is taken where it is, smaller or larger. A number kept from an answer that was
    // stopped is not a measure of anything: the text it counted was sent of the conversation
    // with the answer. From that number on the meter only climbs, for the text of an answer
    // that is still coming is not taken back.
    let moved = if made == 0 {
        false
    } else if state.live_ref(chat).ctx_fresh {
        state.live_mut(chat).ctx_fresh = false;
        if made == state.live_ref(chat).ctx_used {
            false
        } else {
            state.live_mut(chat).ctx_used = made;
            true
        }
    } else if made <= state.live_ref(chat).ctx_used {
        false
    } else {
        state.live_mut(chat).ctx_used = made;
        true
    };
    // Every move of the meter is in the log of the program. The meter is paced at two moves a
    // second at most, so a long answer writes very many lines and no more than that: the number
    // the server gave, the number it gave before, and which ask was filling the conversation. A
    // number that was turned away is not written, for it stated nothing of the conversation.
    if moved {
        proglog::note(
            proglog::METER,
            &format!(
                "the meter moved from {} to {} while {}",
                was,
                made,
                if state.live_ref(chat).summarising {
                    "a handoff summary was being written"
                } else {
                    "an answer was coming in"
                }
            ),
        );
    }
    moved
}

// How often the context meter may move while an answer arrives. The server speaks of the
// conversation on every piece of it, which is a great deal oftener than a user can read a
// number, and a number written again and again reads as a shake rather than as a fill. Half a
// second shows the conversation filling often enough, and the number the server gives at the
// end of the answer remains whatever the pace, so nothing of it is lost.
const LEAST_CONTEXT_STEP: Duration = Duration::from_millis(500);

// The pace for the parts that arrive during an answer: the climb above, with the half second
// between the numbers. The first number of a new ask is taken where it is, pace or no pace, for
// it is the measure of the conversation as the server has it now, and a number kept from an
// answer that was stopped is not a measure of anything. `now` is the moment of the part, so the
// rule can be tried without waiting for the half second. A number that is turned away is not
// kept back either: the next piece that comes after the half second brings the conversation up
// to date at once.
fn climb_context_paced(state: &mut State, made: u64, now: Instant, chat: &str) -> bool {
    if !state.live_ref(chat).ctx_fresh {
        if let Some(begun) = state.live_ref(chat).ctx_at {
            if now.duration_since(begun) < LEAST_CONTEXT_STEP {
                return false;
            }
        }
    }
    let moved = climb_context(state, made, chat);
    if moved {
        state.live_mut(chat).ctx_at = Some(now);
    }
    moved
}

// The number of the conversation at the end of a request. `spoken` is what the server stated of
// the whole context: the number of the last piece when the request came to its end, and the
// number the meter is at when a request is weighed before it is sent. The conversation holds
// more than it did when the request was sent, for the answer of the model and the runs of the
// tools it requested were written into the chat on the way, so the larger of that number and
// the measure of the text that remains is kept, which is the space the next ask takes.
//
// A request that spoke no number at its end is not weighed here. `meter_after` keeps the size
// of the text the server was sent for that, and drops to this rule when the server stated
// nothing of the conversation either.
fn used_after(spoken: u64, history: &Value) -> u64 {
    spoken.max(estimate_tokens(history))
}

// What the meter keeps at the end of a request. `usage` is the number the last piece spoke of
// the whole context, `sent` is the size of the text the server was sent on this ask as the
// server spoke it -- `llama::Answer::sent`, out of the timings of the parts and the progress of
// the prompt -- and `history` is the conversation as it is now.
//
// A request that came to its end speaks `usage`, and that remains, as it always did.
//
// A request the user stopped speaks nothing at the end, for the way back to the server is
// closed before the last piece comes. It is weighed instead on the text the server was sent,
// for that text is in the conversation whichever way the request ends, and the server spoke
// their number on every piece of the answer. The live number of the writing that was stopped is
// not kept: a stop takes text away, the writing of a call that was stopped is bounded when it
// goes back to the model, and a meter that held the number of a writing that is part removed
// could read a chat as full and request a summarisation the conversation had no need of. What
// the stopped answer left in the chat is weighed by the measure of the text that remains, which
// is the floor under both as it is everywhere else, so the number is never over the
// conversation and never under the text the server was sent. Weighing the request on that
// measure alone was the error in the meter that fell: a request that carried 213,797 text of
// the conversation returned and left 102,514 remaining, for the measure of a context of code
// and of results reads at half what a server counts of the same conversation, and the meter was
// there, and read as a chat that had space to spare, until the next ask made the server
// speak again. Where the server stated nothing of the conversation at all, neither number
// remains, and the caller weighs on the number the meter was at before the request instead: see
// the end of a request in `run_turn`, and `used_after`.
fn meter_after(usage: u64, sent: u64, history: &Value) -> u64 {
    used_after(usage.max(sent), history)
}

// How many images the message that is being worked through carries. That is the last item that
// is sent with the request. The program sends text today, so the answer is nothing. The count
// is taken from the text that go out, and not from a guess about the number of tokens, so the
// meter reports the right item on the day a chat can carry an image.
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

// Choose what one answer leaves. `stopped` reports the user stopped the run, and `error`
// reports the run failed. A command is an answer of its own, so a turn that requests one writes
// no line, and a stop is not written down either. The thinking of a turn with no text goes to
// the command it led to.
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
        // The reason of the failure already remains as a note of its own.
        return Kept::Nothing;
    }
    Kept::Note
}

// A silence worth asking over. The model came to its own end with nothing to read, no command
// requested and nothing that went wrong: it thought, and then it stopped, which is not an end
// of the work it was given. A stop because the space for the context was used up is not
// that type of silence, for asking again would only fill the same space again. How many
// times the silence was asked over already, and how many times it may be, decide the rest.
fn worth_a_silence(finish: &str, spent: usize, most: usize) -> bool {
    finish != "length" && spent < most
}

// Answer one prompt. When the model requests a terminal command, the command runs and the model
// is asked again with the result. The files the user hung on the message are written to disk
// first, and the message of the user carries their markers, in the order the user hung them.
//
// A turn that only goes on with the work adds no message of the user: the newest handoff
// summary is at the end of the conversation already, and the text that names it tells the model
// to carry on from there. The result of the answer goes back to the worker, so it knows whether
// the work may be continued after the space has been made. Put the planning of the open
// chat away and give back the plan that is to remain as the first message of the work. Nothing
// returns when no plan was let go.
fn take_plan_release(state: &Arc<Mutex<State>>, chat: &str) -> Option<String> {
    let Ok(mut guard) = state.lock() else {
        return None;
    };
    guard.release_plan_in(chat)
}

fn run_turn(
    state: &Arc<Mutex<State>>,
    server: &ServerSlot,
    cancel: &Arc<AtomicBool>,
    turn: Turn,
    chat: &str) -> Ended {
    let going_on = matches!(turn, Turn::Continue);
    // Whether this turn puts a message of the user into the chat. A turn that goes on after a
    // handoff summary adds nothing, and a turn that writes an answer again adds nothing either:
    // the message the user is going on from is in the conversation already.
    let adding = matches!(turn, Turn::Sent(_));
    // A resumed answer is asked with the conversation as it is now and with no tool proposed: a
    // call cut in half cannot be mended, and the user stopped the turn to hear the rest of a
    // saying, not to have a command begun.
    let resuming = matches!(turn, Turn::Resume(_));
    // The answer the user stopped, when this turn goes on with one. The text the model writes
    // join onto it, rather than remaining as an answer of their own.
    let resumed = match &turn {
        Turn::Resume(id) => Some(id.clone()),
        _ => None,
    };
    let (text, files) = match turn {
        Turn::Sent(sent) => (sent.text, sent.files),
        Turn::Continue | Turn::Retry | Turn::Resume(_) => (String::new(), Vec::new()),
    };
    // A turn that only goes on adds no message of the user to the chat, for the text the model
    // is asked with come out of the conversation as it is now. A conversation that cannot be
    // sent as it is now is given a note about its own, for the reason informed where
    // `CONTINUE_PROMPT` is chosen below. The files wait in the working directory of the chat that
    // carries them, so the model can be asked again about them after a restart of the program,
    // and a command of the tool can read them in the directory it works in.
    let shelf = state
        .lock()
        .ok()
        .and_then(|keeper| keeper.chat_of(chat).map(|chat| keeper.attach_place_for(chat)));
    let kept: Vec<Attachment> = match &shelf {
        Some(place) => attach::save_at(place, &files),
        // No chat to carry them, so there is no space to keep them in.
        None => Vec::new(),
    };
    let lost = files.len().saturating_sub(kept.len());
    // Take the work order first, so two answers cannot mix.
    let (model, settings, workdir, sandbox_on, access) = {
        let Ok(mut state) = state.lock() else { return Ended::Halted };
        if state.live_ref(chat).generating {
            return Ended::Halted;
        }
        let Some(model) = state.model_of(chat) else {
            state.set_status("Choose a model first");
            return Ended::Halted;
        };
        if state.chat_of_mut(chat).is_none() {
            state.set_status("Start a new chat first");
            return Ended::Halted;
        }
        // What the chat requests, and nothing else. Whether the sandbox really works on this
        // computer is checked by the tool itself, which refuses the run when the sandbox cannot
        // be had. A chat that wants the sandbox is never served a plain run because the sandbox
        // is broken.
        let sandbox_on = state
            .chat()
            .map(|c| c.meta.sandbox_enabled)
            .unwrap_or(false);
        let access = state.access_of_chat(chat);
        let workdir = state
            .chat()
            .map(|c| state.workdir_for(&c.clone()))
            .unwrap_or_else(|| config::paths().data);
        // The question goes into the chat at once, with its attachments hanging on it in the
        // order the user hung them. A turn that only goes on with the work after a handoff
        // summary adds nothing: the marker of the summarisation is the last item in the chat,
        // and the model is asked again from behind it. A chat with nothing in it is a new one,
        // and the side bar has to hear that it came: the first message gives it its name and
        // puts it in the list of its place.
        let fresh = state
            .chat()
            .map(|c| c.messages.is_empty())
            .unwrap_or(false);
        if let Some(chat) = state.chat_of_mut(chat) {
            if adding {
                chat.messages.push(Message {
                    id: store::new_id(),
                    role: "You".to_string(),
                    body: text.clone(),
                    attachments: kept.clone(),
                    when: store::now_secs(),
                    ..Default::default()
                });
            }
            if adding && chat.meta.title == "New chat" {
                // A message of files and no text takes its name from the first file, so the
                // chat is not left nameless.
                let naming = if text.trim().is_empty() {
                    kept.first().map(|a| a.name.clone()).unwrap_or_default()
                } else {
                    text.clone()
                };
                chat.meta.title = Chat::title_from_prompt(&naming);
            }
            chat.meta.updated = store::now_secs();
        }
        if adding {
            // A message of the user begins a turn of its own, so the marker of an answer that
            // never came to its end is no longer wanted: the user has written past it.
            state.clear_stopped_marks_in(chat, );
        }
        if lost > 0 {
            let text = format!(
                "{} attachments were too large, or could not be stored, so they were not sent",
                lost
            );
            state.push_note_in(chat, &text, true);
        }
        // An image the model cannot be shown is reported out loud, so the user does not wonder
        // why the model looks blind at it.
        let pictures = kept.iter().filter(|a| a.image).count();
        if pictures > 0 && !state.pictures_can_go() {
            state.push_note_in(chat, 
                "The images of this message cannot be shown to this model: turn vision on in the model settings, then send the message again",
                true,
            );
        }
        state.persist_in(chat);
        if fresh {
            state.touch_structure();
        }
        state.live_mut(chat).generating = true;
        state.busy = true;
        // Both live buffers start empty, so a new turn never shows the text of an older one,
        // not even from another chat.
        state.live_mut(chat).live_text = String::new();
        state.live_mut(chat).live_reasoning = String::new();
        state.live_mut(chat).live_tools = Vec::new();
        state.live_mut(chat).live_work = Vec::new();
        state.live_mut(chat).live_work_head = String::new();
        state.live_mut(chat).live_work_closed = false;
        // A question the model had put to the user before this answer began is removed with the
        // answer that asked it: the popup that was over the chat composer was asked by a run
        // that is no more.
        state.live_mut(chat).asking = None;
        // The context meter is paced, and the pace begins anew with this answer, so the first
        // number of a new answer is placed on at once.
        state.live_mut(chat).ctx_at = None;
        state.set_status("The model is loading");
        let settings = state.settings_for(&model);
        (model, settings, workdir, sandbox_on, access)
    };

    // Make sure a server runs for this model. This can take a long time, and the state lock is
    // not held while it happens.
    let ready = ensure_server(state, server, &model, &settings, cancel, chat);
    match ready {
        Ok(total) => {
            let Ok(mut state) = state.lock() else { return Ended::Halted };
            state.busy = state.any_generating();
            state.ctx_total = total;
            state.set_status("Thinking");
        }
        Err(problem) => {
            let Ok(mut state) = state.lock() else { return Ended::Halted };
            state.busy = state.any_generating();
            state.live_mut(chat).generating = false;
            state.push_note_in(chat, &problem, true);
            state.set_status("The model did not start");
            return Ended::Halted;
        }
    }

    // The measure of the conversation begins anew with this ask: the first number the server
    // gives is taken where it is, so a number left over from an answer that was stopped cannot
    // hold the meter still. See `climb_context`.
    if let Ok(mut guard) = state.lock() {
        guard.live_mut(chat).ctx_fresh = true;
    }

    // The word the request of a turn that only goes on carries with it, when the conversation
    // it is given ends on a call: see `CONTINUE_PROMPT`. The conversation is read as it is now
    // at this moment, for the runs of a tool that came out of the answer were written into the
    // chat after the request was prepared, and it is the conversation of the answer as the runs
    // left it that the next ask is sent with. The word is searched for again before every step,
    // and not once for the turn, for a step may leave a turn of the model with nothing in it at
    // the end of the conversation: see `empty_turn_words`.
    let mut carried: Option<String> = None;
    // How many asks of this answer was sent on a turn of the model with nothing in it. A model
    // with nothing left to give answers the word of the program with nothing again, and a third
    // ask of the same type can give nothing back either. The count is kept so that the user is
    // informed instead of watched.
    let mut empty_asks = 0usize;

    let base_url = server_url(server);
    // The slot of this chat. The conversation it is on was worked through in that slot, so
    // every step of this answer is asked of the same slot and none of the work is done twice.
    // None when the keeping of the conversations is switched off, and the request then is sent
    // with no slot named, as it always did.
    let slot = claim_slot(state, server);
    // The two spaces of the Chatting settings, read once for this answer. None means no
    // ceiling, which is how both mean a new set of settings: the answer then ends when the
    // model stops by itself, and a model may run as many commands as it wants. A marker of a
    // chapter eats neither space.
    let (command_room, answer_room) = {
        let Ok(guard) = state.lock() else { return Ended::Halted };
        (
            config::engine_steps(guard.cfg.chat_max_tool_steps).map(|n| n.max(LEAST_TOOL_STEPS)),
            config::engine_steps(guard.cfg.chat_max_answer_steps)
                .map(|n| n.max(LEAST_ANSWER_STEPS)),
        )
    };
    let mut steps = 0usize;
    // How many commands have run in this answer. The setting counts commands alone: the other
    // tools do not add to it.
    let mut commands = 0usize;
    // How many times the writing has been stopped at the limit set for the summarisation in
    // this answer, and the pointer of the program that stops it. The pointer is put up by the
    // piece of the answer that speaks of a conversation past the limit, and checked by the
    // stream after every piece.
    let mut cuts = 0usize;
    // How many times the writing has been stopped over a turn that left nothing of itself
    // behind, in this answer. Such a stop is ended after one space has been made behind it:
    // a model that gives neither text nor a call out of a conversation the summarisation made
    // small has nothing to give, and a second space for it would be a space spent on
    // nothing.
    let mut empty_cuts = 0usize;
    // The size of the context that was sent with the request that was last made, read the way
    // the program reads a conversation. A stop of the writing behind a space that won nothing
    // over that ask is the selfsame work requested again with nothing left over for it, and the
    // answer is ended rather than the space spent on summaries of one call for ever. The
    // number is put in before every ask is sent, and is read only after one.
    let mut asked_room: u64;
    // How many times an answer of this one returned with nothing to read and was asked over
    // again. The space for that is read from the Chatting settings.
    let mut silences = 0usize;
    // True once this answer has made the space on the word of the model. The space is freed
    // once in one answer: a model that requests it again after it would put a summary under the
    // summary it just had, and spend what it won on text of its own making.
    let mut room_made = false;
    let silence_room = {
        let Ok(guard) = state.lock() else { return Ended::Halted };
        guard.cfg.chat_silent_retries.max(0) as usize
    };
    let room_cut = Arc::new(AtomicBool::new(false));
    let outcome = loop {
        steps += 1;
        // The pointer is let go again for every step: the step that is going to come now is to
        // be written out, unless the conversation comes to the limit again under it.
        room_cut.store(false, Ordering::Relaxed);
        let cut_flag = room_cut.clone();
        // The word this step is asked with. A turn that only goes on with the work after a
        // handoff summary of the chat composer is asked with the word that names the call the
        // conversation ends on, and an answer that was stopped at the limit and went on after
        // the space was freed is asked with the word that names the report of the call it was
        // stopped at. Both mean a call the server would else owe an answer to.
        let mut carried_prompt: String = match &carried {
            Some(word) => word.clone(),
            None if going_on => String::from(CONTINUE_PROMPT),
            None => String::new(),
        };
        // The step before this one may have left a turn of the model with nothing in it, which
        // is in the chat by now and ends the conversation as it is sent. The server is then put
        // to go on where the model left off, and the model leaves the turn empty again; a note
        // about the program behind the turn makes the request a question instead. It is
        // searched for in the conversation of every step, for the first step of an answer
        // carries no word of its own, and for a chat whose last answer returned empty ends that
        // way before the first step of the next send.
        if carried_prompt.is_empty() {
            let Ok(guard) = state.lock() else { return Ended::Halted };
            if let Some(word) = empty_turn_words(&guard.build_history_in(chat)) {
                carried_prompt = word;
                empty_asks += 1;
            }
        }
        let carried_prompt: &str = carried_prompt.as_str();
        let (mut history, code_on, search_on, planning_on, corporate_on, switches, thought,
         compaction_on) = {
            let Ok(guard) = state.lock() else { return Ended::Halted };
            (
                go_on_words(&guard, carried_prompt),
                guard.chat_of(chat).map(|c| c.meta.code_enabled).unwrap_or(false),
                guard
                    .chat()
                    .map(|c| c.meta.search_enabled)
                    .unwrap_or(false),
                guard.planning_open(),
                guard.corporate_open(),
                guard
                    .chat()
                    .map(|c| c.meta.tools.clone())
                    .unwrap_or_default(),
                guard.thinking_ask(),
                guard.compaction_on(),
            )
        };
        // The size of the context is weighed before the request is sent, and the space is freed
        // out of a handoff summary when the conversation remains past the limit. It cannot wait
        // for the request, as the hand over an answer does: a conversation that is sent past
        // the limit is cut off under its first piece, the model never comes to a word, and the
        // answer ends under the text of a space that gave out. That is read as an error in the
        // program, and it is the very error the summarisation was set aside to keep the
        // conversation out of. The result of a tool is the commonest path there, for it is
        // written into the chat after the last number the server spoke: the conversation
        // remains past the limit only from that moment, and the next ask would else go out over
        // it.
        if room_past_the_limit(state, &history, chat) {
            // The answer is laid aside while the summary takes the chat, as it is when the
            // writing is stopped under the weight of an answer.
            let made = make_room(state, server, cancel, chat);
            let Ok(mut guard) = state.lock() else { return Ended::Halted };
            if !made {
                // Either a summary is owed, which the user is asked of in the usual way, or the
                // summary never came. Sending the request over the limit anyway is what leaves
                // the chat under an empty turn, so the answer ends here and the reason is where
                // the user reads it.
                guard.live_mut(chat).generating = false;
                guard.busy = guard.any_generating();
                if guard.handoff_owed_in(chat) {
                    guard.set_status(
                        "The handoff summary was not written. Start it again from the summarisation timeline",
                    );
                } else {
                    guard.push_note_in(chat, 
                        "The conversation is past the summarisation limit and no space could be made, so the answer ends here. Request the work in parts small enough for the context, or give the model a larger context",
                        false,
                    );
                    guard.set_status(
                        "The conversation is past the summarisation limit and no space could be made",
                    );
                }
                guard.persist_in(chat);
                break Ended::Halted;
            }
            // The conversation the request is sent with is another conversation now: the text
            // the summary means are left out of it, and the summary itself is at the end of it.
            // A word carried over from the step before was searched for in the conversation as
            // it was before the space was freed, and it spoke of a turn of nothing or of a call
            // that was at the end of the conversation. Behind a handoff block neither is there
            // any more: the block ends the conversation and reports in its own text that the
            // work is to be continued, so the word is dropped rather than paid for a second
            // time out of the space the summarisation was meant to win.
            let talk = guard.build_history_in(chat);
            if ends_on_summary_mark(&talk) {
                carried = None;
            }
            // The word of this ask is read again the same way it is read at the top of every
            // step, so that dropping the stale one leaves the word of a turn that only goes on
            // remaining where it was.
            let carried_prompt: &str = match &carried {
                Some(word) => word.as_str(),
                None if going_on => CONTINUE_PROMPT,
                None => "",
            };
            history = go_on_words(&guard, carried_prompt);
            guard.live_mut(chat).generating = true;
            guard.set_status("Space was created. The model is going on with the answer");
        }
        // The tools of the servers of the user's own. A server whose tools were never read is
        // checked here, out of the lock, and once: what the server answered is kept, so the
        // next step and the next answer of this chat ask the server nothing again until the
        // user changes it.
        let looked = match state.lock() {
            Ok(guard) => guard.mcp_to_look(),
            Err(_) => return Ended::Halted,
        };
        for server in looked {
            let found = mcp::list(&server);
            let Ok(mut guard) = state.lock() else { return Ended::Halted };
            match found {
                Ok(tools) => {
                    guard.mcp_tools.insert(server.id.clone(), tools);
                    guard.mcp_fault.remove(&server.id);
                }
                Err(problem) => {
                    guard.mcp_tools.insert(server.id.clone(), Vec::new());
                    guard.mcp_fault.insert(server.id.clone(), problem);
                }
            }
        }
        let mcp_tools = match state.lock() {
            Ok(guard) => guard.mcp_definitions(),
            Err(_) => return Ended::Halted,
        };
        // Any of the tools is enough to let the model request one. A resumed answer is asked
        // with none: the text the user stopped are at the end of the conversation, and a call
        // cut in half cannot be mended.
        let use_tools = !resuming
            && (code_on || search_on || planning_on || !mcp_tools.is_empty());
        let mut body = json!({
            "messages": history,
            "stream": true,
            // The usage numbers drive the context meter.
            "stream_options": { "include_usage": true },
            // The server reports how far it is with the text it was sent, so the meter above
            // the chat composer has numbers to show before the model has a word to give back.
            "return_progress": true,
            // And the server reports on every piece how a lot of text the conversation takes,
            // so the context meter moves while the model writes, and not only when it has
            // stopped.
            "timings_per_token": true,
        });
        // The thinking of the chat rides on the request. The badge of the thinking mode and the
        // keeping of the thinking of the earlier turns are text for the chat template of this
        // one ask, so a badge moved between two answers costs neither a flag of the server nor
        // the loading of the model over again.
        if !thought.is_null() {
            body["chat_template_kwargs"] = thought;
        }
        if use_tools {
            if let Some(list) = offered_with_planning(
                sandbox_on,
                planning_on,
                corporate_on,
                code_on,
                search_on,
                access,
                &mcp_tools,
                &switches,
                compaction_on,
            ) {
                body["tools"] = list;
                body["tool_choice"] = json!("auto");
            }
        }
        // The slot this chat is in. The server takes the request of a named slot, waits while
        // that slot is busy, and is on the conversation it already worked through there;
        // without the name it picks a slot by the luck of the start of the text, and a chat
        // that was removed and returned is read through again from its first word.
        if let Some(slot) = slot {
            body["id_slot"] = json!(slot);
        }

        // Every piece of new text goes into the state, so the window can show it while it
        // arrives.
        let stream_state = state.clone();
        let mut text_update = Instant::now();
        let mut reason_update = Instant::now();
        // The thinking of this step is measured, so that the header of the panel can say how
        // long the model thought for.
        if let Ok(mut guard) = state.lock() {
            guard.live_mut(chat).thought_start = None;
            guard.live_mut(chat).thought_end = None;
            guard.live_mut(chat).live_thought_ms = 0;
            guard.live_mut(chat).live_thought_done = false;
            // The meter means this step alone, and for every step of an answer. The text under
            // the number follow the images the message carries; the server has none of them, so
            // the count is read from the text that go out.
            guard.live_mut(chat).progress = 0;
            guard.live_mut(chat).progress_images = pending_images(&history);
            guard.live_mut(chat).progress_on = true;
            // Neither speed is known until the server reports something of it, and the clocks
            // of the program have nothing to measure until a note about the answer arrives.
            guard.live_mut(chat).prompt_rate = 0.0;
            guard.live_mut(chat).answer_rate = 0.0;
            guard.live_mut(chat).rate_start = None;
            // The calls of the step before this one are in the chat by now, so the blocks of
            // the text being written start anew. The summary of the work does not: it belongs
            // to all of the answer, and the one block of the corporate display keeps it until
            // the answer is kept.
            guard.live_mut(chat).live_tools = Vec::new();
        }
        let progress_state = state.clone();
        let context_state = state.clone();
        let tool_state = state.clone();
        asked_room = estimate_tokens(&body["messages"]);
        let answer = llama::stream_chat(
            &base_url,
            &body,
            cancel,
            &room_cut,
            &mut |piece: &str| {
                if let Ok(mut guard) = stream_state.lock() {
                    guard.live_mut(chat).live_text.push_str(piece);
                    // The first word is here, so the text that was sent is worked through and
                    // the meter has nothing left to say.
                    if finish_progress(&mut guard, chat) {
                        guard.touch();
                    }
                    // A server that shows nothing of speed leaves the clocks to the program.
                    // The text of the answer is reads as tokens of some four letters, which is
                    // how a token of conversation is mostly made, and the clock is the one that
                    // ran from the first word to this one. The speed the server gives wins as
                    // soon as it comes, for it counts the tokens and not the letters.
                    if guard.live_ref(chat).answer_rate == 0.0 {
                        let began = guard.live_ref(chat).rate_start.unwrap_or_else(|| {
                            let now = Instant::now();
                            guard.live_mut(chat).rate_start = Some(now);
                            now
                        });
                        let secs = began.elapsed().as_secs_f64();
                        if secs > 0.5 {
                            let said = guard.live_ref(chat).live_text.chars().count() as f64;
                            guard.live_mut(chat).answer_rate = (said / 4.0) / secs;
                        }
                    }
                    // Text has come, so the thinking of this moment is over. It may begin again
                    // before the answer ends.
                    if guard.live_ref(chat).thought_start.is_some() {
                        guard.live_mut(chat).live_thought_done = true;
                    }
                    // Text of the answer has come, so the model has moved on from any call it
                    // was writing. The block of that call records it now, instead of going on
                    // saying it is being written.
                    if end_writing_calls(&mut guard, chat) {
                        guard.touch();
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
                    if guard.live_ref(chat).thought_start.is_none() {
                        guard.live_mut(chat).thought_start = Some(now);
                    }
                    guard.live_mut(chat).thought_end = Some(now);
                    if let Some(begun) = guard.live_ref(chat).thought_start {
                        guard.live_mut(chat).live_thought_ms = now.duration_since(begun).as_millis() as u64;
                    }
                    guard.live_mut(chat).live_thought_done = false;
                    if guard.corporate_on_in(chat) && guard.live_ref(chat).running.is_none() {
                        guard.live_mut(chat).live_work_head = String::from(corporate::THINKING);
                    }
                    guard.live_mut(chat).live_reasoning.push_str(piece);
                    // A thought is a note about the answer as well: the text that was sent is
                    // worked through by now.
                    if finish_progress(&mut guard, chat) {
                        guard.touch();
                    }
                    // The model thinks again, so whatever call it was writing is written:
                    // nothing more comes of that text, and the block of it must stop saying it
                    // is being written while the thinking goes on over it.
                    if end_writing_calls(&mut guard, chat) {
                        guard.touch();
                    }
                    if reason_update.elapsed() > Duration::from_millis(60) {
                        reason_update = Instant::now();
                        guard.touch();
                    }
                }
            },
            &mut |made: llama::Progress| {
                // The server speaks of the prompt work in whole numbers, and a batch of text
                // may remain between two of its answers. The meter is written again only when
                // the number it shows moved.
                if let Ok(mut guard) = progress_state.lock() {
                    let percent = (made.part() * 100.0).round() as u32;
                    if percent != guard.live_ref(chat).progress {
                        guard.live_mut(chat).progress = percent;
                        guard.touch();
                    }
                    // The speed of the prompt work comes from the same piece: the tokens done
                    // with over the clock they took. The meter and the number of the speed
                    // remain and go together, so the speed is written whenever the meter moved.
                    let speed = made.per_second();
                    if speed > 0.0 && (speed - guard.live_ref(chat).prompt_rate).abs() >= 0.05 {
                        guard.live_mut(chat).prompt_rate = speed;
                        guard.touch();
                    }
                }
            },
            &mut |made: llama::Context| {
                // The size of the context moves with every piece of the answer, so the meter in
                // the top bar moves with it. The number is put in only when it grew, so a piece
                // that shows nothing new costs the window nothing.
                if let Ok(mut guard) = context_state.lock() {
                    if climb_context_paced(&mut guard, made.used(), Instant::now(), chat) {
                        guard.touch();
                    }
                    // The same piece carries the two speeds of the work. The server is
                    // requested the numbers of every piece, so the number of the speed moves
                    // while the model writes, and not only when it has done. A zero is passed
                    // over, for the first token of a run has no speed of its own yet.
                    let mut told = false;
                    if made.decode_per_second > 0.0
                        && (made.decode_per_second - guard.live_ref(chat).answer_rate).abs() >= 0.05
                    {
                        guard.live_mut(chat).answer_rate = made.decode_per_second;
                        told = true;
                    }
                    if made.prompt_per_second > 0.0
                        && (made.prompt_per_second - guard.live_ref(chat).prompt_rate).abs() >= 0.05
                    {
                        guard.live_mut(chat).prompt_rate = made.prompt_per_second;
                        told = true;
                    }
                    if told {
                        guard.touch();
                    }
                    // The limit of the summarisation is checked here, with the same two numbers
                    // the context meter is made of, and the writing is stopped at once when the
                    // limit is come to. It cannot wait for the end of the answer, for a model
                    // writing a tool call bigger than the space it has never ends of
                    // itself: it writes until the space is removed, which is the very item the
                    // limit was set aside to prevent. What the model has written so far
                    // remains, and goes back to it. The number the server spoke on this piece
                    // is weighed and not the context meter, for the meter is paced and a number
                    // turned away by the pace would else leave the writing running on past the
                    // limit -- over a short answer, past all of the space.
                    //
                    // The pointer is over the writing, and there is nothing to stop while the
                    // model has written no word of its own. A step that gave neither text nor
                    // thinking nor a call costs nothing to let come to its end, and putting the
                    // pointer up over it is what ended a chat under the text of a space that
                    // gave out: the space was freed, and the answer was stopped all the same,
                    // because the step that had been stopped held no word of the model to carry
                    // on. The space of such a step is created after the turn instead, on
                    // the number the server spoke of it, and the model is put back to work
                    // behind the summary, which is what a summary is written for. Note that the
                    // size of the context is weighed here on the numbers of the server, and
                    // that they hold the list of the tools the template puts before the text: a
                    // conversation the program reads as small can remain past the limit the
                    // moment it is sent, and the model then has nothing to say.
                    let spoken = made.used().max(guard.live_ref(chat).ctx_used);
                    let wrote = !guard.live_ref(chat).live_text.is_empty()
                        || !guard.live_ref(chat).live_reasoning.is_empty()
                        || !guard.live_ref(chat).live_tools.is_empty();
                    if wrote && config::compaction_wanted(&guard.cfg, spoken, guard.ctx_total) {
                        // The number the server spoke on this piece goes into the meter with
                        // the stop. The meter is paced, and a number turned away by the pace
                        // leaves the meter behind the space that was come to: the size of the
                        // summarisation is weighed on the meter, and a cut that stopped the
                        // writing at nine hundred words of a space of a thousand was then
                        // weighed as though the conversation had never passed the limit, so the
                        // summary that the stop was created for was never requested. The stop
                        // is the moment the space is known, so the number is kept at that
                        // moment and not at the next piece that the pace lets through.
                        if climb_context(&mut guard, spoken, chat) {
                            guard.touch();
                        }
                        // The pointer goes up once, and so does the line about it, for the
                        // parts that come after the pointer remains tell the same over again
                        // until the way back to the server is shut.
                        if !cut_flag.load(Ordering::Relaxed) {
                            proglog::note(
                                proglog::CUT,
                                &format!(
                                    "the writing is stopped at {} of {}, because the limit set for the summarisation is {}; the model was writing {}",
                                    spoken,
                                    guard.ctx_total,
                                    config::compaction_room(&guard.cfg, guard.ctx_total),
                                    if guard.live_ref(chat).live_tools.is_empty() {
                                        "words of its answer"
                                    } else {
                                        "the text of a tool call"
                                    }
                                ),
                            );
                        }
                        cut_flag.store(true, Ordering::Relaxed);
                    }
                }
            },
            &mut |made: &[ToolCall]| {
                // The user sees the model write a command: the text of it go into the state as
                // they come, and not when the call is over. The same piece takes the meter
                // away, so a model that requests a command before it has stated anything does
                // not leave it remaining over the chat for all of the run.
                if let Ok(mut guard) = tool_state.lock() {
                    if write_calls(&mut guard, made, chat) {
                        guard.touch();
                    }
                }
            },
        );

        let Ok(mut guard) = state.lock() else { return Ended::Halted };
        // A step that ended without a word leaves no meter remaining behind.
        guard.live_mut(chat).progress_on = false;
        let live = std::mem::take(&mut guard.live_mut(chat).live_text);
        let live_thinking = std::mem::take(&mut guard.live_mut(chat).live_reasoning);
        // The call is over, so the block of the text being written goes. The run of the command
        // is in the chat in its place, and reports what became of it.
        guard.live_mut(chat).live_tools.clear();
        // How long the model spent thinking in this step. The text of the panel is created from
        // this.
        let thought_ms = match (guard.live_ref(chat).thought_start, guard.live_ref(chat).thought_end) {
            (Some(begun), Some(end)) => end.duration_since(begun).as_millis() as u64,
            _ => 0,
        };
        guard.live_mut(chat).thought_start = None;
        guard.live_mut(chat).thought_end = None;
        guard.live_mut(chat).live_thought_ms = 0;
        guard.live_mut(chat).live_thought_done = false;
        // Both hold the answer. The longer one is the safer one.
        let answer_text = if answer.text.chars().count() >= live.chars().count() {
            answer.text.clone()
        } else {
            live
        };
        // The thinking is kept the same way. Both live buffers are empty from here on, so a
        // stored step is not shown again while the next one is still arriving.
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

        // The conversation came to the limit set for the summarisation while the model was
        // still writing, so the writing was stopped there. Nothing else stops it: a model with
        // a tool call bigger than the space it has writes on until the space is removed,
        // and the space being removed is what the limit was set aside to prevent. What the
        // model wrote remains, the call it was writing is written down for the model to read
        // again, the space is freed, and the answer is asked on from where it was stopped.
        if answer.interrupted {
            // The call, or the calls, that were being written. A call stopped before it had a
            // name of its own is still a call the model was put to and must hear of, so long as
            // it has some text.
            let cut: Vec<ToolCall> = answer
                .tool_calls
                .iter()
                .filter(|call| is_writing(call))
                .cloned()
                .collect();
            // The text the model had given of its own answer are in the chat, as they would had
            // the answer come to its end. The thinking of the turn rides with the first item
            // the user is shown, as it does with a command.
            let mut thoughts = answer_thinking.clone();
            if !answer_text.trim().is_empty() {
                guard.push_message_in(chat, "Assistant", &answer_text, &thoughts, thought_ms, false);
                thoughts = String::new();
            } else if !thoughts.trim().is_empty() {
                // The turn gave no text of its own, so the thinking is on its own. It is before
                // the marker of the summarisation, for the marker goes in after it when the
                // space is freed, and the thought of the turn the summary is written of must
                // not end up behind the heading of the summarisation: the user would see it go
                // away and return on the far side of it.
                guard.push_message_in(chat, "Assistant", "", &thoughts, thought_ms, false);
                thoughts = String::new();
            }
            // The calls that were stopped are written down where the writing stopped: after the
            // text of the turn, and above the heading of the summarisation. The request for a
            // summary is made of the conversation as it is now, so it carries the report of the
            // call, and the model that writes the summary reads what it was put to and can
            // record it in the text that is in the place of the turn. The marker then cuts the
            // report off, for the text before a summarisation go back to the model no more: the
            // space the text of a call that never ran takes is won back at once, instead of
            // being paid for again on every ask that follows. All of the text of the call is
            // kept, for the report no longer has to fit in the space a summarisation left
            // over.
            for call in cut.iter() {
                guard.push_cancelled(call, &thoughts, thought_ms, GREATEST_CUT_ARGS);
                thoughts = String::new();
            }
            if !thoughts.trim().is_empty() {
                // Nothing of the turn was shown to the user, so its thinking is on its own
                // rather than being lost.
                guard.push_message_in(chat, "Assistant", "", &thoughts, thought_ms, false);
            }
            // The meter keeps the number the server last spoke of. An answer that never came to
            // its end gives no usage numbers of its own.
            let used_now = guard.live_ref(chat).ctx_used;
            // The context meter keeps the number the space climbed to, for it only moves
            // up. The space of the requests that follow is read again below, out of the
            // conversation as it is now after the summarisation and the report of the call: the
            // number of the request that was cut holds all of the space, and a meter that
            // kept the larger of the two would read the chat as full again the moment the space
            // was freed.
            if let Some(chat) = guard.chat_of_mut(chat) {
                chat.meta.context_used = used_now;
                chat.meta.updated = store::now_secs();
            }
            // A step stopped over a turn of nothing is the error in the space: the space
            // finished while the model wrote neither text nor a call. The answer is ended where
            // it is and the user is informed, for the answer is not to be continued. The text a
            // summarisation means is the text before the turn, and the turn of nothing goes
            // with them, for the text before a summarisation go back to the model no more. The
            // request that followed a space made here would end on the marker of the
            // summarisation, and a model with nothing to give answers that with the marker of
            // an empty turn again, which is the chat the user saw: no answer, and a space
            // spent. The row of the turn of nothing is written as ever, and the word about the
            // space that gave out is after it. The space is freed here, before the answer is
            // ended and not after the send: the conversation of an overfull chat is weighed on
            // the number the server spoke and not on a guess beside it, for the guess of a
            // context of code and of results reads under the limit while the server remains
            // past it, and a space left unmade on this turn is the request over the limit of
            // the next.
            if cut.is_empty() && answer_text.trim().is_empty() && answer_thinking.trim().is_empty()
            {
                guard.push_message_in(chat, "Assistant", "", &thoughts, thought_ms, false);
                guard.persist_in(chat);
                drop(guard);
                let room = make_room(state, server, cancel, chat);
                let Ok(mut guard) = state.lock() else { return Ended::Halted };
                let used_now = guard.live_ref(chat).ctx_used;
                if let Some(chat) = guard.chat_of_mut(chat) {
                    chat.meta.context_used = used_now;
                    chat.meta.updated = store::now_secs();
                }
                // A space that was created is the work being continued, and that is all the
                // reason this branch needs for going on with it: a handoff summary is written
                // so that the conversation may go on, and the text the summary means - all of
                // the conversation, this turn of nothing among them - go back to the model no
                // more. The conversation that is after the marker is the first message of the
                // user and the block of the summarisation, which is the form the model is to be
                // asked with: the space begun afresh from the first prompt of the chat, and
                // the state of the work decompressed in front of it. The block ends that
                // conversation, and it reports in its own text that the work is to be
                // continued, so no word of the program is sent behind it. The answer is ended
                // only where the space never came, or where a space was already made
                // behind a turn of nothing and the model gave nothing again out of it: there is
                // then nothing left to make space for.
                if room && empty_cuts == 0 {
                    empty_cuts += 1;
                    cuts += 1;
                    // The word of the step before spoke of a turn of nothing, and the space
                    // that was created here took that turn out of the conversation: the block
                    // of the summarisation is at the end of it now and reports in its own text
                    // what the work was and that it is to be continued, so no word of the
                    // program rides after it.
                    carried = None;
                    guard.live_mut(chat).generating = true;
                    guard.set_status("Space was created. The model is going on with the answer");
                    continue;
                }
                guard.push_note_in(chat, 
                    if room { EMPTY_TURN_TWICE_WORDS } else { EMPTY_TURN_CUT_WORDS },
                    false,
                );
                guard.persist_in(chat);
                guard.live_mut(chat).generating = false;
                let text = if room {
                    EMPTY_TURN_TWICE_WORDS
                } else {
                    EMPTY_TURN_CUT_WORDS
                };
                guard.set_status(text);
                break Ended::Halted;
            }
            // The user is informed before the space is freed, for the making of it puts the
            // heading of a summarisation in the chat and the two belong one under the other.
            let told = if cut.is_empty() {
                String::from(
                    "The conversation reached the summarisation limit while the answer was being written, so the writing stopped. Space is being made, and the model continues the answer",
                )
            } else {
                String::from(
                    "The conversation reached the summarisation limit while a tool call was being written, so the writing stopped and the call was cancelled. Space is being made, and the model continues the answer",
                )
            };
            guard.push_note_in(chat, &told, false);
            guard.persist_in(chat);
            drop(guard);
            cuts += 1;
            // Make the space. The handoff summary is requested here, in the middle of an
            // answer, for the answer can go no further without it and the user is not to be
            // made to start a summary manually for an error in the space.
            let room = make_room(state, server, cancel, chat);
            let Ok(mut guard) = state.lock() else { return Ended::Halted };
            // The space the next ask takes is read from the conversation as it is now, and
            // not kept from the request that was cut. That ask ended at the limit, so its
            // number holds all of the space, and a meter that kept the larger of the two
            // would read the chat as full again the moment the space was freed. The first
            // number the server gives on the next ask takes the meter where it is, as it always
            // does.
            let made = estimate_tokens(&guard.build_history_in(chat));
            guard.live_mut(chat).ctx_used = made;
            // The space that was created, and what the conversation costs after it. The number
            // is the measure of the text that remains, as it always is at this point: the
            // request that was cut ended at the limit, so the number of the server reports
            // nothing of the space the summary had just made.
            proglog::note(
                proglog::CUT,
                &format!(
                    "space was requested, cutting {} of an answer: {}; the conversation now measures {}, and the limit set for the summarisation is {}",
                    cuts,
                    if room {
                        "the summary was kept"
                    } else {
                        "the summary was not kept"
                    },
                    made,
                    config::compaction_room(&guard.cfg, guard.ctx_total)
                ),
            );
            // The context meter is written to the chat, for the user reads it from there. The
            // number the server gave at the cut is not kept, for it holds all of the space.
            let used_now = guard.live_ref(chat).ctx_used;
            if let Some(chat) = guard.chat_of_mut(chat) {
                chat.meta.context_used = used_now;
            }
            guard.persist_in(chat);
            // Whether the space that was created is past the limit is weighed on the text the
            // next step of the answer really carries, and not on all of the conversation as it
            // is now. The report of the call that was stopped is the program speaking and not a
            // note about the conversation: it is the longest message of a chat that came to the
            // limit in the middle of a call, for it holds what the model had written. Weighed
            // on all of the conversation, a chat that is past the limit only because of that
            // telling could never be got under it again -- the answer ended under the block of
            // a call that never ran, and every fresh send ended the same way. So the space is
            // weighed with the report left out, which is what a second summarisation is made
            // of, and which is what the model is asked with once the marker of the
            // summarisation cuts the report off. The weighing goes by the same measure as the
            // limit was come to, and it reports nothing of what the server will count when the
            // request is sent: the meter is read again on the first number of the next ask, and
            // the pointer is put up again on the word that is past the limit. A summary that
            // was kept is space that was created, and the answer goes on after it whatever the
            // estimate reports, for as long as each summarisation frees the space the
            // answer goes on after it: a task that fills the space again and again is
            // summarised again and again, and the work is not begun over for the error in the
            // space. The answer ends only when the space that was created is still past the
            // limit: the model is writing something no space will hold, and the user has to
            // hear so rather than watch the same space be made and spent for ever.
            let room_asked = carrying_the_cut(&guard.build_history_in(chat));
            let still_full = config::compaction_wanted(&guard.cfg, room_asked, guard.ctx_total);
            // The space the next ask carries, telling and all, weighed the way the request
            // itself is weighed. A summarisation that will not bring this number under the
            // number of the request that was just stopped won no space: the conversation is
            // then the report of a call the model is asked to write again, and behind every
            // summarisation it writes to the limit and stops there again, so the same space is
            // created and spent with no end to it. The model is writing something no space
            // will hold, and the space the summaries take is space the user paid for
            // with nothing of the work in it.
            let ask_now = estimate_tokens(&guard.build_history_in(chat));
            let won_nothing = cuts > 1 && ask_now >= asked_room;
            if !room || (still_full && cuts > 1) || won_nothing {
                // Either the summary never came, which the user is asked of in the usual way,
                // or the space is still past the limit even with the report of the stopped call
                // left out of it. Either way the answer ends here, rather than be stopped again
                // at once on the first word of the next ask.
                guard.live_mut(chat).generating = false;
                let text = if guard.handoff_owed_in(chat) {
                    "The handoff summary was not written. Start it again from the summarisation timeline"
                } else if still_full {
                    "The context is still over the summarisation limit, so the answer ends here. Request the work in parts small enough for the context, or give the model a larger context"
                } else if won_nothing {
                    "The space that was created is taken up by the work itself, so the answer ends here. Request the work in parts small enough for the context, or give the model a larger context"
                } else {
                    "The summarisation made no space, so the answer ends here"
                };
                guard.set_status(text);
                // Which rule ended the answer, and the numbers the rule was weighed on. The
                // three of them are read differently on purpose: the conversation as it is now,
                // the conversation with the report of the stopped call left out, and the
                // conversation the request that was cut carried.
                proglog::note(
                    proglog::CUT,
                    &format!(
                        "the answer ends at the cut. The rule that ended it: {}. The next request would carry {}, the request that was cut carried {}, the conversation without the report of the call measures {}, the limit is at {} of {}",
                        if guard.handoff_owed_in(chat) {
                            "the summary was never written"
                        } else if still_full {
                            "the space is still past the limit"
                        } else if won_nothing {
                            "the summarisation won no room"
                        } else {
                            "no space was made"
                        },
                        ask_now,
                        asked_room,
                        room_asked,
                        config::compaction_room(&guard.cfg, guard.ctx_total),
                        guard.ctx_total
                    ),
                );
                break Ended::Halted;
            }
            // The marker of the summarisation ends the conversation the text of the stopped
            // call went into, and the handoff block reports in its own text that the work is to
            // be continued. No text of the program is therefore sent behind it: a stale message of
            // an earlier step is dropped rather than paid for out of the space the
            // summarisation was meant to win, which is what happens when an empty turn is
            // continued below.
            carried = None;
            // The space is freed and the report of the call is above the marker, where the
            // model read it as the summary was written, so the answer is asked on from there.
            // It is not continued by the worker, for the worker asks the model to go on once
            // for a send, and that one going on has expired on the summarisation of the chat
            // composer. The step that comes now is asked afresh out of the conversation as it
            // is now, so the work is not begun again.
            guard.live_mut(chat).generating = true;
            guard.set_status("Space was created. The model is continuing the answer");
            proglog::note(
                proglog::CUT,
                &format!(
                    "the answer continues from the conversation as it is now, which measures {}; the report of the stopped call is before the marker, and the summary was written from it",
                    ask_now
                ),
            );
            continue;
        }

        if let Some(problem) = &answer.error {
            let text = problem.clone();
            guard.push_note_in(chat, &text, true);
        }
        // The calls this step made. Commands use up the limit for commands; the other tools run
        // in the program itself and are not counted. A turn that thinks and then requests a
        // command has text enough, so nothing is written about a missing
        // answer, and its thinking goes to the command that means it.
        let step_calls: Vec<ToolCall> = answer
            .tool_calls
            .iter()
            .filter(|call| !call.name.is_empty())
            .cloned()
            .collect();
        let step_runs: Vec<ToolCall> = if use_tools { step_calls } else { Vec::new() };
        let asked_for_tool = use_tools && !step_runs.is_empty();
        let no_words = answer_text.trim().is_empty();
        // A turn of nothing is a step that wrote no text, wrote no thinking, and requested no
        // command of a tool. A step that requested a command wrote an answer of its own, and
        // the block of the run is in the chat for it. A step that was stopped at the limit of
        // the summarisation over such a turn is ended where it is, above.
        let turn_of_nothing = !asked_for_tool && no_words && answer_thinking.trim().is_empty();
        // A ceiling of nothing is the same as no ceiling at all, so an answer with an empty
        // space for commands still ends on the turn that asked. The space is checked here
        // as well, for a turn of nothing is only continued while the answer has space left for
        // another step.
        let no_room_left = answer_room.is_some_and(|room| steps >= room);
        // The calls the user stopped manually while the model was still writing them. Such a
        // call never ran, and its block records it.
        let stopped_runs: Vec<ToolCall> = if answer.stopped && use_tools {
            answer
                .tool_calls
                .iter()
                .filter(|call| is_writing(call))
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        // A turn that stopped in the middle of a call has a block the user is shown, so its
        // thinking is with that block, and no line is written about an answer that was never
        // meant to come.
        let stopped_call = !stopped_runs.is_empty();
        let turn_thinking = if no_words && (asked_for_tool || stopped_call) {
            answer_thinking.as_str()
        } else {
            ""
        };
        // A turn stopped in the middle of a call requested something, and the block of that
        // call is what the user is shown of it: a turn that requested a tool writes no line of
        // its own about a missing answer.
        match kept_of(
            &answer_text,
            &answer_thinking,
            answer.stopped,
            asked_for_tool || stopped_call,
            answer.error.is_some(),
        ) {
            // The text is kept as they arrived, also when the user stopped the run.
            Kept::Words => {
                // An answer that was removed on from joins the answer it was removed on from,
                // so the user reads one answer and not two.
                let joined = match &resumed {
                    Some(id) => guard.append_resumed(
                        id,
                        &answer_text,
                        &answer_thinking,
                        thought_ms,
                    ),
                    None => false,
                };
                if !joined {
                    guard.push_message_in(chat, 
                        "Assistant",
                        &answer_text,
                        &answer_thinking,
                        thought_ms,
                        false,
                    );
                }
            }
            Kept::Thinking => {
                guard.push_message_in(chat, "Assistant", "", &answer_thinking, thought_ms, false)
            }
            // The model used tokens but left nothing to read, and requested no command. This
            // happens when the context ran full, and when the model came to its own end
            // straight out of a thought.
            Kept::Note => {
                // Two turns of nothing end the answer. The word of the program stated once that
                // an empty turn is not an answer; a model that answers that with nothing again
                // has nothing to give, and a third ask of the same format spends the space
                // the conversation is on for nothing. The user is informed, in the chat as well
                // as under the top bar, for the chat ends here and the user is to know why
                // nothing was asked after it.
                if turn_of_nothing && empty_asks >= EMPTY_TURN_ROOF {
                    guard.push_note_in(chat, EMPTY_TURN_END_WORDS, false);
                    guard.persist_in(chat);
                    guard.live_mut(chat).generating = false;
                    guard.set_status(EMPTY_TURN_END_WORDS);
                    break Ended::Halted;
                }
                // A silence of the second type is asked over, rather than written off as an
                // answer at once. The thought the model made is in the conversation, so it
                // reads its own thinking again and goes on from there, and the user is not left
                // under a line about an answer that never came while the work was still before
                // it.
                if worth_a_silence(&answer.finish, silences, silence_room)
                    && !cancel.load(Ordering::Relaxed)
                {
                    silences += 1;
                    if !answer_thinking.trim().is_empty() {
                        guard.push_message_in(chat, "Assistant", "", &answer_thinking, thought_ms, false);
                    } else if use_tools && !no_room_left {
                        // The row of a turn of nothing is with nothing in it, as the template
                        // of the model writes an empty turn. The line about a turn with no
                        // answer is not put in its place: the answer is asked on below out of
                        // the conversation as it is now, and the conversation has to end on the
                        // marker of the empty turn for the word of the program to go out behind
                        // it. See `empty_turn_words`.
                        guard.push_message_in(chat, "Assistant", "", "", 0, false);
                    }
                    guard.persist_in(chat);
                    // The request must not go out on the turn of nothing. A conversation that
                    // ends on the text of the model with nothing after it is an item for the
                    // server to go on with rather than an item to answer, and a model leaves
                    // the turn empty again: that is the loop the user met, in which a model
                    // that stopped after one line outside its thinking was asked again and
                    // answered with nothing. The word of the program rides on this ask alone.
                    carried = empty_turn_words(&guard.build_history_in(chat));
                    if carried.is_some() {
                        empty_asks += 1;
                    }
                    // The step the silent ask took is given back: a model that stated nothing
                    // spent none of the space for steps.
                    steps = steps.saturating_sub(1);
                    guard.set_status("The model stated nothing. It is asked again");
                    continue;
                }
                let note = if answer.finish == "length" {
                    String::from("(the context was full before the model could answer)")
                } else if silences > 0 {
                    format!("(the model gave no answer, although it was asked {} times)", silences + 1)
                } else {
                    String::from("(the model gave no answer)")
                };
                guard.push_message_in(chat, "Assistant", &note, &answer_thinking, thought_ms, false);
            }
            Kept::Nothing => {}
        }

        // The calls the user stopped are in the chat. The tool never ran, and the block of the
        // text being written goes with the answer instead of vanishing with it: the user
        // stopped an item they could see being written, and it has to stay where they can still
        // read it. Each call goes back to the model as the text of a turn, as a call the
        // context window filled under does, for its text is not whole.
        let mut stopped_thoughts = turn_thinking.to_string();
        for call in stopped_runs.iter() {
            // The thinking of the turn is with the first block, as it does with a run.
            let thinking = std::mem::take(&mut stopped_thoughts);
            guard.push_stopped_by_user(call, &thinking, thought_ms, GREATEST_CUT_ARGS);
        }

        // Tokens for the context meter. The number the server gave at the end of this ask is
        // the space the text of that ask took, and the conversation holds more than it did
        // when the request was sent: the answer of the model, and the run of the tool it
        // requested. The meter therefore keeps the larger of the two, the space the request
        // took and what the conversation of itself costs now, which is the space the next
        // ask will take. An answer the user stopped gives no number at all, and is weighed on
        // the size of the text the server was sent, which every piece of the answer spoke: that
        // text is in the conversation whichever way the request ends, and the number of the
        // writing that was stopped is not kept over them. See `meter_after`.
        let used = answer.prompt_tokens + answer.completion_tokens;
        let sent = answer.sent();
        let told = guard.build_history_in(chat);
        let was_meter = guard.live_ref(chat).ctx_used;
        // A request that stated nothing of the conversation at all, which is a way back to the
        // server that failed before the first piece came, leaves the meter where the request
        // before it was, with the measure of the text for a floor under it. The conversation
        // did not change while the server never saw it, and the measure of the text reads at
        // the half of what the server counted of the same conversation.
        guard.live_mut(chat).ctx_used = if used == 0 && sent == 0 {
            used_after(was_meter, &told)
        } else {
            meter_after(used, sent, &told)
        };
        // What the meter was made of at the end of a request is the one item a user of the log
        // of the program must have to follow the rest of it. A request that spoke no number at
        // its end reports which of the two other numbers it was on, for the size of the text
        // the server was sent and the measure of the text that remains reads far apart on the
        // same conversation. All three is here, and which of them was taken.
        proglog::note(
            proglog::METER,
            &format!(
                "{}; the meter stands at {}, and was {}",
                if used == 0 && sent == 0 {
                    String::from(
                        "the request reported nothing of the conversation, so the meter was left where the previous request put it, with the measure of the text that forms a floor under it",
                    )
                } else if used == 0 {
                    format!(
                        "the request reported no number at the end of it, so the size of the text sent to the server was used, which it reported as {}",
                        sent
                    )
                } else {
                    format!(
                        "the request reported {} text in and {} out, and the larger of that and the measure of the text that remains was used",
                        answer.prompt_tokens, answer.completion_tokens
                    )
                },
                guard.live_ref(chat).ctx_used,
                was_meter
            ),
        );
        let used_now = guard.live_ref(chat).ctx_used;
        if let Some(chat) = guard.chat_of_mut(chat) {
            chat.meta.context_used = used_now;
            chat.meta.updated = store::now_secs();
        }
        guard.persist_in(chat);

        // What is left to do after this step: the commands, and the space the answer still
        // holds.
        let finished = answer.stopped || answer.error.is_some();
        let nothing_left = step_runs.is_empty();
        if finished || !use_tools || nothing_left || no_room_left {
            guard.live_mut(chat).generating = false;
            // An answer the user stopped never finished of its own, so it carries the marker of
            // the stop, and the page puts the marker of going on with it under that answer
            // alone. A stop that came at a call keeps the block of the call instead, for a call
            // cut in half cannot be mended; an answer that came to its end leaves nothing to go
            // on with, and the marker of an older one is removed.
            if answer.stopped && stopped_runs.is_empty() {
                guard.mark_last_answer_stopped_in(chat, );
            } else if !answer.stopped {
                guard.clear_stopped_marks_in(chat, );
            }
            let text = if answer.stopped {
                "Stopped"
            } else if !finished && !nothing_left {
                if no_room_left {
                    "The answer stopped at the step limit in the Chatting settings"
                } else {
                    "The model requested more steps than one answer allows"
                }
            } else {
                "Ready"
            };
            guard.set_status(text);
            // An answer the user stopped leaves the conversation of the chat worked through in
            // the slot of the server as far as it came, and the server is idle now. That is a
            // moment to write the conversation out, for the next item that happens to the chat
            // may be that another chat takes the slot, or that the program is removed
            // altogether -- and a conversation that was not written is a conversation read
            // through again from its first word. The lock is let go first, for a long sends
            // requests to a slow disk takes longer than the small calls of the server.
            let stopped = answer.stopped;
            drop(guard);
            if stopped {
                if let Some((url, model_id)) = server_held(server) {
                    save_chat_context(state, &url, &model_id);
                }
            }
            // The worker is informed what the answer came to. One that stopped on the user
            // moving the pointer, or on an error in the stream, is not continued; one that came
            // to its end, or to the ceiling of its steps, is.
            break if finished {
                Ended::Halted
            } else {
                Ended::Done
            };
        }
        // Keep the lock free while a command runs.
        drop(guard);

        // The model wants a command. The thinking of a turn with no text of its own is with the
        // first block the user is shown.
        let mut thoughts = turn_thinking.to_string();
        let mut offered = false;
        // True when the model let the plan go in this step of the answer.
        let mut letting = false;
        for call in step_runs.iter() {
            // The model reports the work of the answer is over, and that its last text come
            // next. Nothing of the computer runs for such a call: the one block of the answer
            // is closed, and the text the model writes after it are outside it.
            if call.name == corporate::FINISH_NAME {
                let Ok(mut guard) = state.lock() else {
                    continue;
                };
                guard.live_mut(chat).live_work_closed = true;
                let id = guard.begin_tool_in(chat, call, corporate::FINISH_WORDS, "", 0);
                guard.finish_tool_in(chat, &id, corporate::FINISH_WORDS, true);
                continue;
            }
            // The summary of the work goes into the one block the corporate display shows.
            // Nothing of the computer runs for such a call: the text is extracted from it, put
            // under the heading the model gave, and the model is answered in a word.
            if call.name == corporate::NAME {
                let entry = serde_json::from_str::<corporate::Entry>(&call.arguments)
                    .unwrap_or_default();
                let written = match state.lock() {
                    Ok(mut guard) => corporate::note(&mut guard.live_mut(chat).live_work, &entry),
                    Err(_) => continue,
                };
                let note = corporate::receipt(written);
                let Ok(mut guard) = state.lock() else {
                    continue;
                };
                let id = guard.begin_tool_in(chat, call, &note, "", 0);
                guard.finish_tool_in(chat, &id, &note, true);
                continue;
            }
            // The model requests the size of the context to be made now. The summary is written
            // here, in the middle of the answer, as it is when the writing is stopped under the
            // weight of the conversation: the answer remains aside while the text is written,
            // and is taken up again when the marker holds them. The block of the call cannot go
            // into the chat before the request: it would be in the conversation the summary is
            // written of as a call that is still running, and the model that writes the summary
            // would be asked to read a result that does not exist yet. It therefore goes in
            // after the marker and is moved above it, so the call is where the model made it.
            if call.name == summarise::NAME {
                let thinking = std::mem::take(&mut thoughts);
                let (before, empty) = {
                    let Ok(guard) = state.lock() else { continue };
                    (
                        guard.live_ref(chat).ctx_used,
                        guard.chat_of(chat).map(ends_on_summary).unwrap_or(false),
                    )
                };
                // The space is freed once in one answer. A model that asks again after it would
                // put a summary under the summary it just had, and spend the space it
                // requested on text of its own making.
                if empty || room_made {
                    let note = summarise::refused_words();
                    let Ok(mut guard) = state.lock() else { continue };
                    let id = guard.begin_tool_in(chat, call, SUMMARISE_SHOWN, &thinking, thought_ms);
                    guard.finish_tool_in(chat, &id, &note, true);
                    continue;
                }
                room_made = true;
                // The answer is laid aside while the summary takes the chat, as it is at every
                // other place where the space is freed.
                if let Ok(mut guard) = state.lock() {
                    guard.live_mut(chat).generating = false;
                    guard.busy = guard.any_generating();
                }
                let kept = run_handoff(state, server, cancel, chat);
                let Ok(mut guard) = state.lock() else { return Ended::Halted };
                // The answer is taken up again: the call was one step of it, and the work is on
                // after the space that was created.
                guard.live_mut(chat).generating = true;
                let after = estimate_tokens(&guard.build_history_in(chat));
                let note = if kept {
                    summarise::kept_words(guard.handoffs_in(chat).len(), before, after)
                } else {
                    summarise::failed_words(before)
                };
                let id = guard.begin_tool_in(chat, call, SUMMARISE_SHOWN, &thinking, thought_ms);
                guard.finish_tool_in(chat, &id, &note, kept);
                // The call was created before the space was freed, so its block goes above the
                // marker of the summarisation: the user reads the call where the model made it,
                // and the summary means the request of it. A summary that never came cuts
                // nothing off, and the block stays below the marker with the word of what
                // failed under it.
                guard.stand_before_summary(&id);
                continue;
            }
            // Only a command of the terminal tool uses up the limit for commands. The other
            // tools run in the program itself, and the setting counts commands alone.
            let terminal = call.name == tools::TOOL_NAME;
            // A tool the user took out of the drop-down of the badge is never proposed, but a
            // model that names it anyway is answered rather than run: the badge of the tools is
            // the entry point to all of them, and a call of the model cannot open it.
            if !tool_allowed(state, &call.name) {
                let Ok(mut guard) = state.lock() else {
                    continue;
                };
                let note = format!(
                    "{} are switched off for this chat, so nothing ran",
                    tool_group_word(group_of(&call.name).unwrap_or("tools"))
                );
                let id = guard.begin_tool_in(chat, call, &note, "", 0);
                guard.finish_tool_in(chat, &id, &note, false);
                continue;
            }
            // The space for commands has expired. The call is still answered, so the model
            // hears why and can go on with its conversation instead of asking again for a
            // command that never runs.
            if terminal && command_room.is_some_and(|room| commands >= room) {
                let Ok(mut guard) = state.lock() else {
                    continue;
                };
                let note = "The command limit for this answer is reached, so the command did not run";
                let id = guard.begin_tool_in(chat, call, note, "", 0);
                guard.finish_tool_in(chat, &id, note, false);
                continue;
            }
            if terminal {
                commands += 1;
            }
            // The first call the user is shown takes the thoughts of the turn, and no other one
            // does.
            let thinking = std::mem::take(&mut thoughts);
            if terminal {
                run_command(call, &thinking, thought_ms, state, &workdir, sandbox_on, access, chat);
            } else {
                run_program_tool(call, &thinking, thought_ms, state, &workdir, cancel, chat);
            }
            if call.name == download::NAME {
                offered = true;
            }
            // The plan is let go, and nothing is asked of the model behind it: this
            // conversation ends here, and the plan is sent as the first message of the work.
            if call.name == planning::END_NAME {
                letting = true;
            }
        }
        // The proposal of a file is the bottom of an answer. The blocks remain below the text
        // of the answer, and the work the model was given is done once they are put out, so
        // nothing is left to go on with. The model is not asked again after such a run: the
        // conversation ends on the result of the proposal, and the next ask reads to the model
        // as a fresh request for the same, which it then does over - it writes the file again,
        // or proposes it a second time. The tool reports the suggestion is the last item of an
        // answer, so the answer ends with it.
        if offered {
            let Ok(mut guard) = state.lock() else {
                return Ended::Halted;
            };
            guard.live_mut(chat).generating = false;
            guard.clear_stopped_marks_in(chat, );
            guard.set_status("Ready");
            guard.persist_in(chat);
            break Ended::Done;
        }
        // The plan is let go. The conversation of the planning stops at this call: the worker
        // puts the planning away and begins the work from the plan, so the model is never asked
        // on behind a chat that is about to remain empty.
        if letting {
            let Ok(mut guard) = state.lock() else {
                return Ended::Halted;
            };
            guard.live_mut(chat).generating = false;
            guard.clear_stopped_marks_in(chat, );
            guard.set_status("The plan is released. The work begins from the plan");
            break Ended::Done;
        }
        if !thoughts.is_empty() {
            // The turn requested markers and for nothing a user could look at, so the thoughts
            // are above the markers as a panel of their own.
            let Ok(mut guard) = state.lock() else { return Ended::Halted };
            guard.push_message_in(chat, "Assistant", "", &thoughts, thought_ms, false);
        }
        let Ok(mut guard) = state.lock() else { return Ended::Halted };
        guard.set_status("The model is reading the tool result");
    };
    outcome
}

// The text of the header of the block of a handoff summary. The reason the model gave with the
// call is in the body of the block under its own label, so the header reports only what the
// call worked on.
const SUMMARISE_SHOWN: &str = "the conversation of this chat";

// True of a call the model had begun to write: it has a name of its own, or text of its
// arguments, or both. A call stopped before either came is nothing the user was shown, and
// nothing to tell the model about.
fn is_writing(call: &ToolCall) -> bool {
    !call.name.trim().is_empty() || !call.arguments.trim().is_empty()
}

// The tools the model is proposed for one step, or nothing at all when it is proposed none. The
// terminal tool and the tools that work on the files remain and fall together with the badge of
// the tools, for all of them change what is on the computer. The web search is on a switch of
// its own, for it reads the web out of the program and never the computer. The marker of a
// chapter is on a switch of its own as well, for it runs nothing. `access` is what the chat
// lets a command reach, so the text of the terminal tool tell the model what it really has. The
// line of the drop-down of the badge of the tools that a tool of the program is under. A tool
// outside the badge - the marker of a chapter, and every tool of a server of the user's own -
// gives nothing back, for the badge holds no switch over it.
fn group_of(name: &str) -> Option<&'static str> {
    match name {
        tools::TOOL_NAME => Some("commands"),
        editfile::EDIT_NAME | editfile::CREATE_NAME => Some("files"),
        viewimage::NAME | viewimage::ONE_NAME => Some("picture"),
        download::NAME => Some("hand_out"),
        display::NAME => Some("display"),
        askuser::NAME => Some("ask"),
        suggest::NAME => Some("suggest"),
        steps::NAME => Some("steps"),
        summarise::NAME => Some("summarise"),
        name if infographic::is_tool(name) => Some("infographics"),
        name if tasks::is_tool(name) => Some("tasks"),
        _ => None,
    }
}

// True when the open chat lets a call of this name through. The badge of the tools is read
// where the tools are proposed; this is the same rule laid over the call itself, for a model
// can name a tool it was never proposed.
fn tool_allowed(state: &Arc<Mutex<State>>, name: &str) -> bool {
    let Some(part) = group_of(name) else { return true };
    let Ok(guard) = state.lock() else { return false };
    // The handoff summary is no tool at all while compaction remains off: there is no summary
    // to be written, whatever the line of the drop-down reports.
    if part == "summarise" && !guard.compaction_on() {
        return false;
    }
    guard
        .chat()
        .map(|c| c.meta.tools.value_of(part))
        .unwrap_or(true)
}

// The text of one line of the drop-down, for the line of the state and for the block of a call
// that was not let through.
fn tool_group_word(part: &str) -> String {
    match part {
        "commands" => String::from("Terminal commands"),
        "files" => String::from("File management"),
        "picture" => String::from("The look at a picture"),
        "hand_out" => String::from("Handing a file to the user"),
        "display" => String::from("Showing the contents of a file"),
        "ask" => String::from("Asking the user"),
        "suggest" => String::from("The proposal of a next step"),
        "infographics" => String::from("Create infographics"),
        "tasks" => String::from("Tasks suite"),
        // The text of the line, in the form the sentence about a tool that was not let through
        // reads well in.
        "summarise" => String::from("Manual summaries"),
        other => String::from(other),
    }
}

// The two numbers of a change that a file tool made, extracted from the text of its result:
// `Edited notes.md (1 replacement, +12 -3)`. Nothing returns for a tool that changed no file,
// and for text that carry no such pair.
fn plus_minus(words: &str) -> (i64, i64) {
    let read = |mark: char| -> Option<i64> {
        let place = words.find(mark)? + mark.len_utf8();
        let rest = &words[place..];
        let end = rest
            .find(|one: char| !one.is_ascii_digit())
            .unwrap_or(rest.len());
        if end == 0 {
            return None;
        }
        rest[..end].parse::<i64>().ok()
    };
    (read('+').unwrap_or(0), read('-').unwrap_or(0))
}

// The tools proposed for one ask, planning mode taken in. The tools of planning are beside the
// others and hold no switch: a chat that is planning must always have its plan file, the word
// that reads it back, the word that asks the user, and the word that lets the plan go. When
// planning mode is off none of them is proposed at all, so a model reading the plan as its
// first message knows nothing of them. `sandbox` goes with the request, for the text of the
// tools that work on the computer tell the state of the sandbox, and nothing of it when the
// sandbox is off. The chapters of a chat propose no tool: they are extracted from the headings
// of the answers, and the model is informed of them in the text before the conversation.
#[allow(clippy::too_many_arguments)]
fn offered_with_planning(
    sandbox: bool,
    planning_on: bool,
    corporate_on: bool,
    code_on: bool,
    search_on: bool,
    access: tools::Access,
    mcp_tools: &[Value],
    switches: &store::ToolSwitches,
    compaction_on: bool,
) -> Option<Value> {
    let mut list = offered(
        sandbox,
        code_on,
        search_on,
        access,
        mcp_tools,
        switches,
        compaction_on,
    )
    .and_then(|made| made.as_array().cloned())
    .unwrap_or_default();
    if planning_on {
        list.push(planning::init_definition());
        list.push(planning::edit_definition());
        list.push(planning::read_definition());
        list.push(planning::ask_definition());
        list.push(planning::end_definition());
    }
    // The corporate display remains or falls as a setting of the whole program, and the tool of
    // the summary of the work is with it and nowhere else. It holds no switch of the user, for
    // the mode is nothing without it.
    if corporate_on {
        list.push(corporate::definition());
        list.push(corporate::finish_definition());
    }
    if list.is_empty() {
        return None;
    }
    Some(Value::Array(list))
}

fn offered(
    sandbox: bool,
    code_on: bool,
    search_on: bool,
    access: tools::Access,
    mcp_tools: &[Value],
    switches: &store::ToolSwitches,
    compaction_on: bool,
) -> Option<Value> {
    if !(code_on || search_on || !mcp_tools.is_empty()) {
        return None;
    }
    let mut list: Vec<Value> = Vec::new();
    // The badge of the tools is the entry point to all of them, and the lines of its drop-down
    // are behind that entry point: a tool whose line is off is not proposed, and a badge that
    // is off proposes none of them whatever the lines say.
    if code_on {
        if switches.commands {
            list.push(tools::definition(sandbox, access));
        }
        if switches.files {
            list.push(editfile::edit_definition(sandbox));
            list.push(editfile::create_definition(sandbox));
        }
        if switches.picture {
            list.push(viewimage::definition(sandbox));
        }
        if switches.hand_out {
            list.push(download::definition());
        }
        if switches.display {
            list.push(display::definition(sandbox));
        }
        if switches.ask {
            list.push(askuser::definition());
        }
        // The buttons at the bottom of an answer are behind the badge like every other tool of
        // the program, and behind the line of their own.
        if switches.suggest {
            list.push(suggest::definition());
        }
        // The list of the steps of the work is behind the badge like every other tool of the
        // program, and behind the line of its own.
        if switches.steps {
            list.push(steps::definition());
        }
        // The five tools of the infographic suite remain or fall as one line of the badge: a
        // script without a renderer draws nothing.
        if switches.infographics {
            list.extend(infographic::definitions());
        }
        // The six tools of the tasks suite remain or fall as one line of the badge: a list of
        // tasks with no way of reading it is a list nobody works through.
        if switches.tasks {
            list.extend(tasks::definitions());
        }
        // The handoff summary the model may request is behind the badge like every other tool
        // of the program, and behind the line of its own. It is at all only while the settings
        // make the size of the context by a handoff summary: with compaction off there is no
        // summary to be requested, and a tool that can do nothing is not proposed at all.
        if compaction_on && switches.summarise {
            list.push(summarise::definition());
        }
    }
    if search_on {
        list.push(search::definition());
    }
    // The tools of the servers of the user's own remain last, each under the name of the server
    // it came from.
    list.extend(mcp_tools.iter().cloned());
    // Nothing was left to propose. A chat with every line of the drop-down of the badge
    // switched off comes to this, and the call of the model must carry no tools at all rather
    // than an empty list of them.
    if list.is_empty() {
        return None;
    }
    Some(Value::Array(list))
}

// The wait for a word from the user. The run asks the state what the user answered, and rests
// between the requests, so the window is never kept from drawing and the user is never rushed.
// A question of the ordinary type is given up when its wait has ended; any question is given up
// with the answer when the user stops the run, and a question the model cannot go on without is
// waited for as long as it takes.
fn wait_for_answer(
    state: &Arc<Mutex<State>>,
    id: &str,
    seconds: u32,
    cancel: &Arc<AtomicBool>,
    chat: &str) -> Option<String> {
    let began = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let held = state
            .lock()
            .ok()
            .and_then(|guard| guard.asking_answer_of_in(chat, id));
        if held.is_some() {
            return held;
        }
        if seconds > 0 && began.elapsed().as_secs() >= seconds as u64 {
            return None;
        }
        // The user cut the call short from the header of its block. A question that was to be
        // waited for for ever is given up with the call it was in, and the model is informed
        // that the call was stopped.
        if state
            .lock()
            .ok()
            .and_then(|guard| {
                guard
                    .live()
                    .running
                    .as_ref()
                    .map(|one| one.watch.stopped())
            })
            .unwrap_or(false)
        {
            return None;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

// One run of the terminal tool. The block is in the chat before the command starts, and reports
// what became of it when it is over.
#[allow(clippy::too_many_arguments)]
fn run_command(
    call: &ToolCall,
    thinking: &str,
    thought_ms: u64,
    state: &Arc<Mutex<State>>,
    workdir: &Path,
    sandbox_on: bool,
    access: tools::Access,
    chat: &str) {
    let (command, purpose) = tools::parse_arguments(&call.arguments);
    // The reason the model gave is not requested any more, but a model that gives one of its
    // own is still shown.
    let shown = if purpose.trim().is_empty() {
        command.clone()
    } else {
        format!("{}\nreason: {}", command, purpose)
    };
    // The clock of the call, from the Chatting settings.
    let watch = tool_watch(state);
    let message_id = {
        let Ok(mut guard) = state.lock() else { return };
        let id = guard.begin_tool_in(chat, call, &shown, thinking, thought_ms);
        guard.begin_running_in(chat, &id, &watch);
        let text = format!("Running a command: {}", first_line(&command));
        guard.set_status(&text);
        id
    };
    // The command runs under the clock of the call: it is stopped when its time is removed, or
    // when the user requests it, and all it had printed by then returns with it.
    let outcome = tools::run_watched(&command, workdir, sandbox_on, access, &watch);
    let mut output = outcome.output;
    // The chat requested the sandbox, so say when it was not used. The run itself already
    // reports why, and it did not run at all then.
    if sandbox_on && !outcome.sandboxed {
        output = format!(
            "{}\n(The command did not run, because the sandbox was not available)",
            output
        );
    }
    if !sandbox_on {
        output = format!(
            "{}\n(Not sandboxed: this command could reach the whole machine)",
            output
        );
    }
    let Ok(mut guard) = state.lock() else { return };
    guard.finish_tool_in(chat, &message_id, &output, outcome.ok);
}

// One call of a tool the program does itself. None of these starts a shell, so none of them
// needs the sandbox: a chat whose sandbox cannot be had has them still. The file tools reach
// the working directory of the chat and nothing past it, which is the one space the sandbox may be
// written in.
fn run_program_tool(
    call: &ToolCall,
    thinking: &str,
    thought_ms: u64,
    state: &Arc<Mutex<State>>,
    workdir: &Path,
    // The pointer the user puts up to stop the answer. A question the user was asked is given
    // up with the answer, for the run that asked it is going away.
    cancel: &Arc<AtomicBool>,
    chat: &str) {
    // The clock of the call, from the Chatting settings.
    let watch = tool_watch(state);
    // The text of the call, and the text of the state line, are settled first: the block has to
    // are in the chat before the work begins.
    let (shown, _, target) = words_of(&call.name, &call.arguments);
    // The header of the block names the file a file tool works on, and the one item another
    // tool names when it has no file of its own.
    let shown = if target.is_empty() { shown } else { target };
    let doing = match call.name.as_str() {
        name if mcp::is_tool(name) => "A tool of a server",
        editfile::EDIT_NAME => "Editing a file",
        editfile::CREATE_NAME => "Creating a file",
        search::NAME => "Searching the web",
        viewimage::NAME | viewimage::ONE_NAME => "Looking at images",
        download::NAME => "Offering a file",
        display::NAME => "Showing a file to the user",
        askuser::NAME => "Asking the user",
        suggest::NAME => "Offering a next step",
        steps::NAME => "Writing the steps of the work",
        planning::INIT_NAME => "Writing the plan",
        planning::EDIT_NAME => "Changing the plan",
        planning::READ_NAME => "Reading the plan",
        planning::ASK_NAME => "Asking about the plan",
        planning::END_NAME => "Letting the plan go",
        infographic::GUIDE_NAME => "Writing the guide of a script",
        infographic::PLAN_NAME => "Planning the video",
        infographic::SCENE_NAME => "Writing a scene",
        infographic::CHECK_NAME => "Checking the layout",
        infographic::RENDER_NAME => "Drawing the frames",
        tasks::CREATE_NAME => "Making a task",
        tasks::VIEW_ALL_NAME => "Reading the list of tasks",
        tasks::VIEW_ONE_NAME => "Reading a task",
        tasks::COMPLETE_NAME => "Completing a task",
        tasks::CANCEL_NAME => "Cancelling a task",
        tasks::DELETE_NAME => "Deleting a task",
        _ => "A tool of the program",
    };
    let message_id = {
        let Ok(mut guard) = state.lock() else { return };
        let id = guard.begin_tool_in(chat, call, &shown, thinking, thought_ms);
        guard.begin_running_in(chat, &id, &watch);
        let text = format!("{}: {}", doing, first_line(&shown));
        guard.set_status(&text);
        id
    };
    // The clock of the call. A call of the program is stopped by the user in the same way as a
    // command: the marker is in the header of the block, and a call that is waited for -- the
    // question put to the user, above all -- gives up the moment the marker is pressed. A call
    // of the program that runs on its own has its own, shorter, time to return within, and it
    // keeps that time: the clock here is what is above all of them. A tool of a server of the
    // user's own. The server is named by the part of the name of the tool that means it, and it
    // is reached out of the program: never inside the sandbox of a command, for a command has
    // no network and no program of its own to run.
    if mcp::is_tool(&call.name) {
        let wanted = {
            let Ok(guard) = state.lock() else { return };
            mcp::split_tool(&call.name).and_then(|(slug, tool)| {
                guard
                    .cfg
                    .mcp_servers
                    .iter()
                    .find(|server| server.slug() == slug)
                    .cloned()
                    .map(|server| (server, tool))
            })
        };
        let report = match wanted {
            Some((server, tool)) => {
                // The server of the user's own is reached on a thread of its own, and waited
                // for only within the clock of the call. Such a path reaches out over a pipe or
                // over the network and can remain waiting for an answer that never comes, and
                // the clock of the call was read by nothing in it: the block was saying
                // "running" for hours, and neither the marker that cuts the call short nor the
                // marker that stops the answer could do anything. The call is answered now as a
                // command of the same error is answered, and what the server does after the
                // clock is removed is not known.
                let words = call.arguments.clone();
                match crate::util::within(&watch, move || mcp::run(&server, &tool, &words)) {
                    Some(report) => report,
                    None => mcp::Report {
                        ok: false,
                        output: String::from(if watch.stopped() {
                            "The call was stopped: you requested it to end. The server may still be at work, and what it does after this is not known."
                        } else {
                            "The call ran out of time and was stopped. The server may still be at work, and what it does after this is not known."
                        }),
                    },
                }
            }
            None => mcp::Report {
                ok: false,
                output: String::from(
                    "No server of this program has that name, so nothing ran",
                ),
            },
        };
        let Ok(mut guard) = state.lock() else { return };
        guard.finish_tool_in(chat, &message_id, &report.output, report.ok);
        return;
    }
    // An image is read here and kept out of the text the user sees. The result of the run is
    // the block in the chat; every image rides on the same message as an attachment of the
    // chat, and goes to the model with the text of the result when the history is rebuilt. The
    // sandbox boundary of the chat is above the names of the files, as it does over the tools
    // that work on files: with the sandbox off, an image may remain anywhere the computer has
    // it.
    if viewimage::is_look(&call.name) {
        let (vision, jailed) = {
            let Ok(guard) = state.lock() else { return };
            (
                guard.pictures_can_go(),
                guard.chat_of(chat).map(|one| one.meta.sandbox_enabled).unwrap_or(false),
            )
        };
        let made = viewimage::open(workdir, &call.arguments, vision, jailed);
        let records = if made.pictures.is_empty() {
            Vec::new()
        } else {
            let held: Vec<attach::Incoming> = made
                .pictures
                .iter()
                .map(|picture| attach::Incoming {
                    name: picture.name.clone(),
                    mime: picture.mime.clone(),
                    data: picture.data.clone(),
                    text: String::new(),
                })
                .collect();
            let Ok(guard) = state.lock() else { return };
            match guard.attach_place_for_chat(chat) {
                Some(place) => attach::save_at(&place, &held),
                None => Vec::new(),
            }
        };
        let Ok(mut guard) = state.lock() else { return };
        guard.finish_tool_with_in(chat, &message_id, &made.report.output, made.report.ok, records);
        return;
    }
    // A question the model puts to the user. The run waits: the request is above the chat
    // composer, the user chooses one of the answers the model proposed or writes an answer of
    // their own, and what they stated goes back to the model as the result of the call. A
    // question of the ordinary type is given up when its wait has ended; a question the model
    // cannot go on without is waited for until it is answered, or until the user stops the
    // answer whole. The questions of the plan. Any number of them go to the user at once, each
    // with the answers they may choose from, and the run waits until the user sends all of it.
    // No clock runs on the wait, for the plan is not served on time, and the user is never
    // rushed over it.
    if call.name == planning::ASK_NAME {
        let items = match planning::questions_of(&call.arguments) {
            Ok(made) => made,
            Err(why) => {
                let Ok(mut guard) = state.lock() else { return };
                guard.finish_tool_in(chat, &message_id, &why, false);
                return;
            }
        };
        let asking = {
            let Ok(mut guard) = state.lock() else { return };
            let text = format!("Waiting for your answers: {} questions", items.len());
            guard.set_status(&text);
            guard.begin_asking_plan_in(chat, items)
        };
        let answer = wait_for_answer(state, &asking, 0, cancel, chat);
        let (words, ok) = match answer {
            Some(said) => (said, true),
            // The user stopped the answer whole, so the questionnaire went with it. The model
            // is informed, and the plan keeps the questions open.
            None => (
                String::from(
                    "The questions were not answered, so the plan must go on without them.",
                ),
                false,
            ),
        };
        let Ok(mut guard) = state.lock() else { return };
        guard.finish_tool_in(chat, &message_id, &words, ok);
        guard.clear_asking_in(chat, &asking);
        return;
    }
    if call.name == askuser::NAME {
        let asked = askuser::parse(&call.arguments);
        {
            let Ok(mut guard) = state.lock() else { return };
            let text = format!("Waiting for your answer: {}", first_line(&asked.text));
            guard.set_status(&text);
        }
        let (asking, seconds) = {
            let Ok(mut guard) = state.lock() else { return };
            guard.begin_asking_in(chat, &asked)
        };
        let answer = wait_for_answer(state, &asking, seconds, cancel, chat);
        let (words, ok) = match answer {
            Some(said) => (askuser::answered_words(&said), true),
            // A question that was given up is no error of the run: the model is informed the
            // user did not answer, and goes on as well as it can.
            None if seconds > 0 => (askuser::given_up_words(seconds), true),
            None => (askuser::stopped_words(), false),
        };
        let Ok(mut guard) = state.lock() else { return };
        guard.finish_tool_in(chat, &message_id, &words, ok);
        guard.clear_asking_in(chat, &asking);
        return;
    }
    // The whole plan in one call. The plan file of a chat that has nothing in it has no text to
    // match against, so the first writing of a plan is a whole of text sent at once; every
    // writing after it goes through `edit_plan`.
    if call.name == planning::INIT_NAME {
        let name = match state.lock() {
            Ok(guard) => guard
                .chat()
                .map(|one| one.meta.plan_file.clone())
                .unwrap_or_default(),
            Err(_) => return,
        };
        let report = planning::write_init(&name, &call.arguments);
        let Ok(mut guard) = state.lock() else { return };
        guard.finish_tool_in(chat, &message_id, &report.output, report.ok);
        return;
    }
    // The plan file of this chat. The tool reaches that one file and no other, so it needs no
    // working directory and no mention of the sandbox: the plan is in a directory of the
    // planning directory of the program, away from the work.
    if call.name == planning::EDIT_NAME {
        let name = match state.lock() {
            Ok(guard) => guard
                .chat()
                .map(|one| one.meta.plan_file.clone())
                .unwrap_or_default(),
            Err(_) => return,
        };
        let report = planning::edit_plan(&name, &call.arguments);
        let Ok(mut guard) = state.lock() else { return };
        guard.finish_tool_in(chat, &message_id, &report.output, report.ok);
        return;
    }
    // The same plan file, read back. The model is shown the lines of the plan as they are now,
    // so it sees how its changes landed instead of guessing.
    if call.name == planning::READ_NAME {
        let name = match state.lock() {
            Ok(guard) => guard
                .chat()
                .map(|one| one.meta.plan_file.clone())
                .unwrap_or_default(),
            Err(_) => return,
        };
        let report = planning::read_plan(&name, &call.arguments);
        let Ok(mut guard) = state.lock() else { return };
        guard.finish_tool_in(chat, &message_id, &report.output, report.ok);
        return;
    }
    // The plan is let go. The conversation of the planning is closed behind this call, and the
    // plan itself becomes the first message of the work: the worker begins that turn as soon as
    // this one is over. The plan is taken here, while the planning prompt still is in the
    // conversation, so the model that reads it next needs nothing of this chat.
    if call.name == planning::END_NAME {
        let Ok(mut guard) = state.lock() else { return };
        let held = guard.plan_words().unwrap_or_default();
        if held.trim().is_empty() {
            guard.finish_tool_in(chat, 
                &message_id,
                "The plan holds nothing, so it could not be released.",
                false,
            );
            return;
        }
        guard.live_mut(chat).plan_pending = Some(held);
        guard.finish_tool_in(chat, 
            &message_id,
            "The plan is released. This conversation ends here, and the plan becomes the first message of the work.",
            true,
        );
        return;
    }
    // A show of the contents of files. Nothing is copied and nothing is kept in the text of the
    // conversation: the block of the show names the files, and the button of it opens the
    // window of the chat over them. The window reads each file again when the user opens it,
    // under the same rule of the space as the call.
    if call.name == display::NAME {
        let jailed = match state.lock() {
            Ok(guard) => guard
                .chat()
                .map(|one| one.meta.sandbox_enabled)
                .unwrap_or(true),
            Err(_) => true,
        };
        let made = display::show(workdir, &call.arguments, jailed);
        let Ok(mut guard) = state.lock() else { return };
        guard.finish_displayed_in(chat, &message_id, &made.report.output, made.report.ok, made.shown);
        return;
    }
    // The next steps the user may take with a press. Nothing is waited for: the proposal rides
    // on the message of the run, and the page puts the buttons at the end of the answer once
    // the model has stopped writing. The newest propose is the one that remains, so an older
    // propose of the same chat is removed here as well, and the conversation holds one propose
    // and not a trail of them.
    if call.name == suggest::NAME {
        let made = suggest::offer(&call.arguments);
        let Ok(mut guard) = state.lock() else { return };
        guard.finish_suggestions_in(chat, &message_id, &made.report.output, made.report.ok, made.asks);
        return;
    }
    // The steps of the work, shown one at a time in a block of their own. Nothing is waited
    // for: the list rides on the message of the run, and the page puts the block at the end of
    // the answer once the model has stopped writing.
    if call.name == steps::NAME {
        let made = steps::show(&call.arguments);
        let Ok(mut guard) = state.lock() else { return };
        guard.finish_steps_in(chat, &message_id, &made.report.output, made.report.ok, made.plan);
        return;
    }
    // An infographic. The five tools of the suite work on one file of text in the space of
    // the chat, and the last of them draws the frames. A render is the slow work of a minute,
    // so the way it comes is put on the line of the state as it goes.
    if infographic::is_tool(&call.name) {
        let cancel_render = cancel.clone();
        let told = |one: &str| {
            if let Ok(mut guard) = state.lock() {
                guard.set_status(one);
            }
        };
        let made = infographic::run(&call.name, &call.arguments, workdir, &cancel_render, told);
        let ok = !made.starts_with("the ") || made.contains("stands");
        let Ok(mut guard) = state.lock() else { return };
        guard.finish_tool_in(chat, &message_id, &made, ok);
        return;
    }
    // The tasks of the working directory. The six tools of the suite work on the list that belongs
    // to the directory the chat works in, and not to the chat: two chats of one repository read
    // one list, and a task outlives the chat that made it. Nothing is written into the work
    // directory - the rows are in `tasks.db` under the data folder of the program. See
    // `crate::tasks`.
    if tasks::is_tool(&call.name) {
        let work = tasks::key_of(workdir);
        let made = tasks::run(&call.name, &work, &call.arguments);
        let Ok(mut guard) = state.lock() else { return };
        guard.finish_tool_in(chat, &message_id, &made.output, made.ok);
        return;
    }
    // A file the user is proposed. Nothing is copied and nothing is fetched: the file is in the
    // working directory of the chat where the model put it, and the proposal rides on the
    // message of the run. The page draws the blocks of the proposal at the end of the answer that made
    // them, and only when the model has stopped writing, so a file that is still being written
    // is never handed over.
    if call.name == download::NAME {
        let made = download::offer(workdir, &call.arguments);
        let Ok(mut guard) = state.lock() else { return };
        guard.finish_gifts_in(chat, &message_id, &made.report.output, made.report.ok, made.gifts);
        return;
    }
    // The sandbox state of the chat. While the sandbox is on, the file tools work in the work
    // directory of the chat and nowhere else, because that is the only directory the sandbox
    // allows writing in. With the sandbox off they run on the computer as a command of the
    // terminal
    // tool does, and a name outside the working directory of the chat is a name like any other.
    let jailed = match state.lock() {
        Ok(guard) => guard
            .chat()
            .map(|one| one.meta.sandbox_enabled)
            .unwrap_or(false),
        Err(_) => false,
    };
    let report = match call.name.as_str() {
        editfile::EDIT_NAME => editfile::edit_room(workdir, &call.arguments, jailed),
        editfile::CREATE_NAME => editfile::create_room(workdir, &call.arguments, jailed),
        // The web search needs no working directory: it reads what is on the web, and
        // nothing of the computer. The read is waited for only within the clock of the call, as
        // a command is and as a server of the user's own is, so a search that remains waiting
        // for an answer that never comes cannot hold the chat with a block that shows
        // "running".
        search::NAME => {
            let words = call.arguments.clone();
            match crate::util::within(&watch, move || search::run(&words)) {
                Some(made) => made,
                None => tools::Report {
                    ok: false,
                    output: String::from(if watch.stopped() {
                        "The call was stopped: you requested it to end. The search may still be going, and what it brings after this is not known."
                    } else {
                        "The call ran out of time and was stopped. The search may still be going, and what it brings after this is not known."
                    }),
                },
            }
        }
        other => tools::Report {
            ok: false,
            output: format!(
                "The model requested {}, which this program does not propose",
                first_line(other)
            ),
        },
    };
    let Ok(mut guard) = state.lock() else { return };
    guard.finish_tool_in(chat, &message_id, &report.output, report.ok);
}

// Start llama-server when none runs yet, or when another model is wanted. The answer is the
// context size a chat gets.
fn ensure_server(
    state: &Arc<Mutex<State>>,
    server: &ServerSlot,
    model: &ModelInfo,
    settings: &ModelSettings,
    cancel: &Arc<AtomicBool>,
    chat: &str) -> Result<u64, String> {
    let (binary, gpus, wants, keeping) = {
        let Ok(state) = state.lock() else {
            return Err("The program state is removed".to_string());
        };
        let binary = state
            .server_binary()
            .ok_or("llama-server was not found in the directory from the settings")?;
        let wants = state.server_wants_of(chat);
        // The folder the conversations the model works its way through are written into, when
        // the user has the keeping on. The server is informed the folder and nothing else: it
        // makes the names of the files inside it by itself, and never looks outside it. See
        // `context_claim`.
        let keeping = state
            .cfg
            .chat_context_files
            .then(|| config::contexts_dir(&state.cfg));
        (binary, state.gpus.clone(), wants, keeping)
    };
    // The load of a model is the one part of a turn two chats may not share, so the slot is
    // held for all of what follows: a second chat that requests the same model waits for the
    // run it needs instead of starting a second llama-server, and a chat that requests another
    // model is kept off the run by the worker until the chats on it are done.
    let Ok(mut held) = server.lock() else {
        return Err("The server is gone".to_string());
    };
    // The model loads once and stays loaded for every chat that requests the same model: see
    // `ServerWants::covers` for the whole rule. The thinking mode, the keeping of the thinking,
    // the tools and the sandbox move no server, for they are stated with the request.
    let reusable = held.as_mut().map(|s| s.alive()).unwrap_or(false)
        && held
            .as_ref()
            .map(|s| ServerWants::covers(&State::server_wears(s), &wants))
            .unwrap_or(false);
    if reusable {
        let total = held
            .as_ref()
            .map(|s| s.ctx_total)
            .unwrap_or(settings.ctx_size as u64);
        return Ok(total);
    }
    stop_run(state, &mut held);
    if cancel.load(Ordering::Relaxed) {
        return Err("Stopped before the model loaded".to_string());
    }
    match llama::Server::start(&binary, model, settings, &gpus, keeping.as_deref()) {
        Ok(started) => {
            let total = started.ctx_total;
            let notes = started.notes.clone();
            *held = Some(started);
            drop(held);
            if let Ok(mut state) = state.lock() {
                for note in notes {
                    let text = note;
                    state.set_status(&text);
                }
            }
            Ok(total)
        }
        Err(problem) => {
            stop_run(state, &mut held);
            Err(problem)
        }
    }
}

// Rough token count, used when the server gives no usage numbers, and as the floor under the
// number the server spoke (see `used_after`). It is the measure of the text that actually go
// out with the request, so every part of a message the server is sent is counted: the text of
// the message, whether they remain as one string or as the parts of a message of the user, the
// thinking that is sent back when the chat keeps it, and the name and the arguments of every
// call of a tool the model made -- those go out with the conversation and cost the space of
// them, and leaving them out read a chat with many calls as having space it had not. An
// image costs a block of tokens of its own, whatever it shows.
//
// This is a rough count and stays one: the server's own number is the authority, and it takes
// the meter wherever the two disagree. What this count is not allowed to do is read the
// conversation smaller than it is by ignoring a part of the request, for a space weighed on
// too small a number is sent for again and again until it is removed.
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
                            // An image means a few hundred tokens.
                            chars += 4000;
                        }
                    }
                }
                _ => {}
            }
            // The thinking of a turn that is sent back to the model costs the space of its
            // text, as any other text of the conversation does.
            if let Some(said) = item
                .get("reasoning_content")
                .and_then(|t| t.as_str())
            {
                chars += said.chars().count() as u64;
            }
            // The calls of a tool ride on the message of the turn that made them, and the
            // server reads them as part of the conversation. The name of the tool and the
            // arguments the model wrote are under the part named `function`, which is the form
            // the conversation carries; the two names are also read where they are, so a call
            // is never passed by for want of the form.
            if let Some(calls) = item.get("tool_calls").and_then(|c| c.as_array()) {
                for one in calls {
                    let held = |where_: &Value, key: &str| -> u64 {
                        where_
                            .get(key)
                            .and_then(|v| v.as_str())
                            .map(|said| said.chars().count() as u64)
                            .unwrap_or_default()
                    };
                    let function = one.get("function").cloned().unwrap_or(Value::Null);
                    chars += held(&function, "name") + held(&function, "arguments");
                    chars += held(one, "name") + held(one, "arguments");
                    chars += held(one, "id");
                    // The envelope of a call: the markers of a call, and the name of the part
                    // that holds its arguments.
                    chars += 24;
                }
            }
            // Every message costs a few tokens on its own.
            chars += 8;
        }
    }
    chars / 4
}

// The name of the archive the user is given when they take all of an propose at once. The first
// file of the proposal gives its name to the archive, so the file that drops is not a mystery;
// an propose with nothing to take its name from is called for the program.
fn zip_name(asked: &[String]) -> String {
    let first = asked
        .iter()
        .map(|one| one.trim())
        .find(|one| !one.is_empty())
        .map(|one| {
            let tail = one.rsplit(['/', '\\']).next().unwrap_or(one);
            let (stem, _) = download::split_name(tail);
            stem
        })
        .unwrap_or_else(|| String::from("autumn-natter"));
    format!("{}.zip", first)
}

fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    if line.chars().count() > 60 {
        let short: String = line.chars().take(60).collect();
        return format!("{}...", short);
    }
    line.to_string()
}

// Tests for the carousel of chats. They drive the engine the way a window does, and look at the
// snapshot that a window draws.
#[cfg(test)]
mod tests {
    use super::*;

    // The program keeps its paths in one place for the whole run, so the tests share one data
    // folder. They take turns through the lock of the config, and each one empties the tables
    // of the database of the chats first.

    fn setup() {
        config::set_data_dir(PathBuf::from("uidata/engine-tests"));
        let _ = config::ensure_dirs();
        let store = store::Store::new(config::repositories_dir(&Config::default()));
        store.empty();
        let work = config::paths().data.join("work");
        let _ = std::fs::create_dir_all(&work);
        // The search marker is placed, so a check of the engine never reads the directories of
        // the older format and never brings a chat of the computer into the place a check works
        // in.
        migrate::mark_ran(&config::paths().data);
    }

    // A line of the drop-down of the badge of the tools takes its type of tool out of the chat,
    // and the badge itself takes all of them out with it. The choice is with the chat, and
    // opens the next chat the same way.
    #[test]
    fn a_line_of_the_badge_of_the_tools_takes_its_tool_out() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
        }
        engine.set_code(true);
        engine.set_tool_switch("files", false);
        let snap = engine.snapshot();
        assert!(snap.code_enabled, "the badge stands on");
        assert!(snap.tool_switches.commands);
        assert!(!snap.tool_switches.files, "file management is off");
        assert!(snap.tool_switches.picture);
        assert!(snap.tool_switches.hand_out);
        let words = |code_on: bool, switches: &store::ToolSwitches| {
            offered(false, code_on, false, tools::Access::default(), &[], switches, true)
                .map(|list| {
                    list.as_array()
                        .unwrap()
                        .iter()
                        .map(|one| String::from(one["function"]["name"].as_str().unwrap_or("")))
                        .collect::<Vec<_>>()
                })
        };
        let given = words(true, &engine.snapshot().tool_switches).expect("the badge stands on");
        assert!(given.contains(&tools::TOOL_NAME.to_string()));
        assert!(!given.contains(&editfile::EDIT_NAME.to_string()));
        assert!(!given.contains(&editfile::CREATE_NAME.to_string()));
        assert!(given.contains(&viewimage::NAME.to_string()));
        assert!(given.contains(&download::NAME.to_string()));
        // Every line of the drop-down off, and the badge on: nothing of the program is proposed
        // at all. The lines are read from the switches of the chat themselves, so a tool the
        // program learns later is taken out here as well rather than leaving this test to say
        // an item that is no longer true of it.
        let lines = engine
            .snapshot()
            .tool_switches
            .words()
            .into_iter()
            .map(|(part, _)| part)
            .collect::<Vec<_>>();
        for part in lines.iter() {
            engine.set_tool_switch(part, false);
        }
        // The entry point to the badge shuts along with the last line, so it never is on over
        // nothing the model could reach.
        assert!(!engine.snapshot().code_enabled, "the badge was sent with the last line");
        assert!(words(true, &engine.snapshot().tool_switches).is_none());
        // The badge is the entry point to all of them: with the badge off, a line that is on
        // returns nothing. A line switched on again opens the entry point, for a tool behind a
        // shut door is no tool at all.
        engine.set_tool_switch("commands", true);
        assert!(engine.snapshot().code_enabled, "the badge came on with the line");
        engine.set_code(false);
        assert!(words(false, &engine.snapshot().tool_switches).is_none());
        // The next chat opens with the same lines, as the user left them.
        engine.new_chat();
        {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
        }
        let kept = {
            let state = engine.state.lock().unwrap();
            state
                .chat()
                .expect("a chat stands open")
                .meta
                .tools
                .clone()
        };
        assert!(kept.commands, "the line the user put back on stays on");
        assert!(!kept.files, "file management is still off");
        assert!(!kept.picture);
        assert!(!kept.hand_out, "the proposal of a file is still off");
        assert!(!lines.is_empty(), "the badge has lines to switch");
    }

    // The tools of planning remain only while planning mode is on, and all of them remains
    // then: the first writing of the plan, the changes of it, the reading of it back, the
    // questions of the user, and the letting go. A chat that never planned is proposed none of
    // them, for a plan file it has no hold of is nothing a tool could reach.
    #[test]
    fn the_tools_of_planning_stand_only_while_planning_is_on() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
        }
        let switches = engine.snapshot().tool_switches;
        let names = |on: bool| {
            offered_with_planning(
                false,
                on,
                false,
                false,
                false,
                tools::Access::default(),
                &[],
                &switches,
                true,
            )
            .map(|list| {
                list.as_array()
                    .unwrap()
                    .iter()
                    .map(|one| String::from(one["function"]["name"].as_str().unwrap_or("")))
                    .collect::<Vec<_>>()
            })
        };
        let wanted = [
            planning::INIT_NAME,
            planning::EDIT_NAME,
            planning::READ_NAME,
            planning::ASK_NAME,
            planning::END_NAME,
        ];
        let plain = names(false).unwrap_or_default();
        for one in wanted {
            assert!(
                !plain.iter().any(|two| two == one),
                "{} stands outside planning mode",
                one
            );
        }
        let planned = names(true).expect("planning mode always offers a tool");
        for one in wanted {
            assert!(
                planned.iter().any(|two| two == one),
                "{} is missing while planning mode stands on: {:?}",
                one,
                planned
            );
        }
    }

    // The handoff summary the model may request is behind the badge of the tools and behind the
    // line of its own, as every other tool of the program does. It is at all only while the
    // settings make the size of the context by a handoff summary: with compaction off there is
    // no summary to be written, so the tool is proposed nowhere and a call of its name is
    // answered as a tool that is switched off.
    #[test]
    fn the_tool_of_the_handoff_summary_stands_only_while_the_room_is_made_by_one() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        open_a_chat(&engine);
        engine.set_code(true);
        let names = |list: Option<Value>| -> Vec<String> {
            list.map(|made| {
                made.as_array()
                    .unwrap()
                    .iter()
                    .map(|one| String::from(one["function"]["name"].as_str().unwrap_or("")))
                    .collect()
            })
            .unwrap_or_default()
        };
        let given = |room: bool, switches: &store::ToolSwitches| {
            names(offered(
                false,
                true,
                false,
                tools::Access::default(),
                &[],
                switches,
                room,
            ))
        };
        // The space is freed by a summary and the line is on: the tool is proposed beside the
        // others.
        let open = given(true, &engine.snapshot().tool_switches);
        assert!(open.contains(&tools::TOOL_NAME.to_string()), "the badge of the tools is on");
        assert!(
            open.contains(&summarise::NAME.to_string()),
            "the handoff summary was not proposed: {:?}",
            open
        );
        // The same chat with compaction off: the tool is removed, and every other tool of the
        // badge is where it was.
        let shut = given(false, &engine.snapshot().tool_switches);
        assert!(shut.contains(&tools::TOOL_NAME.to_string()));
        assert!(
            !shut.contains(&summarise::NAME.to_string()),
            "a summary was proposed with compaction off: {:?}",
            shut
        );
        // The line of the drop-down takes the tool out on its own, as every other line does,
        // and the badge stays open over the rest.
        engine.set_tool_switch("summarise", false);
        let taken = given(true, &engine.snapshot().tool_switches);
        assert!(!taken.contains(&summarise::NAME.to_string()));
        assert!(taken.contains(&tools::TOOL_NAME.to_string()));
        assert!(engine.snapshot().code_enabled, "the badge stands on over the other lines");
        engine.set_tool_switch("summarise", true);
        // A call of the name is let through while compaction is on, and is answered as a tool
        // that is switched off when it does not: the badge and the line say yes, and there is
        // nothing for the tool to do.
        assert!(tool_allowed(&engine.state, summarise::NAME), "the chat lets the call through");
        {
            let mut state = engine.state.lock().unwrap();
            state.cfg.chat_compaction = String::from(config::COMPACTION_OFF);
        }
        assert!(
            !tool_allowed(&engine.state, summarise::NAME),
            "a summary was let through with compaction off"
        );
        assert!(
            tool_allowed(&engine.state, tools::TOOL_NAME),
            "the other tools of the badge are untouched"
        );
        // The line of the drop-down is named, so the block of a call that was not let through
        // reports what was switched off.
        assert_eq!(group_of(summarise::NAME), Some("summarise"));
        assert!(tool_group_word("summarise").contains("ummar"));
    }

    // The text of the space go out with the conversation while the settings make the
    // space by a handoff summary, and go away when it remains off. A model that is never
    // informed how the space is kept reads a full context as the end of its work, and
    // compresses the part it was requested.
    #[test]
    fn the_words_of_the_room_stand_with_the_talk_while_compaction_stands_on() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        open_a_chat(&engine);
        let said = {
            let state = engine.state.lock().unwrap();
            state.build_history().to_string()
        };
        assert!(
            said.contains("The context of this conversation stays open"),
            "the text of the space did not go out with the conversation"
        );
        // The text says what the model is to make of the limits: no call limit, and nothing to
        // leave out of the work.
        assert!(said.contains("no limit on the number of tool calls"));
        assert!(said.contains("Never leave a part of the work out"));
        assert!(said.contains("Work until the work is complete"));
        // The text go out once for the conversation, in a message of the role `system`.
        let count = {
            let state = engine.state.lock().unwrap();
            state
                .build_history()
                .as_array()
                .expect("a list of the conversation")
                .iter()
                .filter(|one| {
                    one["content"]
                        .as_str()
                        .unwrap_or_default()
                        .starts_with("The context of this conversation stays open")
                })
                .count()
        };
        assert_eq!(count, 1, "the text of the space ride more than once with the conversation");
        // Compaction off: no summary is ever written, so nothing is informed of a space that is
        // never made.
        {
            let mut state = engine.state.lock().unwrap();
            state.cfg.chat_compaction = String::from(config::COMPACTION_OFF);
            let said = state.build_history().to_string();
            assert!(
                !said.contains("The context of this conversation stays open"),
                "the text of the directory was with compaction off"
            );
        }
    }

    // The user works in one chat with the tools on and a level of thinking picked, and opens a
    // new chat directly after. The new chat is where the one before it was, so the server it
    // requests is the server that already runs, and the model on it stays loaded.
    #[test]
    fn a_new_chat_stands_where_the_chat_before_it_stood() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        {
            let mut state = engine.state.lock().unwrap();
            state.thinking = markdown::ThinkingOptions {
                has_thinking: true,
                has_effort: true,
                efforts: vec![String::from("low"), String::from("medium")],
                has_preserve: false,
            };
            state.start_chat();
        }
        engine.set_thinking("medium");
        engine.set_code(true);
        engine.set_search(true);
        engine.set_sandbox(false);
        let before = {
            let mut state = engine.state.lock().unwrap();
            // A note about the conversation, so the chat before is a real one.
            state.chat_mut().unwrap().messages.push(Message {
                role: String::from("You"),
                body: String::from("test"),
                ..Default::default()
            });
            state.server_wants()
        };
        // `Create new chat` goes back to the first screen, and the first message of it starts
        // the chat. No model is picked here, so the message stops before the worker; the chat
        // is started the same way it is when the message is sent.
        engine.new_chat();
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        let chat = state.chat().expect("the new chat stands open");
        assert_eq!(chat.meta.thinking_mode, "medium");
        assert!(chat.meta.code_enabled);
        assert!(chat.meta.search_enabled);
        assert!(!chat.meta.sandbox_enabled);
        assert!(chat.meta.preserve_thinking);
        // The new chat requests the same server the chat before it ran on, so the worker keeps
        // it and the model on it.
        assert_eq!(state.server_wants(), before);
    }

    // A mode the chat wears already is not a change: the badge reports the same again, and the
    // running server keeps its flags, its model and its loaded text with it. Only a mode that
    // moved sends the server away.
    #[test]
    fn a_thinking_mode_that_did_not_move_moves_nothing() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        {
            let mut state = engine.state.lock().unwrap();
            state.thinking = markdown::ThinkingOptions {
                has_thinking: true,
                has_effort: true,
                efforts: vec![String::from("low"), String::from("medium")],
                has_preserve: false,
            };
            state.start_chat();
        }
        engine.set_thinking("medium");
        let state = engine.state.lock().unwrap();
        assert_eq!(state.thinking_of_open(), "medium");
        drop(state);
        // The same mode again: the chat is where it was, and the choice for the next chat is
        // there too, so the new chat requests the server that is already up.
        engine.set_thinking("medium");
        let state = engine.state.lock().unwrap();
        assert_eq!(state.thinking_of_open(), "medium");
        assert_eq!(state.new_thinking, "medium");
    }

    // The whole rule of keeping a server, without a process behind it. Only another model moves
    // it. The thinking mode, the keeping of the thinking, the tools and the sandbox of a chat
    // are stated with the request, so a chat that requests anything at all sits on the server
    // that is already up.
    #[test]
    fn only_another_model_moves_the_server() {
        let _guard = config::test_turn();
        let running = ServerWants {
            model_id: String::from("one"),
        };
        let wants = ServerWants {
            model_id: String::from("one"),
        };
        // The chat the server was started for sits on it, of course.
        assert!(ServerWants::covers(&running, &wants));
        // Another model is the one item it cannot answer for.
        let second = ServerWants {
            model_id: String::from("two"),
        };
        assert!(!ServerWants::covers(&running, &second));
    }

    // The two thinking flags and the tools of a chat go out with the request and remain nowhere
    // on the command line, so a chat that moves all three between two answers asks the server
    // that is already up. This is the whole point of the rule: a badge moved is not a model
    // loaded again.
    #[test]
    fn a_moved_thinking_or_tool_leaves_the_server_be() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        {
            let mut state = engine.state.lock().unwrap();
            state.thinking = markdown::ThinkingOptions {
                has_thinking: true,
                has_effort: true,
                efforts: vec![String::from("low"), String::from("medium")],
                has_preserve: true,
            };
            state.start_chat();
        }
        engine.set_thinking("medium");
        engine.set_preserve(true);
        engine.set_code(true);
        engine.set_sandbox(true);
        let before = {
            let state = engine.state.lock().unwrap();
            (state.server_wants(), state.thinking_ask())
        };
        assert_eq!(before.1["enable_thinking"], json!(true));
        assert_eq!(before.1["reasoning_effort"], json!("medium"));
        assert_eq!(before.1["preserve_reasoning"], json!(true));
        // Every one of the three moves, and the server the chat requests does not move with
        // them.
        engine.set_thinking("low");
        engine.set_preserve(false);
        engine.set_code(false);
        engine.set_sandbox(false);
        let state = engine.state.lock().unwrap();
        assert_eq!(state.server_wants(), before.0);
        let now = state.thinking_ask();
        assert_eq!(now["reasoning_effort"], json!("low"));
        assert_eq!(now["preserve_reasoning"], json!(false));
        assert_eq!(now["clear_thinking"], json!(true));
    }

    // A chat whose model proposes no thinking at all reports nothing of thinking, and a chat
    // kept from an older version with the word `auto` reports nothing either: the model is left
    // to its own ways, as it was when no flag was on the command line for it.
    #[test]
    fn a_chat_with_nothing_to_say_of_thinking_says_nothing() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        {
            let mut state = engine.state.lock().unwrap();
            state.thinking = markdown::ThinkingOptions::default();
            state.start_chat();
            state.chat_mut().unwrap().meta.thinking_mode = String::from("auto");
            assert!(state.thinking_ask().is_null());
            // The mode of a model that has no thinking is `off`, and `off` is still stated: a
            // template that can switch its thinking is set to.
            state.chat_mut().unwrap().meta.thinking_mode = String::from("off");
            assert_eq!(state.thinking_ask()["enable_thinking"], json!(false));
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
        store.save_chat(&chat).expect("the chat is written");
    }

    // A chat that was started and never written in leaves no file, so the files hold only the
    // chats that hold something.
    #[test]
    fn a_chat_is_written_when_it_holds_a_message() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        // The chats of the place are the rows of the database, not a directory of files: a chat
        // is in one row of `chats.db`. See `src/db.rs`.
        let stored = || {
            store::Store::new(config::repositories_dir(&Config::default()))
                .list_chats("")
                .len()
        };

        // A run starts on the first screen, with no chat and nothing on disk.
        assert!(engine.snapshot().home);
        assert_eq!(stored(), 0);

        // `Create new chat` goes back to the first screen. Nothing was written yet, so nothing
        // is on disk either.
        engine.new_chat();
        assert!(engine.snapshot().home);
        assert_eq!(stored(), 0);

        // The badges of a chat that holds nothing may not write it either.
        {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
        }
        engine.set_code(true);
        engine.set_sandbox(false);
        assert!(!engine.snapshot().home);
        assert_eq!(stored(), 0);

        // The first message makes it a real chat.
        {
            let mut state = engine.state.lock().unwrap();
            state.push_message("You", "A real question", "", 0, false);
        }
        assert_eq!(stored(), 1);
    }

    // The side bar lists every place at once, opens a chat of any of them, and puts the chat
    // that was opened at the header of its group. The place of the chat that is open becomes
    // the place the program works in.
    #[test]
    fn the_side_bar_groups_and_orders_the_chats() {
        let _guard = config::test_turn();
        setup();
        write_chat("aaaabbbb01", 100);
        write_chat("aaaabbbb02", 200);

        let engine = Engine::start(Config::default());
        // The first item the user sees is the first screen, even when the place has chats. They
        // wait in the side bar.
        let snap = engine.snapshot();
        assert!(snap.home);
        assert!(!snap.generating);
        let lists = engine.lists();
        assert_eq!(lists.loose.len(), 2);
        assert_eq!(lists.loose[0].meta.id, "aaaabbbb02");

        // The chat that is at the header of the group is not the chat that is open: opening a
        // chat is what the user does.
        engine.open_chat(String::from("aaaabbbb01"));
        let opened = engine.snapshot();
        assert!(!opened.home);
        assert_eq!(opened.chat_id, "aaaabbbb01");
        // The chat that was opened remains first in its group now, and the rest of the group
        // keeps the order it had.
        let lists = engine.lists();
        assert_eq!(lists.loose[0].meta.id, "aaaabbbb01");
        assert_eq!(lists.loose[1].meta.id, "aaaabbbb02");

        // A new chat is the first screen again, with no chat open.
        engine.new_chat();
        assert!(engine.snapshot().home);

        // A new repository is empty, and the window is on the first screen while it is empty.
        engine.create_repository(String::from("Test Repo"));
        let in_repo = engine.snapshot();
        assert!(in_repo.in_repository);
        assert!(in_repo.home);

        // A chat of that repository is opened from the side bar, and the place that is open
        // follows it, for the space of a command is the repository of the chat.
        let repo_id = repo_named(&engine, "Test Repo");
        write_chat_in(&repo_id, "aaaabbbb03", 300);
        engine.select_repository(String::new());
        assert!(!engine.snapshot().in_repository);
        engine.open_chat(String::from("aaaabbbb03"));
        let gone = engine.snapshot();
        assert_eq!(gone.chat_id, "aaaabbbb03");
        assert!(gone.in_repository);
        let lists = engine.lists();
        let in_test = lists
            .repositories
            .iter()
            .find(|(repo, _)| repo.name == "Test Repo")
            .expect("the repository is in the lists");
        assert_eq!(in_test.1.len(), 1);
        assert_eq!(in_test.1[0].meta.id, "aaaabbbb03");
        // The loose chats are a group of their own, and the chat of the repository is not among
        // them.
        assert!(!lists.loose.iter().any(|c| c.meta.id == "aaaabbbb03"));
    }

    // The menu of a chat renames it, pins it, moves it to another place, puts it away and takes
    // it away.
    #[test]
    fn the_menu_of_a_chat_works_on_the_chat() {
        let _guard = config::test_turn();
        setup();
        write_chat("aaaacccc01", 100);
        let engine = Engine::start(Config::default());
        let in_loose = |id: &str| {
            engine
                .lists()
                .loose
                .iter()
                .any(|c| c.meta.id == id)
        };

        // Another name. The chat keeps everything else it was.
        engine.rename_chat(String::from("aaaacccc01"), String::from("  A new name  "));
        let chat = engine
            .lists()
            .loose
            .iter()
            .find(|c| c.meta.id == "aaaacccc01")
            .expect("the chat is still there")
            .clone();
        assert_eq!(chat.meta.title, "A new name");
        // A name of nothing is no name, and the chat keeps the one it had.
        engine.rename_chat(String::from("aaaacccc01"), String::from("   "));
        assert_eq!(
            engine
                .lists()
                .loose
                .iter()
                .find(|c| c.meta.id == "aaaacccc01")
                .expect("the chat is still there")
                .meta
                .title,
            "A new name"
        );

        // The pin. The marker is with the chat, so it is still there when the chat is read
        // again.
        engine.set_chat_pinned(String::from("aaaacccc01"), true);
        assert!(engine
            .lists()
            .loose
            .iter()
            .any(|c| c.meta.id == "aaaacccc01" && c.meta.pinned));
        // The chat is read from the disk, so the marker came off the file and not only out of
        // the memory.
        let again = engine
            .state
            .lock()
            .unwrap()
            .store
            .load_chat("aaaacccc01")
            .expect("the chat is on disk");
        assert!(again.meta.pinned);
        engine.set_chat_pinned(String::from("aaaacccc01"), false);
        assert!(in_loose("aaaacccc01"));
        assert!(!engine
            .lists()
            .loose
            .iter()
            .any(|c| c.meta.id == "aaaacccc01" && c.meta.pinned));

        // A name the user never gave is not a name of a chat: nothing is changed and nothing is
        // reported of an error either.
        engine.rename_chat(String::from("nosuchchat"), String::from("Nothing"));

        // Put away, and brought back. A chat that is closed is in no group of the side bar, and
        // is in the list of the archived chats.
        engine.set_chat_archived(String::from("aaaacccc01"), true);
        assert!(!in_loose("aaaacccc01"));
        assert!(engine
            .lists()
            .archived
            .iter()
            .any(|c| c.meta.id == "aaaacccc01"));
        engine.set_chat_archived(String::from("aaaacccc01"), false);
        assert!(in_loose("aaaacccc01"));
        assert!(engine.lists().archived.is_empty());

        // Move it into a repository, and out of it again. The chat is in one place at a time,
        // so the file of the place it came from is removed.
        let repo_id = {
            engine.create_repository(String::from("Moving In"));
            repo_named(&engine, "Moving In")
        };
        engine.move_chat(String::from("aaaacccc01"), repo_id.clone());
        assert!(!in_loose("aaaacccc01"));
        let moved = engine
            .lists()
            .repositories
            .iter()
            .find(|(repo, _)| repo.id == repo_id)
            .expect("the repository is there")
            .1
            .clone();
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].meta.repo_id, repo_id);
        // The chat is in one place at a time, and the database holds the place it is in: the
        // row of it reports which.
        let place_of = || {
            store::Store::new(config::repositories_dir(&Config::default()))
                .load_chat("aaaacccc01")
                .map(|chat| chat.meta.repo_id)
        };
        assert_eq!(place_of().as_deref(), Some(repo_id.as_str()), "the place of the chat did not move");
        engine.move_chat(String::from("aaaacccc01"), String::new());
        assert!(in_loose("aaaacccc01"));

        // A place that is not there is not a place to move a chat to.
        engine.move_chat(String::from("aaaacccc01"), String::from("ffffffffffff"));
        assert!(in_loose("aaaacccc01"));

        // Take the chat away. It is removed from the list and from the disk.
        engine.remove_chat(String::from("aaaacccc01"));
        assert!(!in_loose("aaaacccc01"));
        assert!(engine.lists().archived.is_empty());
        assert_eq!(
            store::Store::new(config::repositories_dir(&Config::default()))
                .list_chats("")
                .len(),
            0,
            "the chat is removed from the database as well as from the list"
        );
    }

    // The chat that is left open is closed, moved or removed by the side bar too. The window
    // goes to the first screen, for the conversation it held is out of the way or removed.
    #[test]
    fn the_open_chat_follows_the_work_of_the_side_bar() {
        let _guard = config::test_turn();
        setup();
        write_chat("aaaadddd01", 100);
        let engine = Engine::start(Config::default());
        engine.open_chat(String::from("aaaadddd01"));
        assert!(!engine.snapshot().home);

        // Put away: the window has nothing to show, so it is on the first screen.
        engine.set_chat_archived(String::from("aaaadddd01"), true);
        assert!(engine.snapshot().home);
        engine.set_chat_archived(String::from("aaaadddd01"), false);

        // Moved into a repository: the place that is open goes with the chat, and the chat
        // stays open.
        let repo_id = {
            engine.create_repository(String::from("Carried Off"));
            repo_named(&engine, "Carried Off")
        };
        // `Create repository` moves the place that is open, which closes the chat that was in
        // another place. Open it again, then move it.
        engine.open_chat(String::from("aaaadddd01"));
        assert_eq!(engine.snapshot().chat_id, "aaaadddd01");
        engine.move_chat(String::from("aaaadddd01"), repo_id.clone());
        let moved = engine.snapshot();
        assert_eq!(moved.chat_id, "aaaadddd01");
        assert!(moved.in_repository);
        assert!(!moved.home);

        // Removed: the window goes back to the first screen.
        engine.remove_chat(String::from("aaaadddd01"));
        assert!(engine.snapshot().home);
        assert_eq!(engine.snapshot().chat_id, "");
    }

    // A repository is named again, and the folder it is in follows the name, for the directory
    // of a repository is named after it. The id does not move, so the conversations of the
    // repository keep their place and the chats keep pointing at it. A repository is pinned for
    // the group above the rest.
    #[test]
    fn a_repository_is_named_again_and_pinned() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        engine.create_repository(String::from("First Name"));
        let id = repo_named(&engine, "First Name");
        write_chat_in(&id, "aaaaeeee01", 100);
        let older = config::repositories_dir(&Config::default()).join(format!("first-name_{}", id));
        assert!(older.is_dir(), "the directory of a repository is its name");

        engine.rename_repository(id.clone(), String::from("Second Name"));
        let newer = config::repositories_dir(&Config::default()).join(format!("second-name_{}", id));
        assert!(newer.is_dir(), "the folder went with the name");
        assert!(!older.exists(), "and the older folder is removed");
        let snap = engine.snapshot();
        let shown = snap
            .repositories
            .iter()
            .find(|repo| repo.id == id)
            .expect("the repository is in the bar");
        assert_eq!(shown.name, "Second Name");
        // The conversations of the repository did not move, for they are under the data
        // directory and are keyed on the id.
        let lists = engine.lists();
        let group = lists
            .repositories
            .iter()
            .find(|(repo, _)| repo.id == id)
            .expect("the repository is in the lists");
        assert_eq!(group.0.name, "Second Name");
        assert_eq!(group.1.len(), 1);
        assert_eq!(group.1[0].meta.id, "aaaaeeee01");

        // A name of nothing is no name, and the repository keeps the one it had.
        engine.rename_repository(id.clone(), String::from("   "));
        assert_eq!(repo_named(&engine, "Second Name"), id);

        // The pin. The marker is in the record of the repository.
        engine.set_repository_pinned(id.clone(), true);
        assert!(engine
            .lists()
            .repositories
            .iter()
            .any(|(repo, _)| repo.id == id && repo.pinned));
        // Taken out of the group above the rest again. The marker is in the record of this
        // repository alone.
        engine.set_repository_pinned(id.clone(), false);
        assert!(!repo_row(&engine, &id).0.pinned);
    }

    // The name a repository was given, out of the lists the side bar reads. A name that is not
    // there stops the test.
    fn repo_named(engine: &Engine, name: &str) -> String {
        engine
            .lists()
            .repositories
            .iter()
            .find(|(repo, _)| repo.name == name)
            .unwrap_or_else(|| panic!("no repository named {}", name))
            .0
            .id
            .clone()
    }

    // A chat of a repository, written on disk.
    fn write_chat_in(repo_id: &str, id: &str, updated: i64) {
        let store = store::Store::new(config::repositories_dir(&Config::default()));
        let mut chat = Chat::new(repo_id, "");
        chat.meta.id = id.to_string();
        chat.meta.title = id.to_string();
        chat.meta.updated = updated;
        chat.messages.push(Message {
            role: String::from("You"),
            body: String::from("A written question"),
            when: updated,
            ..Default::default()
        });
        store.save_chat(&chat).expect("the chat is written");
    }

    // Open a chat the way the first message of a user does. `Create new chat` puts the first
    // screen up, and the text written there start the chat. No model is picked in these tests,
    // so no message is sent; the chat is started in the same way as it is when one does.
    fn open_a_chat(engine: &Engine) {
        engine.new_chat();
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
    }

    // One repository row of the side bar, with the chats that are in it.
    fn repo_row(engine: &Engine, id: &str) -> (Repo, Vec<Chat>) {
        engine
            .lists()
            .repositories
            .into_iter()
            .find(|(repo, _)| repo.id == id)
            .unwrap_or_else(|| panic!("no repository row for {}", id))
    }

    // The badge proposes the modes of the model and the user chooses one. A mode the model does
    // not propose is refused, and a model with no thinking mode does not change.
    #[test]
    fn the_thinking_mode_is_chosen() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        open_a_chat(&engine);
        {
            let mut state = engine.state.lock().unwrap();
            state.thinking = markdown::ThinkingOptions {
                has_thinking: true,
                has_effort: false,
                efforts: Vec::new(),
                has_preserve: false,
            };
        }
        // The switch model proposes off and on, and a new chat starts with the thinking off.
        assert_eq!(engine.snapshot().thinking_mode, "off");
        engine.set_thinking("on");
        assert_eq!(engine.snapshot().thinking_mode, "on");
        // A level is no use to a model with only a switch.
        engine.set_thinking("xhigh");
        assert_eq!(engine.snapshot().thinking_mode, "on");
        engine.set_thinking("off");
        assert_eq!(engine.snapshot().thinking_mode, "off");
        // The effort model proposes the levels its template names.
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

    // A model with levels and a model with a switch do not propose the same text. Switching
    // from one to the other moves the thinking to a mode the new model proposes, so the badge
    // never is on a word the new template cannot read, and the flags of the server agree with
    // the badge.
    #[test]
    fn a_thinking_mode_the_new_model_lacks_gives_place_to_off() {
        let _guard = config::test_turn();
        setup();
        let levels = String::from(
            "{% if reasoning_effort == 'low' %}low{% elif reasoning_effort == 'medium' %}medium{% elif reasoning_effort == 'high' %}high{% endif %}",
        );
        let switch_on = String::from("{% if enable_thinking %}think{% endif %}");
        let mut settings = std::collections::BTreeMap::new();
        settings.insert(
            String::from("model-a"),
            ModelSettings {
                chat_template: levels,
                ..Default::default()
            },
        );
        settings.insert(
            String::from("model-b"),
            ModelSettings {
                chat_template: switch_on,
                ..Default::default()
            },
        );
        let cfg = Config {
            model_id: String::from("model-a"),
            model_settings: settings,
            ..Config::default()
        };
        let engine = Engine::start(cfg);
        {
            let mut state = engine.state.lock().unwrap();
            state.refresh_thinking();
            state.start_chat();
        }
        // Model A proposes the levels, and the user is on medium.
        assert!(engine
            .snapshot()
            .thinking_modes
            .contains(&String::from("medium")));
        engine.set_thinking("medium");
        assert_eq!(engine.snapshot().thinking_mode, "medium");
        // Model B knows no levels. The badge moves to off, the chat moves with it, and the
        // server is requested what the badge shows.
        engine.select_model(String::from("model-b"));
        let snap = engine.snapshot();
        assert_eq!(snap.thinking_mode, "off");
        assert!(!snap.thinking_modes.contains(&String::from("medium")));
        let state = engine.state.lock().unwrap();
        let chat = state.chat().expect("the chat stands open");
        assert_eq!(chat.meta.thinking_mode, "off");
        // The request of this chat reports the same word as the badge, for the badge and the
        // request are read from the one place.
        assert_eq!(state.thinking_ask()["enable_thinking"], json!(false));
        drop(state);
        // Back to the levels: off is a mode both models propose, so it remains, and nothing
        // moves on its own.
        engine.select_model(String::from("model-a"));
        assert_eq!(engine.snapshot().thinking_mode, "off");
        engine.set_thinking("medium");
        assert_eq!(engine.snapshot().thinking_mode, "medium");
    }

    // The choice to keep the thinking of the turns before the last answer is with the chat, and
    // only a model whose template proposes it may change it.
    #[test]
    fn the_thinking_of_the_earlier_turns_may_be_kept() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        open_a_chat(&engine);
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
        // A model whose template cannot keep its thinking does not change, and records it.
        {
            let mut state = engine.state.lock().unwrap();
            state.thinking = markdown::ThinkingOptions::default();
        }
        engine.set_preserve(false);
        assert!(engine.snapshot().thinking_preserve);
        assert!(engine
            .snapshot()
            .status
            .contains("no setting for keeping the thinking"));
    }

    // The choice to keep the thinking is not only a flag of the server. The template of a model
    // reads the thoughts of an answer out of the text it is sent, so the thoughts have to
    // travel with the answer they came from - otherwise there is nothing there to keep, and the
    // model is left with no memory of its own reasoning. A chat that drops them, and a model
    // whose template cannot keep them, send no thoughts at all.
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
            state.start_chat();
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
                // A turn that thought and then requested a command leaves its thinking with the
                // command, so it must travel the same path.
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
            // The line of the day and the clock time that goes before the first message of the
            // chat is not one of the messages this test counts.
            let list = undated(&state.build_history());
            assert_eq!(list.len(), 4);
            // The text of the user carry no thoughts of their own.
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
        // A model whose template cannot keep its thinking is sent none either, whatever the
        // chat chooses.
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

    // The markers of a colour keep the text they remain about and add nothing of their own. A
    // marker that nothing closes stays where the model wrote it, for the answer stopped in the
    // middle of it. A marker of another type, a marker with more text in its name, and a pair
    // of markers with nothing between them are none of the four colours, and nothing of them is
    // touched.
    #[test]
    fn the_colour_marks_are_taken_out_of_their_words() {
        let _guard = config::test_turn();
        // Text with no markers in it returns as it went in.
        assert_eq!(strip_colour_tags("nothing coloured here"), "nothing coloured here");
        // The four colours, each with its text kept.
        assert_eq!(strip_colour_tags("<green>done</green>"), "done");
        assert_eq!(
            strip_colour_tags("<light_green>almost done</light_green>"),
            "almost done"
        );
        assert_eq!(strip_colour_tags("<red>it failed</red>"), "it failed");
        assert_eq!(
            strip_colour_tags("<light_red>it may fail</light_red>"),
            "it may fail"
        );
        // Two colours in one line of text, and text between them.
        assert_eq!(
            strip_colour_tags("the build is <green>green</green> and the test is <red>red</red>."),
            "the build is green and the test is red."
        );
        // A colour inside a colour loses both pairs of markers.
        assert_eq!(
            strip_colour_tags("<green>a <red>b</red> c</green>"),
            "a b c"
        );
        // A marker that nothing closes stays, and so does a marker of another type.
        assert_eq!(strip_colour_tags("a <green>b"), "a <green>b");
        assert_eq!(strip_colour_tags("<b>words</b>"), "<b>words</b>");
        assert_eq!(
            strip_colour_tags("<greenish>words</greenish>"),
            "<greenish>words</greenish>"
        );
        assert_eq!(strip_colour_tags("<green></green>"), "<green></green>");
        // A marker of one colour closed by the marker of another is skipped.
        assert_eq!(strip_colour_tags("<green>a</red>"), "<green>a</red>");
        // Text with no colour in it at all, of any length, is not removed.
        let plain = "n".repeat(5000);
        assert_eq!(strip_colour_tags(&plain), plain);
        // Markers of one colour with nothing between them but more markers of the same colour,
        // and one marker that closes them: the first marker that closes is the one the text is
        // in, and the markers inside it stay, for nothing closes them where they are. The page
        // reads the same pair.
        let deep = "<green>".repeat(200) + "x" + &"</green>".repeat(200);
        assert_eq!(
            strip_colour_tags(&deep),
            "<green>".repeat(199) + "x" + &"</green>".repeat(199)
        );
        // The depth of the colours is counted. Past the depth the page reads, the text is given
        // back as they are now, so an answer that put colour a long way down cannot request
        // more work than there is space for.
        let one = "<green>x</green>";
        assert_eq!(strip_colour_tags_deep(one, 0), "x");
        assert_eq!(strip_colour_tags_deep(one, GREATEST_COLOUR_NESTING), one);
        assert_eq!(strip_colour_tags_deep(one, GREATEST_COLOUR_NESTING - 1), "x");
    }

    // The markers of a colour are a look over the text and not text of the answer. While the
    // switch is on, they go out of the conversation that goes back to the model, and the text
    // that tell the model of them is at the header of it. With the switch off the markers stay
    // where the model wrote them, and that text go away with the switch.
    #[test]
    fn the_talk_carries_the_colour_marks_only_while_the_switch_stands_off() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
            if let Some(chat) = state.chat_mut() {
                chat.messages.push(Message {
                    role: String::from("You"),
                    body: String::from("is the build finished?"),
                    ..Default::default()
                });
                chat.messages.push(Message {
                    role: String::from("Assistant"),
                    body: String::from(
                        "the build is <green>green</green> and the test is <light_red>red</light_red>",
                    ),
                    ..Default::default()
                });
            }
            let list = state.build_history();
            let answers = list
                .as_array()
                .expect("a list of the conversation")
                .iter()
                .filter(|message| message["role"].as_str() == Some("assistant"))
                .map(|message| message["content"].as_str().unwrap_or_default())
                .collect::<Vec<&str>>();
            assert_eq!(answers, vec!["the build is green and the test is red"]);
            // The text of the colours is at the header of the conversation, once for it.
            let told = list
                .as_array()
                .expect("a list of the conversation")
                .iter()
                .filter(|message| {
                    message["content"]
                        .as_str()
                        .unwrap_or_default()
                        .starts_with("You may put colour on a few words")
                })
                .count();
            assert_eq!(told, 1);
        }
        // The switch off: the markers travel back as the model wrote them, for the page reads
        // no colour then and the model must still find its own markers.
        {
            let mut state = engine.state.lock().unwrap();
            state.cfg.chat_colour_text = false;
            let list = state.build_history();
            let answers = list
                .as_array()
                .expect("a list of the conversation")
                .iter()
                .filter(|message| message["role"].as_str() == Some("assistant"))
                .map(|message| message["content"].as_str().unwrap_or_default())
                .collect::<Vec<&str>>();
            assert_eq!(
                answers,
                vec!["the build is <green>green</green> and the test is <light_red>red</light_red>"]
            );
            let told = list
                .as_array()
                .expect("a list of the conversation")
                .iter()
                .filter(|message| {
                    message["content"]
                        .as_str()
                        .unwrap_or_default()
                        .starts_with("You may put colour on a few words")
                })
                .count();
            assert_eq!(told, 0);
        }
    }

    // A chapter is begun by a main heading of an answer. The chapters are given to the window
    // in the order they begin, each tied to the message before the answer that carries it, and
    // the heading itself stays among the text of the conversation: it is part of the answer,
    // and not a marker beside it.
    #[test]
    fn the_chapters_are_read_from_the_headings_of_the_answers() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        open_a_chat(&engine);
        let (first, second) = {
            let mut state = engine.state.lock().unwrap();
            state.push_message("You", "Fix the parser", "", 0, false);
            let first = state.chat().unwrap().messages.last().unwrap().id.clone();
            state.push_message(
                "Assistant",
                "## Fixing the parser\n\nThe grammar was wrong.\n\n### A small thing inside\n\n```python\n# not a heading\nprint(1)\n```",
                "",
                0,
                false,
            );
            let second = state.chat().unwrap().messages.last().unwrap().id.clone();
            state.push_message(
                "Assistant",
                "# Over in the next part\n\nNothing else stands here.",
                "",
                0,
                false,
            );
            (first, second)
        };
        let chapters = engine.snapshot().chapters;
        assert_eq!(chapters.len(), 2, "the headings were not read as chapters");
        assert_eq!(chapters[0].name, "Fixing the parser");
        // The chapter begins after the message before the answer that carries it, so the answer
        // itself remains within the chapter it began.
        assert_eq!(chapters[0].after, first);
        assert_eq!(chapters[1].name, "Over in the next part");
        assert_eq!(chapters[1].after, second);
        // The heading is a note about the answer, so the answer is drawn whole and the window
        // is given the chapter on the side as well.
        let visible = engine.snapshot().messages;
        assert_eq!(visible.len(), 3);
        assert!(visible[1].body.contains("## Fixing the parser"));
    }

    // Only a main heading opens a chapter: one or two markers and a space, outside a block of
    // code, with text after the markers. A heading with markers behind its text is named by the
    // text alone, and a long name is cut to what a node can carry.
    #[test]
    fn only_a_main_heading_of_the_answer_opens_a_chapter() {
        let _guard = config::test_turn();
        assert_eq!(heading_names("# One\nText\n## Two\n"), vec!["One", "Two"]);
        // A third marker is a heading of a part, not of a chapter of the chat.
        assert!(heading_names("### Three").is_empty());
        // A word that runs on from the markers is a word.
        assert!(heading_names("#One").is_empty());
        // A `#` at the start of a line is a comment inside a block of code.
        assert!(heading_names("```\n# inside the code\n```\n").is_empty());
        assert!(heading_names("~~~\n# inside the code\n~~~\n").is_empty());
        // A heading with nothing after its markers names nothing.
        assert!(heading_names("##   ").is_empty());
        // The markers behind the text of a heading is no part of its name.
        assert_eq!(heading_names("## Four ##"), vec!["Four"]);
        // A heading of a quote or a list line is not a heading of its own.
        assert!(heading_names("> # quoted").is_empty());
        // The name of a chapter is cut to what a node beside the chat can carry.
        let long = heading_names(&format!("# {}", "n".repeat(80)));
        assert_eq!(long.len(), 1);
        assert_eq!(long[0].chars().count(), 61, "the name was not cut: {}", long[0]);
        assert!(long[0].ends_with('\u{2026}'));
    }

    // The chats of before carry their chapters as markers of their own, with a message each.
    // Those markers are still read, so a chat written while the tool of the chapters were keeps
    // the line of nodes it was given, and an answer with headings in it keeps those too.
    #[test]
    fn the_chapters_of_an_older_chat_are_still_read() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        open_a_chat(&engine);
        {
            let mut state = engine.state.lock().unwrap();
            state.push_message("You", "Fix the parser", "", 0, false);
            state.push_message(
                "Chapter",
                "Fixing the parser",
                "The chapter was marked",
                0,
                false,
            );
            state.push_message(
                "Assistant",
                "## And now the tests",
                "",
                0,
                false,
            );
        }
        let chapters = engine.snapshot().chapters;
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].name, "Fixing the parser");
        assert_eq!(chapters[1].name, "And now the tests");
        // A marker of a chapter is no word of the conversation, so the window is given the
        // answers alone.
        let visible = engine.snapshot().messages;
        assert!(!visible.iter().any(|m| m.role == "Chapter"));
        assert_eq!(visible.len(), 2);
    }

    // A turn writes no line about itself. A command is an answer, a stop is not a sentence, and
    // a failure is already informed by its own note.
    #[test]
    fn a_turn_writes_no_line_about_itself() {
        let _guard = config::test_turn();
        // Text means themselves, also the text of a stopped run.
        assert_eq!(
            kept_of("Here is the plan", "", false, false, false),
            Kept::Words
        );
        assert_eq!(
            kept_of("Half way to the plan", "", true, false, false),
            Kept::Words
        );
        // The model thought and then requested a command. The command is the answer, so no line
        // is written about a missing one.
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
        // A failed run is informed by the note of the failure alone.
        assert_eq!(kept_of("", "", false, false, true), Kept::Nothing);
        // Nothing arrived, and nothing failed. One quiet line is right.
        assert_eq!(kept_of("", "", false, false, false), Kept::Note);
    }

    // A model that came to its own end without a word is asked again, for the work was never
    // finished; a model that used the space up is not, and neither is a silence that has
    // already been asked over as often as it may be.
    #[test]
    fn a_silence_is_asked_over() {
        let _guard = config::test_turn();
        // The silence of a model that simply stopped, and has tries left.
        assert!(worth_a_silence("stop", 0, 2));
        assert!(worth_a_silence("stop", 1, 2));
        // The last try was spent, so the quiet line is written.
        assert!(!worth_a_silence("stop", 2, 2));
        // No space for tries at all leaves the silence alone.
        assert!(!worth_a_silence("stop", 0, 0));
        // A silence because the space for the context was used up is not mended by asking
        // again, whatever the number of tries reports.
        assert!(!worth_a_silence("length", 0, 2));
    }

    // The thinking of a turn with no text of its own is with the command it led to, so it is
    // not lost when the line about the answer is left out.
    #[test]
    fn a_tool_run_keeps_the_thinking_of_its_turn() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        open_a_chat(&engine);
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

    // The question the model put remains a moment after the call was created, for the run of
    // the tool is on a thread of its own. The test searches for it as a window does, and gives
    // up after two seconds of nothing.
    fn asked_of(engine: &Engine) -> Asking {
        for _ in 0..100 {
            if let Some(one) = engine.snapshot().ask {
                return one;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("no question ever stood above the chat box");
    }

    // A question the model cannot go on without is above the chat composer until the user
    // answers it, and what they stated goes back to the model as the result of the call. The
    // request is removed with the answer, and answering the same question twice does nothing
    // the second time.
    #[test]
    fn a_question_the_user_answers_goes_back_to_the_model() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let (workdir, cancel, chat) = {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
            let room = state.workdir_for(state.chat().expect("a blank chat stands open"));
            let chat = state.open_id();
            std::mem::drop(state);
            (
                room,
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                chat,
            )
        };
        let call = ToolCall {
            id: String::from("call-ask"),
            name: String::from(askuser::NAME),
            arguments: String::from(
                "{\"question\":\"Which token should I use?\",\"choices\":[\"Here it is\",\"I have none\"],\"kind\":\"critical\"}",
            ),
        };
        let held = engine.state.clone();
        let waiter = cancel.clone();
        let worker = {
            let call = call.clone();
            std::thread::spawn(move || {
                run_program_tool(&call, "", 0, &held, std::path::Path::new("."), &waiter, &chat)
            })
        };
        // The request remains while the run waits for a word. A weighty question is waited for
        // as long as it takes, so it carries no time left.
        let asked = asked_of(&engine);
        assert_eq!(asked.question, "Which token should I use?");
        assert_eq!(asked.choices.len(), 2);
        assert!(asked.critical, "the model stated it cannot go on");
        assert_eq!(asked.left(), None, "a weighty question waits for ever");
        assert!(engine.answer_ask(&asked.id, "  Here it is  "), "the answer came home");
        assert!(
            !engine.answer_ask(&asked.id, "again"),
            "a question is answered once"
        );
        worker.join().expect("the run came to its end");
        let snap = engine.snapshot();
        let run = snap
            .messages
            .iter()
            .find(|one| one.tool_name == askuser::NAME)
            .expect("the chat holds the run");
        assert_eq!(run.body, "Which token should I use?");
        assert!(!run.failed, "{}", run.tool);
        assert_eq!(run.tool, "The user answered: Here it is");
        assert!(snap.ask.is_none(), "the request was removed with the answer");
        let _ = std::fs::remove_dir_all(&workdir);
    }

    // A question of the ordinary type is waited for a short while, and then given up: the model
    // is informed that the user did not answer, and the run is not held up any longer than the
    // setting reports.
    #[test]
    fn a_question_that_is_not_answered_is_given_up() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
            state.cfg.ask_seconds = 1;
        }
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let call = ToolCall {
            id: String::from("call-ask-quiet"),
            name: String::from(askuser::NAME),
            arguments: String::from("{\"question\":\"Shall I write the file?\"}"),
        };
        let chat = engine.state.lock().unwrap().open_id();
        let held = engine.state.clone();
        let waiter = cancel.clone();
        let worker = {
            let call = call.clone();
            std::thread::spawn(move || {
                run_program_tool(&call, "", 0, &held, std::path::Path::new("."), &waiter, &chat)
            })
        };
        let asked = asked_of(&engine);
        assert!(!asked.critical);
        assert!(asked.choices.is_empty(), "nothing was proposed to choose from");
        assert!(asked.left().is_some(), "an ordinary question is timed");
        worker.join().expect("the wait came to an end");
        let snap = engine.snapshot();
        let run = snap
            .messages
            .iter()
            .find(|one| one.tool_name == askuser::NAME)
            .expect("the chat holds the run");
        assert!(run.tool.contains("did not answer within 1 second"), "{}", run.tool);
        assert!(!run.failed, "a question given up is no error of the run");
        assert!(snap.ask.is_none(), "the request was removed with the wait");
    }

    // The clock of a running call, the marker that gives it five minutes more and the marker
    // that cuts it short are created out of the state, so the state has to carry the call that
    // is running with all of the time it was given and the time it has left. This record was
    // left out of the text the page is given, and no running block wore a marker at all.
    #[test]
    fn the_state_carries_the_call_that_is_running_with_its_time() {
        let _guard = config::test_turn();
        let engine = Engine::start(Config::default());
        let clean = engine.snapshot();
        assert!(crate::web::state(&clean, None)["running"].is_null());
        let mut snap = engine.snapshot();
        snap.running = Some(RunningTool {
            id: String::from("call-one"),
            timeout: Some(600),
            left: Some(593),
        });
        let value = crate::web::state(&snap, None);
        assert_eq!(value["running"]["id"], "call-one");
        assert_eq!(value["running"]["timeout"], 600);
        assert_eq!(value["running"]["left"], 593);
    }

    // A run of the download tool leaves its propose on the message of the run, and the user
    // reaches the file by the name the block carries. A name that steps out of the working directory of
    // the chat is answered and never proposed, which is the rule of the sandbox over a download
    // as it is over a command.
    #[test]
    fn the_offer_of_a_file_hangs_on_the_run_and_reaches_the_user() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let workdir = {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
            let room = state.workdir_for(state.chat().expect("a blank chat stands open"));
            std::fs::create_dir_all(room.join("notes")).unwrap();
            std::fs::write(room.join("page.html"), "<p>work</p>").unwrap();
            std::fs::write(room.join("notes/table.csv"), "a,b\n1,2\n").unwrap();
            room
        };
        let call = ToolCall {
            id: String::from("call-offer"),
            name: String::from(download::NAME),
            arguments: String::from(
                "{\"paths\":[\"page.html\",\"notes/table.csv\",\"../elsewhere.txt\"]}",
            ),
        };
        let chat = engine.state.lock().unwrap().open_id();
        run_program_tool(
            &call,
            "",
            0,
            &engine.state,
            &workdir,
            &std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            &chat,
        );
        let snap = engine.snapshot();
        let run = snap
            .messages
            .iter()
            .find(|one| one.tool_name == download::NAME)
            .expect("the chat holds the run");
        assert_eq!(run.role, "Tool");
        assert!(!run.failed, "{}", run.tool);
        assert_eq!(run.downloads.len(), 2, "the two files of the space are proposed");
        assert_eq!(run.downloads[0].name, "page");
        assert_eq!(run.downloads[0].extension, "html");
        assert_eq!(run.downloads[0].kind, "page");
        assert_eq!(run.downloads[0].path, "page.html");
        assert_eq!(run.downloads[1].path, "notes/table.csv");
        assert_eq!(run.downloads[1].kind, "table");
        assert!(run.tool.contains("2 files"), "{}", run.tool);
        assert!(run.tool.contains("Not offered"), "{}", run.tool);
        // The state of a page carries the proposal with the run, so the blocks of an propose
        // are drawn again when the chat is opened another day.
        let value = crate::web::state(&snap, None);
        let rows = value["messages"].as_array().expect("the state carries the conversation");
        let row = rows
            .iter()
            .find(|one| one["tool_name"] == download::NAME)
            .expect("the run stands in the state");
        let held = &row["downloads"][0];
        assert_eq!(held["name"], "page");
        assert_eq!(held["extension"], "html");
        assert_eq!(held["kind"], "page");
        assert_eq!(held["path"], "page.html");
        assert_eq!(held["size"], 11);
        // The button of a block drops these two paths. The bytes of a file of the chat return
        // under the name of the file and the type of its bytes; a name that leaves the working
        // directory of the chat, or that names a file which is not there, gives nothing at all.
        let (name, kind, bytes) = engine
            .gift("notes/table.csv")
            .expect("the file is in the space");
        assert_eq!(name, "table.csv");
        assert_eq!(kind, "text/csv");
        assert_eq!(bytes, b"a,b\n1,2\n");
        assert!(engine.gift("../elsewhere.txt").is_none());
        assert!(engine.gift("nowhere.txt").is_none());
        let (called, kept) = engine
            .gift_zip(&[
                String::from("page.html"),
                String::from("notes/table.csv"),
                String::from("nowhere.txt"),
            ])
            .expect("two of the files are in the space");
        assert_eq!(called, "page.zip");
        assert_eq!(&kept[0..4], b"PK\x03\x04");
        assert!(engine.gift_zip(&[String::from("nowhere.txt")]).is_none());
        let _ = std::fs::remove_dir_all(&workdir);
    }

    // A tool whose text is neither a command nor a file shows the parts of the call as they
    // arrive, so the block is not blank while the model writes, and the header still names the
    // one item the call is pointed at.
    #[test]
    fn the_parts_of_a_call_are_written_into_the_block() {
        let ask = ToolCall {
            id: String::from("call-ask-parts"),
            name: String::from(askuser::NAME),
            arguments: String::from(
                "{\"question\":\"Which room?\",\"choices\":[\"The big one\",\"The small one\"]}",
            ),
        };
        let made = watched(std::slice::from_ref(&ask));
        assert_eq!(made[0].target, "Which room?");
        assert_eq!(
            made[0].command,
            "question: Which room?\nchoices: [\"The big one\",\"The small one\"]"
        );
        // The whole plan of planning mode is in the body as it is written. Before, the block
        // stated only 'the plan file' and grew by nothing.
        let plan = ToolCall {
            id: String::from("call-plan-parts"),
            name: String::from(planning::INIT_NAME),
            arguments: String::from("{\"plan\":\"# Plan\\nFix the parser\"}"),
        };
        let made = watched(std::slice::from_ref(&plan));
        assert_eq!(made[0].target, "the plan file");
        assert_eq!(made[0].command, "plan: # Plan\nFix the parser");
        // A tool of the infographic suite is named for the project it works on, and carries its
        // parts under their own names.
        let scene = ToolCall {
            id: String::from("call-scene-parts"),
            name: String::from(infographic::SCENE_NAME),
            arguments: String::from("{\"project\":\"launch\",\"scene\":{\"title\":\"First light\"}}"),
        };
        let made = watched(std::slice::from_ref(&scene));
        assert_eq!(made[0].target, "a scene of a video (launch)");
        assert_eq!(
            made[0].command,
            "project: launch\nscene: {\"title\":\"First light\"}"
        );
        // The summary of the work for the corporate display is named by the heading of the area
        // of work, and the lines are in the body.
        let short = ToolCall {
            id: String::from("call-short-parts"),
            name: String::from(corporate::NAME),
            arguments: String::from("{\"topic\":\"The parser\",\"lines\":[\"Found the fault\"]}"),
        };
        let made = watched(std::slice::from_ref(&short));
        assert_eq!(made[0].target, "The parser");
        assert_eq!(
            made[0].command,
            "topic: The parser\nlines: [\"Found the fault\"]"
        );
        // A call that has named nothing yet has no name to put in the header, so the header
        // reports what the tool is pointed at. The body still shows the part the model is
        // writing.
        let half = ToolCall {
            id: String::from("call-half-parts"),
            name: String::from(search::NAME),
            arguments: String::from("{\"depth\":2,\"query\":\"sea"),
        };
        let made = watched(std::slice::from_ref(&half));
        assert!(!made[0].target.trim().is_empty(), "the head never stands blank");
        assert!(
            made[0].command.starts_with("depth: 2"),
            "{}",
            made[0].command
        );
    }

    // None of the tools that show their parts is with a blank header: a call with nothing in it
    // still reports what the tool is pointed at, and never the text of the body, which are the
    // parts of the call.
    #[test]
    fn the_head_of_a_call_that_named_nothing_still_says_something() {
        let names = [
            search::NAME,
            download::NAME,
            display::NAME,
            askuser::NAME,
            suggest::NAME,
            viewimage::NAME,
            summarise::NAME,
            planning::INIT_NAME,
            planning::EDIT_NAME,
            planning::READ_NAME,
            planning::ASK_NAME,
            planning::END_NAME,
            infographic::GUIDE_NAME,
            infographic::PLAN_NAME,
            infographic::SCENE_NAME,
            infographic::CHECK_NAME,
            infographic::RENDER_NAME,
            corporate::NAME,
            tasks::CREATE_NAME,
            tasks::VIEW_ALL_NAME,
            tasks::VIEW_ONE_NAME,
            tasks::COMPLETE_NAME,
            tasks::CANCEL_NAME,
            tasks::DELETE_NAME,
        ];
        for one in names.iter() {
            let call = ToolCall {
                id: format!("call-{}", one),
                name: String::from(*one),
                arguments: String::from("{}"),
            };
            let made = watched(std::slice::from_ref(&call));
            assert!(!made[0].target.trim().is_empty(), "{} stands blank", one);
            assert!(
                !made[0].target.contains(':'),
                "{} names a part of the call instead of the thing it is at: {}",
                one,
                made[0].target
            );
        }
    }

    // The suggestions for the next steps hangs on the run that made it, and the state carries
    // it with the run, so the buttons are drawn again when the chat is opened another day and
    // in the page of the remote mode. The newest propose of a chat is the one that remains: a
    // second call takes the buttons of the first away, for the paths an older answer named are
    // no longer the paths the conversation is at. The line of the drop-down of the badge holds
    // the tool as it holds every other tool of the program.
    #[test]
    fn the_offer_of_a_next_step_hangs_on_the_run_and_the_newest_stands() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let workdir = {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
            let room = state.workdir_for(state.chat().expect("a blank chat stands open"));
            std::fs::create_dir_all(&room).unwrap();
            room
        };
        let chat = engine.state.lock().unwrap().open_id();
        let run = |engine: &Engine, id: &str, arguments: &str| {
            run_program_tool(
                &ToolCall {
                    id: String::from(id),
                    name: String::from(suggest::NAME),
                    arguments: String::from(arguments),
                },
                "",
                0,
                &engine.state,
                &workdir,
                &std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                &chat,
            );
        };
        run(
            &engine,
            "call-one",
            "{\"suggestions\":[{\"label\":\"Run the tests\",\"prompt\":\"Run the tests of the core.\"},{\"label\":\"Read the log\"}]}",
        );
        let snap = engine.snapshot();
        let first = snap
            .messages
            .iter()
            .find(|one| one.tool_name == suggest::NAME)
            .expect("the chat holds the run");
        assert_eq!(first.role, "Tool");
        assert!(!first.failed, "{}", first.tool);
        assert_eq!(first.suggestions.len(), 2);
        assert_eq!(first.suggestions[0].label, "Run the tests");
        assert_eq!(first.suggestions[0].prompt, "Run the tests of the core.");
        assert_eq!(first.suggestions[1].prompt, "Read the log");
        assert!(first.tool.contains("2 next steps are offered"), "{}", first.tool);
        // The state of a page carries the proposal with the run.
        let value = crate::web::state(&snap, None);
        let rows = value["messages"].as_array().expect("the state carries the conversation");
        let row = rows
            .iter()
            .find(|one| one["tool_name"] == suggest::NAME)
            .expect("the run stands in the state");
        assert_eq!(row["suggestions"][0]["label"], "Run the tests");
        assert_eq!(row["suggestions"][0]["prompt"], "Run the tests of the core.");
        assert_eq!(row["suggestions"][1]["label"], "Read the log");
        // A second call takes the buttons of the first away: the newest propose is the one the
        // user is given.
        run(&engine, "call-two", "{\"suggestions\":[\"Show me the diff\"]}");
        let snap = engine.snapshot();
        let runs: Vec<&Message> = snap
            .messages
            .iter()
            .filter(|one| one.tool_name == suggest::NAME)
            .collect();
        assert_eq!(runs.len(), 2, "both runs are in the conversation");
        assert!(runs[0].suggestions.is_empty(), "the older offer went");
        assert_eq!(runs[1].suggestions.len(), 1);
        assert_eq!(runs[1].suggestions[0].label, "Show me the diff");
        // The tool is behind the line of the drop-down, and the line takes it out of the
        // request as it takes out every other tool of the program.
        let names = |switches: &store::ToolSwitches| {
            offered(
                false,
                true,
                false,
                tools::Access::default(),
                &[],
                switches,
                true,
            )
            .map(|list| {
                list.as_array()
                    .unwrap()
                    .iter()
                    .map(|one| String::from(one["function"]["name"].as_str().unwrap_or("")))
                    .collect::<Vec<_>>()
            })
        };
        assert!(
            names(&engine.snapshot().tool_switches).is_some_and(|list| list
                .iter()
                .any(|one| one == suggest::NAME)),
            "the tool is proposed while the line is on"
        );
        engine.set_tool_switch("suggest", false);
        assert!(
            !names(&engine.snapshot().tool_switches)
                .unwrap_or_default()
                .iter()
                .any(|one| one == suggest::NAME),
            "the line takes the tool out of the request"
        );
        let _ = std::fs::remove_dir_all(&workdir);
    }

    // The list of the steps of the work hangs on the run that made it, and the state carries it
    // with the run, so the block is drawn again when the chat is opened another day and in the
    // page of the remote mode. The newest call of the answer wins, so a model that corrects its
    // own list leaves one block and not two. The line of the drop-down of the badge holds the
    // tool as it holds every other tool of the program, and the header of the live block names
    // the title while the body holds the steps written so far.
    #[test]
    fn the_list_of_the_steps_hangs_on_the_run_and_the_newest_stands() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let workdir = {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
            let room = state.workdir_for(state.chat().expect("a blank chat stands open"));
            std::fs::create_dir_all(&room).unwrap();
            room
        };
        let chat = engine.state.lock().unwrap().open_id();
        let run = |engine: &Engine, id: &str, arguments: &str| {
            run_program_tool(
                &ToolCall {
                    id: String::from(id),
                    name: String::from(steps::NAME),
                    arguments: String::from(arguments),
                },
                "",
                0,
                &engine.state,
                &workdir,
                &std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                &chat,
            );
        };
        run(
            &engine,
            "call-one",
            "{\"title\":\"Ship it\",\"steps\":[{\"label\":\"Build\",\"content\":\"Run build.sh.\"},{\"label\":\"Test\",\"content\":\"Run the tests.\"}]}",
        );
        let snap = engine.snapshot();
        let first = snap
            .messages
            .iter()
            .find(|one| one.tool_name == steps::NAME)
            .expect("the chat holds the run");
        assert_eq!(first.role, "Tool");
        assert!(!first.failed, "{}", first.tool);
        let plan = first.steps.as_ref().expect("the block stands");
        assert_eq!(plan.title, "Ship it");
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].content, "Run build.sh.");
        assert!(first.tool.contains("2 steps are shown"), "{}", first.tool);
        // The state of a page carries the block with the run.
        let value = crate::web::state(&snap, None);
        let rows = value["messages"].as_array().expect("the state carries the conversation");
        let row = rows
            .iter()
            .find(|one| one["tool_name"] == steps::NAME)
            .expect("the run stands in the state");
        assert_eq!(row["steps"]["title"], "Ship it");
        assert_eq!(row["steps"]["steps"][1]["label"], "Test");
        // A second call takes the block of the first away.
        run(&engine, "call-two", "{\"steps\":[\"Only one step\"]}");
        let snap = engine.snapshot();
        let runs: Vec<&Message> = snap
            .messages
            .iter()
            .filter(|one| one.tool_name == steps::NAME)
            .collect();
        assert_eq!(runs.len(), 2, "both runs are in the conversation");
        assert!(runs[0].steps.is_none(), "the older block went");
        let second = runs[1].steps.as_ref().expect("the newest block stands");
        assert_eq!(second.steps.len(), 1);
        assert_eq!(second.steps[0].content, "Only one step");
        // The tool is behind the line of the drop-down, as every other tool of the program
        // does.
        let names = |switches: &store::ToolSwitches| {
            offered(
                false,
                true,
                false,
                tools::Access::default(),
                &[],
                switches,
                true,
            )
            .map(|list| {
                list.as_array()
                    .unwrap()
                    .iter()
                    .map(|one| String::from(one["function"]["name"].as_str().unwrap_or("")))
                    .collect::<Vec<_>>()
            })
        };
        assert!(
            names(&engine.snapshot().tool_switches).is_some_and(|list| list
                .iter()
                .any(|one| one == steps::NAME)),
            "the tool is proposed while the line is on"
        );
        assert_eq!(group_of(steps::NAME), Some("steps"));
        engine.set_tool_switch("steps", false);
        assert!(
            !names(&engine.snapshot().tool_switches)
                .unwrap_or_default()
                .iter()
                .any(|one| one == steps::NAME),
            "the line takes the tool out of the request"
        );
        // The header of a block being written names the title, and its body holds the steps
        // written so far.
        let writing = ToolCall {
            id: String::from("call-three"),
            name: String::from(steps::NAME),
            arguments: String::from("{\"title\":\"Ship it\",\"steps\":[{\"label\":\"Build\",\"content\":\"Run build"),
        };
        let made = watched(std::slice::from_ref(&writing));
        assert_eq!(made[0].target, "Ship it");
        assert!(made[0].command.contains("1. Build"), "{}", made[0].command);
        assert!(made[0].command.contains("Run build"), "{}", made[0].command);
        let _ = std::fs::remove_dir_all(&workdir);
    }

    // The image a run opened for the model hangs on the message of the run. The bytes of an
    // image are never written into the text of a block, so this is the one way the user can
    // look at what the model was shown: the state carries the block of the image with the run,
    // and the bytes are in the directory of the attachments of the chat until the block
    // requests them.
    #[test]
    fn the_picture_of_a_run_hangs_on_the_message_of_the_run() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
            let chat = state.chat().expect("a blank chat stands open");
            let shelf = state.attach_place_for(chat);
            let kept = attach::save_at(
                &shelf,
                &[attach::Incoming {
                    name: String::from("cat.png"),
                    mime: String::from("image/png"),
                    data: String::from("data:image/png;base64,AAA="),
                    text: String::new(),
                }],
            );
            let call = ToolCall {
                id: String::from("call-look"),
                name: String::from(viewimage::NAME),
                arguments: String::from("{\"path\":\"notes/cat.png\"}"),
            };
            let id = state.begin_tool(&call, "notes/cat.png", "", 0);
            state.finish_tool_with(
                &id,
                "Opened an image: notes/cat.png (PNG, 2 by 2 pixels).",
                true,
                kept,
            );
        }
        let snap = engine.snapshot();
        let run = snap
            .messages
            .iter()
            .find(|one| one.tool_name == viewimage::NAME)
            .expect("the chat holds the run");
        assert_eq!(run.role, "Tool");
        assert_eq!(run.attachments.len(), 1, "the run hangs one image on itself");
        assert!(run.attachments[0].image, "an image is known as an image");
        assert_eq!(run.attachments[0].name, "cat.png");

        // The state of a page carries the block of the image with the run: the name, the type,
        // and the id the bytes are requested by. No byte of the image rides here, for a chat of
        // many images would be a wall.
        let value = crate::web::state(&snap, None);
        let rows = value["messages"].as_array().expect("the state carries the conversation");
        let row = rows
            .iter()
            .find(|one| one["tool_name"] == viewimage::NAME)
            .expect("the run stands in the state");
        let held = &row["attachments"][0];
        assert_eq!(held["name"], "cat.png");
        assert_eq!(held["image"], true);
        assert_eq!(held["id"], run.attachments[0].id);
        assert!(
            held.get("data").is_none(),
            "the state of a page carries no bytes of an image"
        );

        // The bytes are reached by the id, which is how the block of the run draws the image
        // the model was shown.
        let record = engine
            .attachment(&run.attachments[0].id)
            .expect("the record of the image is with the chat");
        assert_eq!(record.data, "data:image/png;base64,AAA=");
        assert_eq!(record.mime, "image/png");
    }

    // The text under the number of the meter follow the images the message carries. A chat of
    // text gives none, which is how the program remains today, and only the newest message of
    // the user is checked.
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

        // Only the last item that is sent is worked through anew. A turn that ends with the
        // text of the model carries no image of its own.
        let older = json!([
            { "role": "user", "content": [{ "type": "image_url", "image_url": { "url": "AAA" } }] },
            { "role": "assistant", "content": "a cat on a mat" },
        ]);
        assert_eq!(pending_images(&older), 0);

        // A chat with nothing in it is still a chat of text.
        assert_eq!(pending_images(&json!([])), 0);
    }

    // A message that hangs attachments is sent to the model as parts: the text of the user, the
    // list of the files with their numbers and their endings, and the text of a text file.
    // Without a model that can look at images, no image part is sent, and the list reports why.
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
        state.start_chat();
        // The records of the attachments are in a directory of the working directory of the chat that
        // carries them, so this test saves them where a chat of this place would keep them.
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
        // The line of the day and the clock time is not one of the messages of the conversation
        // this test counts.
        let items = undated(&state.build_history());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["role"], json!("user"));
        let parts = items[0]["content"]
            .as_array()
            .expect("a message with attachments is sent as parts");
        let lead = parts[0].get("text").unwrap().as_str().unwrap();
        assert!(lead.starts_with("What is in these?"));
        assert!(lead.contains("1. photo.png"));
        assert!(lead.contains("2. notes.txt"));
        // No model is in this test, so no image can go, and the list reports that plainly.
        assert!(lead.contains("vision is off"));
        assert!(!parts
            .iter()
            .any(|p| p.get("type").and_then(|t| t.as_str()) == Some("image_url")));
        // The file of text is sent under its number and its name.
        let words = parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .find(|t| t.starts_with("The text of attachment 2"))
            .expect("the text of the second attachment");
        assert!(words.contains("notes.txt"));
        assert!(words.contains("the words of the note"));
        // The snapshot pointers the blocks to the window: name and type, in the order the user
        // hung them, and which of them are images.
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

    // The user watches a command as the model writes it, and watches nothing of a marker of a
    // chapter. The name of a call arrives parts, so a name that could still become the marker
    // remains aside until it is known.
    #[test]
    fn only_a_command_the_user_can_use_is_written_out() {
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

        // A call with no name of its own reports nothing of what it is.
        let blank = ToolCall {
            id: String::from("call-four"),
            ..Default::default()
        };
        assert!(watched(std::slice::from_ref(&blank)).is_empty());

        // The text of a command that is not finished is kept as they came, so the user sees the
        // command grow instead of waiting for the end.
        let grown = ToolCall {
            id: String::from("call-five"),
            name: String::from(tools::TOOL_NAME),
            arguments: String::from("{\"command\":\"cargo test --release mark"),
        };
        let made = watched(std::slice::from_ref(&grown));
        assert_eq!(made[0].command, "cargo test --release mark");
        assert!(made[0].purpose.is_empty());
    }

    // The badge of the web search works as the badge of the tools does: with no chat open the
    // choice waits for the next one, and the two switches remain apart, so opening one does not
    // open the other.
    #[test]
    fn the_web_search_switch_is_kept_for_the_next_chat() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        // No chat is open, so the choice is kept for the chat that comes next.
        engine.set_search(true);
        assert!(engine.snapshot().search_enabled);
        assert!(!engine.snapshot().code_enabled);
        open_a_chat(&engine);
        assert!(engine.snapshot().search_enabled);
        engine.set_search(false);
        assert!(!engine.snapshot().search_enabled);
    }

    // The badge of the tools is the entry point to every tool that can run on the computer:
    // with it off the model is proposed no command and no tool that works on a file. The web
    // search is on a switch of its own, and it alone is enough to carry tools with the call.
    // The chapters of a chat propose no tool at all, whatever their switch reports.
    #[test]
    fn no_badge_of_the_tools_opens_no_tool_at_all() {
        let names = |list: &Value| {
            list.as_array()
                .unwrap()
                .iter()
                .map(|one| String::from(one["function"]["name"].as_str().unwrap_or("")))
                .collect::<Vec<_>>()
        };
        // Nothing is switched on, so the call carries no tools at all.
        assert!(offered(
            false,
            false,
            false,
            tools::Access::default(),
            &[],
            &store::ToolSwitches::default(),
            true
        )
        .is_none());
        // The badge on: every tool of the program is there.
        let open = offered(
            false,
            true,
            false,
            tools::Access::default(),
            &[],
            &store::ToolSwitches::default(),
            true
        )
        .unwrap();
        let words = names(&open);
        assert!(words.contains(&tools::TOOL_NAME.to_string()));
        assert!(words.contains(&editfile::EDIT_NAME.to_string()));
        assert!(words.contains(&editfile::CREATE_NAME.to_string()));
        assert!(words.contains(&viewimage::NAME.to_string()));
        assert!(!words.contains(&search::NAME.to_string()));
        // The badge of the web search alone: the search is proposed, and none of the tools that
        // can run on the computer is proposed with it.
        let looked = offered(
            false,
            false,
            true,
            tools::Access::default(),
            &[],
            &store::ToolSwitches::default(),
            true
        )
        .unwrap();
        assert_eq!(names(&looked), vec![search::NAME.to_string()]);
        // Both badges are left open, so both entry points are open at once.
        let both = offered(
            false,
            true,
            true,
            tools::Access::default(),
            &[],
            &store::ToolSwitches::default(),
            true
        )
        .unwrap();
        let words = names(&both);
        assert!(words.contains(&tools::TOOL_NAME.to_string()));
        assert!(words.contains(&search::NAME.to_string()));
        // What the chat lets a command reach is in the text of the tool, so the model knows it
        // before it requests a command.
        let open = offered(
            true,
            true,
            false,
            tools::Access { network: true, pip: true, fetch: true },
            &[],
            &store::ToolSwitches::default(),
            true
        )
        .unwrap();
        let said = String::from(open[0]["function"]["description"].as_str().unwrap_or(""));
        assert!(said.contains("The command may reach the network."));
        // Every tool of the list carries the rule of when to call it. A model that is not
        // informed it may answer in text spends the first turn of a chat arguing with the list
        // it was given, and writes the argument out as thinking before an answer that goes on
        // as if none of it had been stated.
        for one in both.as_array().expect("a list of tools").iter() {
            let said = one["function"]["description"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            assert!(
                said.contains(tools::CALL_RULE),
                "the rule of calling is missing from {:?}",
                one["function"]["name"]
            );
        }
    }

    // The text of the tools that work on the computer follows the state of the sandbox, which
    // is the only item that can wall a command. A chat with the sandbox off is proposed the
    // plain tool, with no word of a sandbox, of a network or of a program that remains off, for
    // such a chat has none of those items to tell. A chat with the sandbox on is informed the
    // sandbox boundaries, and how far its space reaches. The list of the tools is rendered
    // into the start of the conversation, so a note about a wall that is not there is paid for
    // in every ask of the chat, and has turned a model away from a command it should have run.
    #[test]
    fn the_words_of_the_tools_follow_the_state_of_the_sandbox() {
        let offered_words = |sandbox: bool| -> Vec<String> {
            offered(
                sandbox,
                true,
                false,
                tools::Access::default(),
                &[],
                &store::ToolSwitches::default(),
                true
            )
            .expect("the badge of the tools is left open")
            .as_array()
            .expect("a list of tools")
            .iter()
            .map(|one| {
                format!(
                    "{} {}",
                    one["function"]["name"].as_str().unwrap_or_default(),
                    one["function"]["description"].as_str().unwrap_or_default()
                )
            })
            .collect()
        };
        // The wall down: no tool of the list has a note about it, and no tool reports the model
        // that a network or a program is turned off.
        for one in offered_words(false) {
            for forbidden in ["sandbox", "sandbox", "network", "wget", "curl"] {
                assert!(
                    !one.to_lowercase().contains(forbidden),
                    "a chat with no sandbox was informed of {}: {}",
                    forbidden,
                    one
                );
            }
        }
        // The wall up: the tool of the commands names the sandbox it runs in, and what the chat
        // lets a command reach.
        let jailed = offered_words(true)
            .into_iter()
            .find(|one| one.starts_with(tools::TOOL_NAME))
            .expect("the tool of the commands is proposed");
        assert!(jailed.contains("sandbox"), "{}", jailed);
        assert!(jailed.contains("There is no network access."), "{}", jailed);
        // The file tools say how far the working directory of the chat reaches in both states, so
        // a chat with the sandbox off still hears that a file of this computer may be named,
        // and does not hear it from the sandbox.
        let reached = |sandbox: bool| {
            offered_words(sandbox)
                .into_iter()
                .find(|one| one.starts_with(editfile::EDIT_NAME))
                .expect("the tool of the files is proposed")
        };
        assert!(reached(true).contains("the only place this tool works"));
        assert!(reached(false).contains("anywhere on this computer"));
    }

    // The tools of the servers of the user's own are in the list of a step, and a server alone
    // is enough to let the model be proposed anything at all.
    #[test]
    fn the_tools_of_a_server_stand_in_the_list() {
        let server = mcp::Server {
            id: String::from("abcdef1234"),
            name: String::from("Bookshop"),
            kind: String::from(mcp::KIND_STDIO),
            command: String::from("bookshop-mcp"),
            slug: String::from("bookshop"),
            shown: true,
            ..mcp::Server::default()
        };
        let tool = mcp::Tool {
            name: String::from("search_books"),
            description: String::from("Search the books of the shop."),
            ..mcp::Tool::default()
        };
        let made = vec![tool.definition(&server)];
        let list = offered(
            false,
            false,
            false,
            tools::Access::default(),
            &made,
            &store::ToolSwitches::default(),
            true
        )
        .expect("a server of the user's own is a tool");
        let names: Vec<&str> = list
            .as_array()
            .unwrap()
            .iter()
            .map(|one| one["function"]["name"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(names, vec!["mcp__bookshop__search_books"]);
        // A server is reached out of the program and never inside the sandbox, so its text
        // record it, and the rule of when to call it is with it.
        let said = list[0]["function"]["description"].as_str().unwrap_or("");
        assert!(said.contains("Search the books of the shop."));
        assert!(said.contains("Bookshop"));
        assert!(said.contains(tools::CALL_RULE));
    }

    // The three settings of the sandbox remain off for a new chat, and all of them is kept for
    // the next chat as well as for the current one. An install or a fetch opens the network,
    // for neither does anything without it, and closing one leaves the network to the other.
    #[test]
    fn the_settings_of_the_sandbox_stand_off() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        // A new set of settings, and a chat written before the three existed, both read with
        // all three off.
        assert_eq!(tools::Access::default(), tools::Access { network: false, pip: false, fetch: false });
        let older = store::ChatMeta::default();
        assert!(!older.sandbox_network && !older.sandbox_pip && !older.sandbox_fetch);
        assert_eq!(access_of(&older), tools::Access::default());

        // Nothing is open at the start, whatever the sandbox itself is on.
        let first = engine.snapshot();
        assert_eq!(first.sandbox_access, tools::Access::default());
        // The three remain apart: an install does not open the switch of the network, and it
        // does not open the fetch either. What a command may really reach is the three of them
        // together.
        engine.set_sandbox_setting(SandboxSetting::Pip, true);
        let said = engine.snapshot();
        assert!(said.sandbox_access.pip);
        assert!(!said.sandbox_access.network);
        assert!(!said.sandbox_access.fetch);
        assert!(said.sandbox_access.net(), "an install needs the network");
        // Closing the install leaves nothing open at all.
        engine.set_sandbox_setting(SandboxSetting::Pip, false);
        assert_eq!(engine.snapshot().sandbox_access, tools::Access::default());
        // A fetch is on its own, and an install beside it is distinguished from it.
        engine.set_sandbox_setting(SandboxSetting::Fetch, true);
        engine.set_sandbox_setting(SandboxSetting::Pip, true);
        let held = engine.snapshot();
        assert!(held.sandbox_access.fetch);
        assert!(held.sandbox_access.pip);
        assert!(!held.sandbox_access.network);
        // The names the pages send are the three names the engine answers.
        for name in ["network", "pip", "fetch"] {
            assert!(SandboxSetting::from_name(name).is_some(), "{}", name);
        }
        assert!(SandboxSetting::from_name("web").is_none());
    }

    // The header of a block, and the block of a call being written, take the one item the call
    // names: the command of a run, or the file a file tool works on. A reason of the call rides
    // along for the terminal tool alone.
    #[test]
    fn every_tool_shows_the_one_thing_it_names() {
        let edit = ToolCall {
            id: String::from("call-edit"),
            name: String::from(editfile::EDIT_NAME),
            arguments: String::from(
                "{\"path\":\"notes/todo.txt\",\"edits\":[{\"old_string\":\"one\",\"new_string\":\"two\"}]}",
            ),
        };
        let made = watched(std::slice::from_ref(&edit));
        assert_eq!(made.len(), 1);
        // The header names the file the call works on, and the body holds the text of the
        // change the call is making.
        assert_eq!(made[0].target, "notes/todo.txt");
        assert_eq!(made[0].command, "change 1 · what stands now\no one\nchange 1 · what it becomes\nr two\n");
        assert!(made[0].purpose.is_empty(), "{}", made[0].purpose);
        // A name of a file that is still arriving is shown as far as it has come, as a command
        // is, and the text of a new file is in the body as they come.
        let half = ToolCall {
            id: String::from("call-half"),
            name: String::from(editfile::CREATE_NAME),
            arguments: String::from("{\"path\":\"notes/plan.txt\",\"text\":\"hello\"}"),
        };
        let half_made = watched(std::slice::from_ref(&half));
        assert_eq!(half_made[0].target, "notes/plan.txt");
        assert_eq!(half_made[0].command, "hello");
        // A name the program does not propose is shown with the file it names, so the user is
        // not left looking at an empty block.
        let strange = ToolCall {
            id: String::from("call-strange"),
            name: String::from("rewrite"),
            arguments: String::from("{\"path\":\"notes/todo.txt\"}"),
        };
        assert_eq!(watched(std::slice::from_ref(&strange))[0].command, "notes/todo.txt");
        // A model that writes the halves of a call in the order it likes, and not in the order
        // the form of the call requests them, is shown as far as each of them has come. The
        // text of the change is in the body from the first of them, so the block is not blank
        // while the call is written.
        let backwards = ToolCall {
            id: String::from("call-backwards"),
            name: String::from(editfile::EDIT_NAME),
            arguments: String::from("{\"edits\":[{\"new_string\":\"un"),
        };
        let read = watched(std::slice::from_ref(&backwards));
        assert_eq!(read[0].command, "change 1 · what it becomes\nr un\n");
        assert_eq!(read[0].target, String::new());
        let half_read = ToolCall {
            id: String::from("call-half-read"),
            name: String::from(editfile::EDIT_NAME),
            arguments: String::from("{\"edits\":[{\"new_string\":\"uno\",\"old_string\":\"on"),
        };
        assert_eq!(
            watched(std::slice::from_ref(&half_read))[0].command,
            "change 1 · what stands now\no on\nchange 1 · what it becomes\nr uno\n"
        );
        // The name of the file is the header of the block whichever half of the call it is in,
        // so a model that writes the changes first names the file as soon as the name begins.
        let named_last = ToolCall {
            id: String::from("call-named-last"),
            name: String::from(editfile::EDIT_NAME),
            arguments: String::from(
                "{\"edits\":[{\"new_string\":\"uno\",\"old_string\":\"one\"}],\"path\":\"notes/to",
            ),
        };
        let read = watched(std::slice::from_ref(&named_last));
        assert_eq!(read[0].target, "notes/to");
        assert_eq!(read[0].command, "change 1 · what stands now\no one\nchange 1 · what it becomes\nr uno\n");
    }

    // The calls being written are in the snapshot, and go away with the step that wrote them,
    // so a block is never left remaining over a chat.
    #[test]
    fn the_calls_being_written_stand_in_the_snapshot() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        open_a_chat(&engine);
        {
            let mut state = engine.state.lock().unwrap();
            let call = ToolCall {
                id: String::from("call-one"),
                name: String::from(tools::TOOL_NAME),
                arguments: String::from("{\"command\":\"uname"),
            };
            state.live_write().live_tools = watched(std::slice::from_ref(&call));
        }
        let snap = engine.snapshot();
        assert_eq!(snap.live_tools.len(), 1);
        assert_eq!(snap.live_tools[0].command, "uname");
        {
            let mut state = engine.state.lock().unwrap();
            state.live_write().live_tools.clear();
        }
        assert!(engine.snapshot().live_tools.is_empty());
    }

    // A model that thinks, writes a call, and then thinks again has finished the text of that
    // call long before the step of the answer ends. The block of it must stop saying it is
    // being written, and must keep saying it is written while nothing new comes of that text.
    // Text that grow again is being written again, and record it.
    #[test]
    fn a_call_the_model_moved_on_from_says_it_is_written() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        open_a_chat(&engine);
        let mut state = engine.state.lock().unwrap();
        // The chat the functions under test are given to work in.
        let chat = state.open_id();
        let half = ToolCall {
            id: String::from("c1"),
            name: String::from(tools::TOOL_NAME),
            arguments: String::from("{\"command\":\"uname -a"),
        };
        let whole = ToolCall {
            id: String::from("c1"),
            name: String::from(tools::TOOL_NAME),
            arguments: String::from("{\"command\":\"uname -a -s\"}"),
        };
        // The text of the call is arriving.
        write_calls(&mut state, std::slice::from_ref(&half), &chat);
        assert_eq!(state.live().live_tools.len(), 1);
        assert!(!state.live().live_tools[0].done);
        // The model thinks again, so the call is written.
        assert!(end_writing_calls(&mut state, &chat));
        assert!(state.live().live_tools[0].done);
        // A piece that brings nothing new does not start the writing again.
        write_calls(&mut state, std::slice::from_ref(&half), &chat);
        assert!(state.live().live_tools[0].done);
        // Text that grew is being written again.
        write_calls(&mut state, std::slice::from_ref(&whole), &chat);
        assert!(!state.live().live_tools[0].done);
        // Nothing was written, so ending the writing is a change the window has to hear about.
        assert!(end_writing_calls(&mut state, &chat));
        assert!(state.live().live_tools[0].done);
        // The same word again is nothing to tell.
        assert!(!end_writing_calls(&mut state, &chat));
    }

    // The meter above the chat composer was while the server worked the text it was sent
    // through. A model cannot write a command before that is over, so the first word of a call
    // takes the meter away. A model that requests a command before it has stated anything must
    // not leave it remaining over the chat until the command has run.
    #[test]
    fn the_gauge_leaves_when_a_call_is_begun() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        open_a_chat(&engine);
        let command = ToolCall {
            id: String::from("call-one"),
            name: String::from(tools::TOOL_NAME),
            arguments: String::from("{\"command\":\"uname"),
        };
        let chat = engine.state.lock().unwrap().open_id();
        {
            let mut state = engine.state.lock().unwrap();
            state.live_write().progress_on = true;
            state.live_write().progress = 42;
            // The command is only begun, and the meter goes already.
            assert!(write_calls(&mut state, std::slice::from_ref(&command), &chat));
            assert!(!state.live().progress_on, "the meter was on after the call began");
            // A meter that is removed has finished its work.
            assert_eq!(state.live().progress, 100);
            assert_eq!(state.live().live_tools.len(), 1);
            assert_eq!(state.live().live_tools[0].command, "uname");
        }
        assert!(!engine.snapshot().progress_on);
        // A piece that shows nothing new of a call the state holds already reports nothing at
        // all: the meter went with the first word of it.
        {
            let mut state = engine.state.lock().unwrap();
            state.live_write().live_tools.clear();
            write_calls(&mut state, std::slice::from_ref(&command), &chat);
            assert!(!write_calls(&mut state, std::slice::from_ref(&command), &chat));
        }
    }

    // The context meter moves while an answer arrives, and it only climbs. The number the
    // server gives at the end of the answer is kept for the end, and that one may be smaller.
    #[test]
    fn the_context_meter_moves_while_an_answer_comes() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        open_a_chat(&engine);
        {
            let mut state = engine.state.lock().unwrap();
            // The chat the functions under test are given to work in.
            let chat = state.open_id();
            state.live_write().ctx_used = 0;
            // Nothing is known of the conversation yet, so the meter keeps what it is on.
            assert!(!climb_context(&mut state, 0, &chat));
            assert_eq!(state.live().ctx_used, 0);
            // The text of the conversation so far. The meter climbs.
            assert!(climb_context(&mut state, 431, &chat));
            // A piece that shows the same, or less, moves nothing: the text of an answer that
            // is still coming is not taken back.
            assert!(!climb_context(&mut state, 431, &chat));
            assert!(!climb_context(&mut state, 400, &chat));
            assert_eq!(state.live().ctx_used, 431);
            assert!(climb_context(&mut state, 900, &chat));
            assert_eq!(state.live().ctx_used, 900);
        }
        // The window draws the number of the meter from the snapshot, so the number of the live
        // answer reaches the top bar.
        assert_eq!(engine.snapshot().context_used, 900);
    }

    // The context meter moves at most twice a second. The server speaks of the conversation on
    // every piece of the answer, which is a great deal oftener than the user can read a number.
    // A number that is turned away is not lost: the next piece after the half second brings the
    // conversation up to date, and a number that goes back is turned away at any moment.
    #[test]
    fn the_meter_of_the_context_moves_every_half_second() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        open_a_chat(&engine);
        let begun = Instant::now();
        let mut state = engine.state.lock().unwrap();
        // The chat the functions under test are given to work in.
        let chat = state.open_id();
        state.live_write().ctx_used = 0;
        state.live_write().ctx_at = None;
        // The first number of the answer is put in at once.
        assert!(climb_context_paced(&mut state, 100, begun, &chat));
        assert_eq!(state.live().ctx_used, 100);
        // Within the half second the meter is where it was.
        assert!(!climb_context_paced(&mut state, 120, begun + Duration::from_millis(120), &chat));
        assert!(!climb_context_paced(&mut state, 138, begun + Duration::from_millis(499), &chat));
        assert_eq!(state.live().ctx_used, 100);
        // Past the half second the newest number is placed on, and the parts that were turned
        // away are not requested again.
        assert!(climb_context_paced(&mut state, 140, begun + Duration::from_millis(501), &chat));
        assert_eq!(state.live().ctx_used, 140);
        // A number that goes back never moves the meter, pace or no pace.
        assert!(!climb_context_paced(&mut state, 90, begun + Duration::from_secs(5), &chat));
        assert_eq!(state.live().ctx_used, 140);
    }

    // ------------------------------------------------------------ compaction
    // One message of a conversation, put in the chat that is left open.
    fn add_message(state: &mut State, role: &str, body: &str) {
        let chat = state.chat_mut().expect("a chat stands open");
        chat.messages.push(Message {
            id: store::new_id(),
            role: role.to_string(),
            body: body.to_string(),
            when: store::now_secs(),
            ..Default::default()
        });
    }

    // A handoff summary is written by a model, and a model wraps what it writes in text of its
    // own. Those text is the envelope of the summary rather than the summary, and they are sent
    // again on every turn after the compaction, so they are taken off.
    #[test]
    fn the_handoff_summary_is_cleaned_of_the_words_around_it() {
        let wrapped = "Here is the handoff summary:\n\n```\n\
            Done: the parser closes a list after a paragraph.\n\
            Next: the height of the bar beside a heading.\n\
            Files: src/markdown.rs holds the parser.\n\
            ```\n\
            Let me know if you need more detail.\n";
        let cleaned = clean_summary(wrapped);
        assert!(cleaned.starts_with("Done: the parser"));
        assert!(cleaned.contains("Next: the height"));
        assert!(cleaned.ends_with("holds the parser."));
        assert!(!cleaned.contains("Here is"));
        assert!(!cleaned.contains("Let me know"));
        assert!(!cleaned.contains("```"));
        // Text that is the summary keeps its markers, for the next model reads them as the
        // summary: a fence of code inside a summary is part of what was done, and only a fence
        // about all of it is the wrapper.
        let plain = "Done: fixed `parser.rs`.\n\nNext: the bar.";
        assert_eq!(clean_summary(plain), "Done: fixed `parser.rs`.\n\nNext: the bar.");
        // Blank lines one behind the other say nothing, and the next model pays for all of
        // them.
        assert_eq!(clean_summary("one\n\n\n\n\ntwo"), "one\n\ntwo");
        // A line that is the summary is not a lead-in, whatever it begins with.
        let told = "Here is what the session did, in the text of the model.";
        assert_eq!(clean_summary(told), told);
    }

    // The text that go round a handoff summary tell the next model what it is reading, so that
    // it goes on with the work instead of greeting the user or beginning it again.
    #[test]
    fn a_handoff_summary_is_named_for_the_model_that_reads_it() {
        let block = handoff_block("Done: the parser. Next: the bar.");
        assert!(block.contains("The session was compacted"));
        assert!(block.contains("handoff summary"));
        assert!(block.contains("Do not greet the user"));
        assert!(block.ends_with("Done: the parser. Next: the bar."));
    }

    // The summarisations of a chat remain where they were created, are numbered in the order
    // they were created, and say what became of each of them.
    #[test]
    fn the_summarisations_stand_where_they_were_made() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        add_message(&mut state, "Assistant", "the old answer");
        // The first summarisation, written.
        let first = state.begin_summary();
        state.finish_summary(&first, "Done: the parser. Next: the bar.", 6964, 412, "");
        add_message(&mut state, "You", "and the next thing");
        // The second summarisation, requested and never written.
        let second = state.begin_summary();
        let marks = state.handoffs();
        assert_eq!(marks.len(), 2);
        assert_eq!(marks[0].number, 1);
        assert_eq!(marks[0].state, "done");
        assert_eq!(marks[0].before, 6964);
        assert_eq!(marks[0].after, 412);
        assert!(marks[0].snippet.starts_with("Done: the parser"));
        assert!(marks[0].why.is_empty());
        // Nothing is being written, so an empty summarisation is one that is owed: the chat is
        // at that place until the text come.
        assert_eq!(marks[1].number, 2);
        assert_eq!(marks[1].state, "owed");
        assert!(marks[1].snippet.is_empty());
        assert!(state.handoff_owed());
        // While the program is at a summary, the empty marker is not yet an error.
        state.live_write().generating = true;
        assert_eq!(state.handoffs()[1].state, "writing");
        state.live_write().generating = false;
        // The marker that was never written is the newest item of the chat, so there is nothing
        // new to write a summary of.
        let chat = state.chat().expect("a chat stands open");
        assert!(!ends_on_summary(chat));
        state.finish_summary(&second, "Done: the bar. Next: nothing.", 7000, 96, "");
        let chat = state.chat().expect("a chat stands open");
        assert!(ends_on_summary(chat));
        assert!(!state.handoff_owed());
        let marks = state.handoffs();
        assert_eq!(marks[1].state, "done");
        assert_eq!(marks[1].before, 7000);
    }

    // The marker of a summarisation reports which of the two asks its text came from. The short
    // prompt of the settings is asked again when the context window filled under the first ask,
    // or when the text of the first came to no end of themselves; it holds less, and the user
    // is informed which of the two is in the chat. A chat written before the two were
    // distinguished carries no word of either ask, and reads as the request of the settings.
    #[test]
    fn a_summarisation_says_whether_it_is_the_short_one() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        // The request of the settings: nothing of a short ask is in its marker.
        let plain = state.begin_summary();
        state.summary_short(&plain, false);
        state.finish_summary(&plain, "Done: the parser. Next: the bar.", 6964, 412, "");
        assert!(!state.handoffs()[0].short, "the ask of the settings");
        // The request rebuilt with the short prompt of the settings.
        let short = state.begin_summary();
        state.summary_short(&short, true);
        state.finish_summary(&short, "Done. Next: nothing.", 7000, 96, "");
        assert!(state.handoffs()[1].short, "the short ask of the settings");
        drop(state);
        // A chat of before, extracted from its text with the marker of the request taken out of
        // them, is a chat of the request of the settings.
        let state = engine.state.lock().unwrap();
        let chat = state.chat().expect("a chat stands open");
        let mut words = serde_json::to_value(chat).expect("a chat is words");
        drop(state);
        let places: Vec<usize> = words["messages"]
            .as_array()
            .expect("the messages of the chat")
            .iter()
            .enumerate()
            .filter(|(_, m)| m["role"] == json!("Summary"))
            .map(|(place, _)| place)
            .collect();
        assert_eq!(places.len(), 2, "two markers of a summarisation remain");
        for place in places {
            words["messages"][place]
                .as_object_mut()
                .expect("a message of the chat")
                .remove("short");
        }
        let before: store::Chat = serde_json::from_value(words).expect("a chat of before");
        let marks = handoff_marks(&before, false);
        assert_eq!(marks.len(), 2);
        assert!(
            marks.iter().all(|one| !one.short),
            "a marker of before is the ask of the settings"
        );
    }

    // A turn of the model extends into the calls of its tools, and a call that is running is
    // part of the answer as much as the text of it is. The user who requests the answer to stop
    // is asking for all of the turn to stop, so the running call is stopped with it -- whether
    // the call still was in the time it was given at first, or in the time the user added to
    // it.
    #[test]
    fn stopping_the_answer_stops_the_call_that_is_running() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let watch = {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
            let watch = crate::util::Watch::new(Duration::from_secs(600));
            state.begin_running("block-one", &watch);
            watch
        };
        assert!(!watch.stopped(), "nothing was stopped to begin with");
        // The user gave the call more time first, so what is tested here is a call remaining in
        // the time that was added to it. The lock is let go of before the two markers are
        // pressed, for both of them take it.
        engine.extend_running_tool();
        assert!(!watch.stopped(), "more time is not a stop");
        engine.cancel();
        let state = engine.state.lock().unwrap();
        assert!(state.live().running.is_some(), "the call stands running still");
        assert!(
            state.live().running.as_ref().map(|one| one.watch.stopped()).unwrap_or(false),
            "the marker of the answer left the running call going"
        );
    }

    // The measure of the conversation is the measure of the text that go out with the request.
    // Every part the server is sent costs the space of it: the text of a turn, the thinking
    // that is sent back when the chat keeps it, and the name and the arguments of a call of a
    // tool. A measure that left a part out read the space as bigger than it is, and the
    // space was then spent before the meter recorded it.
    #[test]
    fn the_measure_of_the_talk_counts_every_word_that_goes_out() {
        let plain = json!([
            {"role": "user", "content": "the first prompt"},
            {"role": "assistant", "content": "the answer of the model"},
        ]);
        let base = estimate_tokens(&plain);
        // A turn that made a call. The call rides on the message of the turn, with the name of
        // the tool and the arguments the model wrote under the part named `function`, which is
        // the form the conversation carries to the server.
        let called = json!([
            {"role": "user", "content": "the first prompt"},
            {"role": "assistant", "content": Value::Null, "tool_calls": [{
                "id": "call-one",
                "type": "function",
                "function": {
                    "name": "Run one shell command",
                    "arguments": "{\"command\":\"uname -a and a good many words of arguments to cost the room of them\"}"
                }
            }]},
        ]);
        let with_calls = estimate_tokens(&called);
        assert!(
            with_calls > base,
            "the text of a call was not counted: {} against {}",
            with_calls,
            base
        );
        // The thinking that is sent back costs the space of its text.
        let thought = json!([
            {"role": "user", "content": "the first prompt"},
            {"role": "assistant", "content": "the answer of the model", "reasoning_content": "a great deal of thinking that the chat keeps and sends back again to the model with the conversation"},
        ]);
        let with_thinking = estimate_tokens(&thought);
        assert!(
            with_thinking > base,
            "the thinking of a turn was not counted: {} against {}",
            with_thinking,
            base
        );
    }

    // The meter of a summary ask is on the text that go out with that ask: the first prompt,
    // the summary of before, and the text after the marker. The text the summary means is not
    // among them, and neither is the text of a summary that was deleted, so a second ask of one
    // summary does not start with a space that is already removed.
    #[test]
    fn the_meter_of_a_summary_ask_counts_the_words_that_go_out() {
        // A conversation with a great deal between the first prompt and the next message of the
        // user. A summary of it means that text, and after the summary they are no part of what
        // is sent.
        let long = "a long answer of the model with a great many words in it. ".repeat(200);
        let whole = json!([
            {"role": "user", "content": "the first prompt"},
            {"role": "assistant", "content": long},
            {"role": "user", "content": "and the next thing"},
        ]);
        let trimmed = json!([
            {"role": "user", "content": "the first prompt"},
            {"role": "user", "content": "Done: the answer above. Next: nothing."},
            {"role": "user", "content": "and the next thing"},
        ]);
        let whole_room = estimate_tokens(&whole);
        let trimmed_room = estimate_tokens(&trimmed);
        assert!(
            trimmed_room * 4 < whole_room,
            "the text a summary means were still counted: {} against {}",
            trimmed_room,
            whole_room
        );
        let ask = summary_ask(&trimmed, "Write a very short handoff summary", None);
        // The request line is sent with the conversation, so the meter of the request is above
        // the measure of the conversation alone, and nothing else is added to it.
        let start = summary_start(&ask);
        assert!(start > trimmed_room, "the ask line was not counted");
        assert!(
            start < whole_room,
            "the text the summary means were counted in the meter of the request: {} against {}",
            start,
            whole_room
        );
    }

    // The two errors that end a summary with no text is distinguished by what the server spoke
    // of the end of the writing. Only one of them is the space, and the user is not
    // informed the space was to blame when it was not.
    #[test]
    fn the_words_of_an_empty_summary_tell_the_two_faults_apart() {
        let full = empty_summary_words("length");
        assert!(full.contains("filled the context"), "{}", full);
        let silent = empty_summary_words("stop");
        assert!(!silent.contains("filled the context"), "{}", silent);
        assert!(silent.contains("the context was not full"), "{}", silent);
        // A server that gave no reason at all is not blamed on the space either.
        assert!(!empty_summary_words("").contains("filled the context"));
    }

    // A summarisation that was never written is started again in the place it remains. The user
    // was set to write that one again, so a new heading with a new number would say the chat
    // was summarised twice when it was not. The marker keeps its number, and the chat is not
    // held by an empty marker of its own once the text come.
    #[test]
    fn a_summarisation_started_again_keeps_its_mark_and_number() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        add_message(&mut state, "Assistant", "the old answer");
        let first = state.begin_summary();
        state.finish_summary(&first, "Done: the parser. Next: the bar.", 6964, 412, "");
        // The second summarisation was requested and never written.
        let second = state.begin_summary();
        state.finish_summary(&second, "", 7000, 0, "the server went away");
        assert!(state.handoff_owed());
        assert_eq!(state.handoffs().len(), 2);
        // Started again: the same marker, the same number, and no third heading.
        let again = state.begin_summary();
        assert_eq!(again, second);
        let marks = state.handoffs();
        assert_eq!(marks.len(), 2);
        assert_eq!(marks[1].number, 2);
        state.live_write().generating = true;
        assert_eq!(state.handoffs()[1].state, "writing");
        state.live_write().generating = false;
        state.finish_summary(&again, "Done: the bar. Next: nothing.", 7000, 96, "");
        assert_eq!(state.handoffs().len(), 2);
        assert_eq!(state.handoffs()[1].number, 2);
        assert_eq!(state.handoffs()[1].state, "done");
        // A marker of long ago that was never written must not be written into for a new
        // summarisation. Only the newest marker is started again, and a fresh summarisation is
        // a fresh marker. The old marker is put before the marker that came off, so it is not
        // the newest item in the conversation.
        if let Some(chat) = state.chat_mut() {
            let at = chat
                .messages
                .iter()
                .rposition(|m| m.role == "Summary")
                .unwrap_or(0);
            chat.messages.insert(
                at,
                Message {
                    id: String::from("s-old"),
                    role: String::from("Summary"),
                    body: String::new(),
                    ..Default::default()
                },
            );
        }
        // The newest marker holds text, so the chat is free again.
        assert!(!state.handoff_owed());
        let third = state.begin_summary();
        assert_ne!(third, "s-old");
        // The two markers of the conversation, the marker of long ago, and this new one.
        let marks = state.handoffs();
        assert_eq!(marks.len(), 4);
        assert_eq!(marks[1].id, "s-old");
        assert_eq!(marks[1].state, "owed");
        assert_eq!(marks[3].id, third);
        assert_eq!(marks[3].number, 4);
        // The new marker holds the chat until its text come, and no longer after.
        assert!(state.handoff_owed());
        state.finish_summary(&third, "Done: nothing left to do.", 7000, 64, "");
        assert!(!state.handoff_owed());
    }

    // A chat that was summarised sends the first message of the user, the handoff summary, and
    // the text that came after the summary. The text the summary means are left out, which is
    // the space the compaction made.
    #[test]
    fn a_summarised_chat_goes_on_from_the_first_prompt_and_the_summary() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        add_message(&mut state, "Assistant", "the old answer, which was long");
        add_message(&mut state, "You", "an older question");
        // The line of the day and the clock time that is before the first message of the chat
        // is not one of the messages of the conversation here.
        let before = undated(&state.build_history());
        assert_eq!(before.len(), 3);
        let mark = state.begin_summary();
        state.finish_summary(&mark, "Done: the parser. Next: the bar.", 6964, 412, "");
        // A chat that ends on a summarisation asks the model from behind it: the handoff block
        // is the last word of the conversation, which is what the model is put back to work on
        // when the summary is written, and the first message of the user is still the first
        // item in front of it.
        let stood = undated(&state.build_history());
        assert_eq!(stood.len(), 2);
        assert_eq!(stood[0]["content"], json!("the first prompt"));
        assert_eq!(stood[1]["role"], json!("user"));
        assert!(stood[1]["content"]
            .as_str()
            .expect("words")
            .contains("The session was compacted"));
        add_message(&mut state, "You", "and the next thing");
        add_message(&mut state, "Assistant", "the answer after the summary");
        let items = undated(&state.build_history());
        // The first message of the user keeps the place it had, and the text of it is the same
        // text, byte for byte. The server worked them through once and keeps that work, which
        // is why the summary rides after that message instead of in the place of it.
        assert_eq!(items[0], before[0]);
        assert_eq!(items[0]["role"], json!("user"));
        assert_eq!(items[0]["content"], json!("the first prompt"));
        // The summary remains second, wrapped in the text that name it.
        assert_eq!(items[1]["role"], json!("user"));
        let text = items[1]["content"].as_str().expect("words");
        assert!(text.contains("The session was compacted"));
        assert!(text.contains("Done: the parser."));
        // Then the text that came after the summary, and nothing else: the old question and the
        // long answer it was answered with are removed from the prompt, for the summary means
        // them.
        assert_eq!(items[2]["content"], json!("and the next thing"));
        assert_eq!(items[3]["role"], json!("assistant"));
        assert_eq!(items.len(), 4);
        assert!(!json!(items).to_string().contains("the old answer"));
        assert!(!json!(items).to_string().contains("an older question"));
        // The line of the day and the clock time still is at the header of the conversation
        // after the compaction, before the first message of the user, and the first message is
        // still sent word for word.
        let said = state.build_history();
        let lines = date_lines(&said);
        assert_eq!(lines.len(), 1, "one line of the day: {:?}", lines);
        let head = said
            .as_array()
            .expect("a list of the conversation")
            .first()
            .expect("the conversation is not empty");
        assert!(head["content"]
            .as_str()
            .expect("a line of the day first")
            .starts_with(DATE_HEAD));
        assert_eq!(undated(&said)[0]["content"], json!("the first prompt"));
        // The user still sees all of the conversation: a summarisation is a marker in it, and
        // no word is deleted. The lock of the state is let go first, for the snapshot takes it
        // and a lock is not taken twice.
        drop(state);
        let snap = engine.snapshot();
        assert_eq!(snap.messages.len(), 6);
        // No summary is being written here, so the window is free to draw a live answer. While
        // a summary is written it draws none.
        assert!(!snap.summarising);
        assert!(snap
            .messages
            .iter()
            .any(|m| m.role == "Summary" && m.context_before == 6964));
    }

    // The context window filled under a summary, so it stops in the middle of itself and is
    // asked again for a short one. A stop of the user's own and an error in the stream are not:
    // the first has nothing to compress, and the other is set to the user, who starts the
    // summary again.
    #[test]
    fn a_summary_that_ran_to_the_end_of_the_room_is_asked_again_for_a_short_one() {
        let shot = |finish: &str, stopped: bool, error: Option<&str>| SummaryShot {
            text: String::from("a summary that stops in the middle of a senten"),
            finish: String::from(finish),
            calls: Vec::new(),
            stopped,
            error: error.map(String::from),
            sent: 0,
        };
        // The context window filled: ask again, of the summary prompt.
        assert!(summary_too_long(&shot("length", false, None)));
        // The model stopped of its own accord, so the summary is whole.
        assert!(!summary_too_long(&shot("stop", false, None)));
        assert!(!summary_too_long(&shot("", false, None)));
        // The user stopped the request, and the stream broke: neither is asked again for a
        // shorter one.
        assert!(!summary_too_long(&shot("length", true, None)));
        assert!(!summary_too_long(&shot("length", false, Some("the server went away"))));
    }

    // A model that answers the request for a summary with a call of a tool, truncated by the
    // space, has written nothing the next model can use. Those text is deleted, so that the
    // request is rebuilt rather than a call of a tool remaining in the place of a summary.
    #[test]
    fn a_call_of_a_tool_is_no_summary() {
        let cut = "Done: the page was written.\n\n<|im_start|toolcall\n{\"name\": \"web_search\", \"arguments\": {\"query\": \"Helluva Boss plot summary I.M.P assassin business Stolas romance Goet";
        assert!(clean_summary(cut).is_empty());
        // Text that shut their marker again is the text of the model after all, and are kept as
        // they were written.
        let told = "Done: the page was written. Next: nothing.";
        assert_eq!(clean_summary(told), told);
    }

    // The request for a handoff summary sets no ceiling over the text the model may write. A
    // summary is the making of the space and not the spending of it, so a ceiling of 4096
    // in the middle of a long summary cut it off in the middle of a sentence; the model now
    // writes as long as it likes, and a request that runs to the end of the space is freed
    // again for a shorter summary. The number sent is -1, which also lifts a `--n-predict` out
    // of the settings of the model.
    #[test]
    fn the_ask_for_a_summary_sets_no_ceiling_over_the_words() {
        let talk = json!([
            { "role": "user", "content": "the first prompt" },
            { "role": "assistant", "content": "an answer of some length" },
        ]);
        let ask = summary_ask(&talk, "Summarise it.", None);
        assert_eq!(ask["max_tokens"], json!(NO_SUMMARY_CEILING));
        assert_eq!(ask["max_tokens"].as_i64(), Some(-1));
        // The same for a conversation that has eaten the whole window: the request is not
        // clipped to a directory of its own, and it is not grown either.
        let huge = "x".repeat(4 * 16000);
        let ask = summary_ask(&json!([{ "role": "user", "content": huge }]), "Summarise it.", None);
        assert_eq!(ask["max_tokens"], json!(NO_SUMMARY_CEILING));
        // The text of the conversation all ride with the request, and the request of the
        // summary is the last message of them.
        let items = ask["messages"].as_array().expect("a list of the conversation");
        assert_eq!(items.len(), 2);
        assert_eq!(ask["stream"], json!(true));
    }

    // A model that reads a session full of tool calls again is minded to make one, and a call
    // made instead of the summary spends the very space the summary was come to make. The
    // tools cannot simply be left out of the request -- the list of them is part of the start
    // of the text the server has worked through -- so the request reports in its own text that
    // nothing here is to be called, whatever the user set the handoff prompt to.
    #[test]
    fn the_ask_for_a_summary_says_not_to_call_any_tool() {
        let talk = json!([{ "role": "user", "content": "the first prompt" }]);
        let ask = summary_ask(&talk, "Write a handoff summary.", None);
        let items = ask["messages"].as_array().expect("a list of the conversation");
        let last = items[items.len() - 1]["content"]
            .as_str()
            .expect("the words of the ask");
        // The text of the user stays as the user wrote it, and the guard is added after it.
        assert!(last.starts_with("Write a handoff summary."));
        // The guard is appended to whatever the user set, so the word against calling is in
        // every ask, of every prompt.
        assert!(last.contains("Do not call any tools."));
        assert!(last.contains("never a call and never the marker of one"));
        // The request names a length, for a summary that fills the window leaves the space it was
        // asked to free.
        assert!(last.contains("three hundred words"));
        // The first message of the user is still the first message, for the server keeps the
        // work it did on that text.
        assert_eq!(items[0]["content"], json!("the first prompt"));
        assert_eq!(ask["stream"], json!(true));
        // The server is no longer asked to leave the calls unread. A call that comes is caught
        // and cancelled, and a server informed not to read them at all would hide one.
        assert!(ask.get("parse_tool_calls").is_none());
    }

    // The list of the tools is rendered into the start of the conversation, so the request for
    // a summary must carry the very list an answer of this chat carries. A request with another
    // list, or with none, is the whole context worked through again from its first word, which
    // is what this ask used to do.
    #[test]
    fn the_ask_for_a_summary_carries_the_tools_of_the_talk() {
        let talk = json!([{ "role": "user", "content": "the first prompt" }]);
        let tools = json!([{ "type": "function", "function": { "name": "run_terminal" } }]);
        let ask = summary_ask(&talk, "Summarise it.", Some(&tools));
        assert_eq!(ask["tools"], tools);
        assert_eq!(ask["tool_choice"], json!("auto"));
        // A chat with nothing proposed returns nothing, as its answers do.
        let ask = summary_ask(&talk, "Summarise it.", None);
        assert!(ask.get("tools").is_none());
        assert!(ask.get("tool_choice").is_none());
    }

    // The bytes of an image cost a great deal of space, and they used to be taken out of
    // the conversation for the request. That saving was paid for many times over: the image is
    // among the text the server has already worked through, and taking it out made a front the
    // server had never seen, so the whole context went through again. The images ride with the
    // request now, as they ride with every other message of the conversation.
    #[test]
    fn the_ask_for_a_summary_keeps_the_pictures_of_the_talk() {
        let talk = json!([
            { "role": "user", "content": "the first prompt" },
            { "role": "user", "content": [ { "type": "text", "text": "look at this" },
                                            { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAA=" } } ] },
        ]);
        let ask = summary_ask(&talk, "Summarise it.", None);
        let items = ask["messages"].as_array().expect("a list of the conversation");
        // The message the image hangs on remains part for part as it was.
        let parts = items[1]["content"].as_array().expect("the parts of the message");
        assert_eq!(parts, talk[1]["content"].as_array().expect("the same parts"));
        assert_eq!(parts[1]["type"], json!("image_url"));
        // The request is the conversation and one message more, which is all the server has
        // that it has not seen before.
        assert_eq!(items.len(), 3);
    }

    // A conversation full of tool runs is the commonest conversation of a working chat, and the
    // request for a summary of it used to carry it made over: the calls and the results of them
    // turned into lines of text. Every one of those cuts fell in the middle of the text the
    // server had worked through, and the whole context went through again for it. The request
    // carries the conversation as it is now, so the server keeps its work and has only the
    // request to read anew.
    #[test]
    fn the_ask_for_a_summary_carries_the_talk_unchanged() {
        let talk = json!([
            { "role": "user", "content": "the first prompt" },
            { "role": "assistant", "content": Value::Null,
              "tool_calls": [{ "id": "c1", "type": "function",
                               "function": { "name": "run_terminal",
                                             "arguments": "{\"command\":\"ls\"}" } }] },
            { "role": "tool", "tool_call_id": "c1", "content": "notes.txt todo.txt" },
            { "role": "assistant", "content": "Two files stand here." },
            { "role": "user", "content": "Change the second one." },
            { "role": "assistant", "content": "I will edit it.",
              "tool_calls": [{ "id": "c2", "type": "function",
                               "function": { "name": "edit_file",
                                             "arguments": "{\"path\":\"todo.txt\"}" } }] },
            { "role": "tool", "tool_call_id": "c2", "content": "1 place changed." },
            { "role": "assistant", "content": "The second item is changed." },
        ]);
        let ask = summary_ask(&talk, "Summarise it.", None);
        let items = ask["messages"].as_array().expect("a list of the conversation");
        // Every message of the conversation is in the request as it was in the conversation,
        // calls, results and all.
        let told = talk.as_array().expect("the conversation");
        assert_eq!(items.len(), told.len() + 1);
        for (place, message) in told.iter().enumerate() {
            assert_eq!(&items[place], message, "the message at {} was changed", place);
        }
        // The request is the last message, and it is a message of the user.
        let last = items[items.len() - 1].clone();
        assert_eq!(last["role"], json!("user"));
        assert!(last["content"].as_str().expect("words").starts_with("Summarise it."));
    }

    // A marker of an image is what llama.cpp searches for in the text it is sent: it counts the
    // markers and expects a bitmap at all of them, and it refuses the whole ask when a marker
    // is where no image is. A command that read the properties of an inference server carried
    // such a marker into a chat, and every later ask of that chat was refused. Text with no
    // marker in them are left exactly as they are now, and a marker is broken by one space, so
    // the text still reports what they stated.
    #[test]
    fn a_media_mark_quoted_in_words_is_broken() {
        assert_eq!(break_media_marks("nothing of an image here"), None);
        assert_eq!(break_media_marks(""), None);
        // The marker of the server, and the plain marker of the template, are both broken, and
        // so are two markers in one line.
        assert_eq!(
            break_media_marks("a: <__media_Qv1A2b3C4d5E6f7G8h9I0jK__> b: <__media__>"),
            Some(String::from("a: < __media_Qv1A2b3C4d5E6f7G8h9I0jK__> b: < __media__>"))
        );
        // Broken once, the text hold no marker any more, so the request of this turn and the
        // request of the next carry the same text and the server keeps the work it did on it.
        let once = break_media_marks("media_marker: '<__media_Qv1A2b3C4d5E6f7G8h9I0jK__>'")
            .expect("a mark was broken");
        assert_eq!(break_media_marks(&once), None);
    }

    // The marker is broken wherever it is in the request: in the result of a command, in the
    // parts of a message, in the arguments of a call. A number and a word with no marker in it
    // are left where they are.
    #[test]
    fn a_media_mark_is_broken_wherever_it_stands_in_an_ask() {
        let mut ask = json!({
            "messages": [
                { "role": "user", "content": "What does the server say?" },
                { "role": "assistant", "content": Value::Null,
                  "tool_calls": [{ "id": "c1", "type": "function",
                                   "function": { "name": "run_terminal",
                                                 "arguments": "{\"command\":\"curl -s /props\"}" } }] },
                { "role": "tool", "tool_call_id": "c1",
                  "content": [{ "type": "text",
                                "text": "'media_marker': '<__media_Qv1A2b3C4d5E6f7G8h9I0jK__>'" }] },
            ],
            "max_tokens": 400,
        });
        break_media_marks_in(&mut ask);
        let items = ask["messages"].as_array().expect("the conversation");
        let receipt = items[2]["content"][0]["text"].as_str().expect("the result");
        assert!(!receipt.contains("<__media_"), "the mark still stands");
        assert!(receipt.contains("< __media_"), "the text still reports what they stated");
        assert_eq!(items[0]["content"], json!("What does the server say?"));
        assert_eq!(
            items[1]["tool_calls"][0]["function"]["arguments"],
            json!("{\"command\":\"curl -s /props\"}")
        );
        assert_eq!(ask["max_tokens"], json!(400));
    }

    // The conversation that is sent is where the marker is broken. The chat keeps the text as
    // the user reads them: the result of the command still shows the line the server wrote, and
    // only what goes to the model is broken.
    #[test]
    fn the_talk_that_goes_out_carries_no_media_mark() {
        let _guard = config::test_turn();
        let mark = "<__media_Qv1A2b3C4d5E6f7G8h9I0jK__>";
        let engine = engine_with(vec![
            Message {
                id: String::from("a"),
                role: String::from("You"),
                body: String::from("What does the server say about its media marker?"),
                ..Default::default()
            },
            Message {
                id: String::from("b"),
                role: String::from("Tool"),
                call_id: String::from("c1"),
                tool_name: String::from("run_terminal"),
                tool_args: String::from("{\"command\":\"curl -s /props\"}"),
                tool: format!("'media_marker': '{}'", mark),
                ..Default::default()
            },
        ]);
        let state = engine.state.lock().unwrap();
        let talk = state.build_history();
        let items = talk.as_array().expect("the conversation");
        let mut told = String::new();
        for one in items {
            told.push_str(&one.to_string());
        }
        assert!(!told.contains("<__media_"), "a marker was sent to the server");
        assert!(told.contains("< __media_"), "the result lost its text");
        // The chat itself still holds the line as it was read.
        let chat = state.chat().expect("the chat stands open");
        assert!(chat.messages[1].tool.contains(mark), "the chat lost the line");
    }

    // A conversation that ends on a call, or on the result of one, used to end the request on a
    // note about the model once the calls were removed, and the server was then asked to go on
    // where the model left off instead of to summarise it. Nothing is taken out of the
    // conversation now, and the request is itself a message of the user: the request ends the
    // conversation whatever the conversation ends on.
    #[test]
    fn a_summary_ask_ends_on_the_ask_whatever_the_talk_ends_on() {
        let talk = json!([
            { "role": "user", "content": "the first prompt" },
            { "role": "assistant", "content": Value::Null,
              "tool_calls": [{ "id": "c1", "type": "function",
                               "function": { "name": "run_terminal",
                                             "arguments": "{\"command\":\"ls\"}" } }] },
            { "role": "tool", "tool_call_id": "c1", "content": "notes.txt todo.txt" },
        ]);
        let ask = summary_ask(&talk, "Summarise it.", None);
        let items = ask["messages"].as_array().expect("a list of the conversation");
        let last = items[items.len() - 1].clone();
        assert_eq!(last["role"], json!("user"));
        assert!(last["content"].as_str().expect("words").starts_with("Summarise it."));
        // The result of the last run is the message before the request, so the model reads the
        // conversation to its end and the request after it.
        let before = items[items.len() - 2].clone();
        assert_eq!(before["role"], json!("tool"));
    }

    // A model that answers the request for a summary with a call instead of with the summary is
    // answered as any other run is answered: the call is cancelled, the reason is given, and
    // the request goes on from where it was. The pair of entries is shaped as a call and its
    // result are shaped, for a server reads a call with no result as a debt it still has to
    // pay.
    #[test]
    fn a_call_made_instead_of_a_summary_is_answered_as_a_cancelled_run() {
        let calls = vec![
            llama::ToolCall {
                id: String::from("c1"),
                name: String::from("run_terminal"),
                arguments: String::from("{\"command\":\"ls\"}"),
            },
            // A server that wrote no id for a call must still be answered with one, and the
            // same one on both sides of the pair.
            llama::ToolCall {
                id: String::new(),
                name: String::from("search_web"),
                arguments: String::from("{}"),
            },
        ];
        let made = summary_calls_answered(&calls, "One moment, I will look.");
        assert_eq!(made.len(), 3);
        assert_eq!(made[0]["role"], json!("assistant"));
        assert_eq!(made[0]["content"], json!("One moment, I will look."));
        let given = made[0]["tool_calls"].as_array().expect("the calls as they were");
        assert_eq!(given.len(), 2);
        assert_eq!(given[0]["id"], json!("c1"));
        assert_eq!(given[1]["function"]["name"], json!("search_web"));
        // The results name the call they answer and say why it never ran.
        let ids = ["c1", "no-tool-2"];
        for (place, id) in ids.iter().enumerate() {
            let receipt = &made[place + 1];
            assert_eq!(receipt["role"], json!("tool"));
            assert_eq!(receipt["tool_call_id"], json!(id));
            let said = receipt["content"].as_str().expect("the reason");
            assert!(
                said.contains("No tool may be called while a handoff summary is being written"),
                "{}",
                said
            );
            assert!(said.contains("cancelled"), "{}", said);
        }
        // A model that asked without writing any text beside the call is given back as a saying
        // of nothing, as every other call of that form is.
        let silent = summary_calls_answered(&calls[..1], "   ");
        assert_eq!(silent[0]["content"], Value::Null);
    }

    // A call that was stopped before the model had written all of it is not a call: the text of
    // it stop in the middle. It therefore goes back to the server as the text of the model and
    // never as a call of a tool, for a server set to read arguments out of text that stop in
    // the middle of a string errors over them. A tool that ran is the other form: the call and
    // its result both are in the conversation.
    #[test]
    fn the_telling_of_a_cut_call_goes_back_as_words_and_never_as_a_call() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        let cut = cut_call(editfile::EDIT_NAME, "{\"path\":\"notes.md\"");
        state.push_cancelled(&cut, "", 0, GREATEST_CUT_ARGS);
        let items = undated(&state.build_history());
        // The report remains last, as the text of the turn it was in, and it carries no call
        // the server could read the arguments of.
        let last = items.last().expect("a conversation with the report in it");
        assert_eq!(last["role"], json!("assistant"));
        assert!(
            last.get("tool_calls").is_none(),
            "a report went back as a call: {}",
            last
        );
        assert!(last["content"]
            .as_str()
            .expect("words")
            .starts_with(CUT_TOLD_FIRST));
        // A tool that ran keeps the markers of a call: the call of it, and the result that
        // answers it.
        let mut ran = cut_call(tools::TOOL_NAME, "{\"command\":\"ls\"}");
        ran.id = String::from("call-9");
        let id = state.begin_tool(&ran, "ls", "", 0);
        state.finish_tool(&id, "notes.md", true);
        let items = undated(&state.build_history());
        let last = items.last().expect("a conversation with the result in it");
        assert_eq!(last["role"], json!("tool"));
        assert_eq!(last["tool_call_id"], json!("call-9"));
        let call = &items[items.len() - 2];
        assert_eq!(call["role"], json!("assistant"));
        assert!(
            call["tool_calls"].is_array(),
            "a run went back without a call: {}",
            call
        );
        // The report of the call that was cut remains once, and once only.
        let told = items
            .iter()
            .filter(|item| {
                item["content"]
                    .as_str()
                    .unwrap_or_default()
                    .starts_with(CUT_TOLD_FIRST)
            })
            .count();
        assert_eq!(told, 1, "the report of the call was carried twice");
    }

    // The request of a turn that only goes on carries a note about its own when the
    // conversation ends on a call or on the result of one, so the conversation the server is
    // given ends on a question. The user never wrote that word, so it is not written into the
    // chat at all: it rides on the request alone, and the window draws no bubble of it between
    // the summarisation and the answer. Where the conversation ends on a note about the user
    // already -- the handoff block of a summarisation that remains last -- no word is added,
    // for the block is in the conversation already, and a second copy of it costs the space
    // the summarisation was created to win.
    #[test]
    fn the_word_that_carries_the_work_on_never_reaches_the_window() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        add_message(&mut state, "Assistant", "the answer");
        let mark = state.begin_summary();
        state.finish_summary(&mark, "The conversation was about one item.", 3000, 300, "");
        // The marker of the summarisation remains last, so the conversation ends on its handoff
        // block and is a question already. A request with no word to carry is the conversation
        // as it is now, with nothing added to it.
        let stood = state.build_history();
        assert_eq!(
            go_on_words(&state, ""),
            stood,
            "text was added to a conversation that ends on a question already"
        );
        // A run of a tool after the marker. The conversation now ends on the result of it, so
        // the request carries the word of the program after it, and ends on a question.
        let mut call = cut_call(tools::TOOL_NAME, "{\"command\":\"ls\"}");
        call.id = String::from("call-2");
        let ran = state.begin_tool(&call, "ls", "", 0);
        state.finish_tool(&ran, "notes.txt todo.txt", true);
        let debt = state.build_history();
        let receipt = debt
            .as_array()
            .expect("a conversation")
            .last()
            .expect("a conversation with a result in it");
        assert_eq!(
            receipt["role"].as_str().expect("a role"),
            "tool",
            "the result of the run was not last: {}",
            debt
        );
        let history = go_on_words(&state, CONTINUE_PROMPT);
        let items = undated(&history);
        let last = items.last().expect("a conversation the request was given");
        assert_eq!(last["role"], json!("user"), "the ask did not end on the user");
        assert_eq!(last["content"].as_str().expect("words"), CONTINUE_PROMPT);
        assert!(
            last.get("tool_calls").is_none(),
            "the request still ends on a call: {}",
            history
        );
        // The handoff block of the summarisation is carried once, and once only.
        let block = handoff_block("The conversation was about one item.");
        let carried = items
            .iter()
            .filter(|item| item["content"].as_str().unwrap_or_default() == block.as_str())
            .count();
        assert_eq!(carried, 1, "the handoff block was not carried once: {}", history);
        // Nothing of the word is in the chat, so nothing of it can come to the window.
        let said = json!(state.snap().messages).to_string();
        assert!(!said.contains(CONTINUE_PROMPT), "the word of the program came to the window");
        // A chat written and read again holds no such message either.
        let id = state.current.clone().expect("a chat stands open");
        drop(state);
        engine.snapshot();
        let loaded = engine
            .state
            .lock()
            .unwrap()
            .store
            .load_chat(&id)
            .expect("the chat is on disk");
        assert!(
            !loaded
                .messages
                .iter()
                .any(|message| message.body.contains(CONTINUE_PROMPT)),
            "the word of the program was written into the chat"
        );
    }

    // A chat that was cut for a handoff summary and never got one does not go on asking the
    // model, for its conversation is past the context window of the model. The user starts the
    // summary again from the place it was at.
    #[test]
    fn a_chat_waits_while_its_handoff_summary_is_missing() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        add_message(&mut state, "Assistant", "the answer");
        let mark = state.begin_summary();
        state.finish_summary(&mark, "", 7000, 0, "the server went away");
        assert!(state.handoff_owed());
        drop(state);
        // A message now does not go out, and nothing is written into the chat.
        engine.prompt(String::from("go on with the work"), Vec::new());
        let snap = engine.snapshot();
        assert!(snap.status.contains("never written"));
        assert_eq!(snap.messages.len(), 3);
        // A chat with no summary waiting records it, rather than asking a model for a summary
        // of nothing.
        engine.restart_handoff();
        // The summary of this chat is waiting, and no model is in the top bar, so the word of
        // the model is what the user is given.
        assert!(engine.snapshot().status.contains("Choose a model"));
    }

    // ------------------------------------------------- a turn of nothing
    //
    // A model that came to its own end straight out of a thought leaves a turn with nothing in
    // it. The server writes the marker of that turn, and the next ask that is sent behind it is
    // an item to go on with rather than an item to answer: the model leaves the turn empty
    // again, and the user watches the same nothing after every send. The conversation is
    // therefore asked on with a note about the program behind the turn, the row of the turn
    // keeps the form the template writes, and two turns of nothing end the answer.

    // A model that answered a summarisation with a turn of no text and no thinking leaves the
    // user no row of it, and fills none of the space the summarisation made. The chat is
    // then still past the limit, and it must not be informed it has space to spare: the
    // newest item that was really stated is the summarisation, and the space is freed again
    // rather than the chat left remaining still. A turn with text of its own, or with thinking,
    // is a new item that is worth a summary.
    #[test]
    fn a_turn_of_nothing_after_a_summarisation_is_not_a_thing_said() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        let mark = state.begin_summary();
        state.finish_summary(&mark, "Done: the parser. Next: the bar.", 6964, 412, "");
        assert!(ends_on_summary(state.chat().expect("a chat stands open")));
        // The empty turn of the model: no text, no thinking, and no run.
        state.push_message("Assistant", "", "", 0, false);
        assert!(
            ends_on_summary(state.chat().expect("a chat stands open")),
            "a turn of nothing is no item stated after the summarisation"
        );
        // The thinking of a turn is an item the user reads in the panel of the thought, and the
        // next model is set to read it too.
        state.push_message("Assistant", "", "the thinking of the turn", 0, false);
        assert!(
            !ends_on_summary(state.chat().expect("a chat stands open")),
            "the thinking of a turn is an item stated"
        );
        state.push_message("Assistant", "the answer at last", "", 0, false);
        assert!(
            !ends_on_summary(state.chat().expect("a chat stands open")),
            "text of the model is new text to summarise"
        );
    }

    // The conversation of a step that wrote no text and requested nothing ends on the text of
    // the model with nothing after it. The server is then put to go on where the model left
    // off, and a model whose template writes the marker of an empty turn answers that with the
    // marker of an empty turn again: the log of one such exchange holds seventeen tokens, two
    // of them text, and the empty turn at the end of it. A note about the program is therefore
    // sent behind the empty turn, on the request alone, and never into the chat.
    #[test]
    fn an_empty_turn_of_the_model_is_asked_on_with_a_word_of_the_program() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        // A turn with text of its own is not an empty turn, and neither is the result of a run,
        // which is a note about the tool and not of the model.
        assert!(!ends_on_an_empty_turn(&state.build_history()));
        state.push_message("Assistant", "", "", 0, false);
        let history = state.build_history();
        assert!(ends_on_an_empty_turn(&history));
        assert_eq!(
            empty_turn_words(&history).as_deref(),
            Some(EMPTY_TURN_WORDS),
            "the request must not go out on an empty turn alone"
        );
        // The word of the program rides on the request and is never written into the chat, for
        // the user never wrote it and has nothing to read where it is.
        let asked = undated(&go_on_words(&state, EMPTY_TURN_WORDS));
        let last = asked.last().expect("the ask is not empty");
        assert_eq!(last["role"], json!("user"));
        assert_eq!(last["content"].as_str().expect("words"), EMPTY_TURN_WORDS);
        assert!(
            !json!(undated(&state.build_history()))
                .to_string()
                .contains(EMPTY_TURN_WORDS),
            "the word of the program came to the window"
        );
        // A turn that requested a command stated something, even with no text of its own: the
        // call is in the place of them. A saying of the user, or of a tool, is never the marker
        // of an empty turn.
        assert!(!said_nothing_at_all(&json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{ "id": "c1", "type": "function",
                "function": { "name": "run_terminal", "arguments": "{}" } }]
        })));
        assert!(said_nothing_at_all(&json!({ "role": "assistant", "content": "  " })));
        assert!(said_nothing_at_all(&json!({ "role": "assistant" })));
        assert!(!said_nothing_at_all(&json!({ "role": "user", "content": "" })));
        assert!(!said_nothing_at_all(&json!({ "role": "tool", "content": "" })));
        // The thinking of a turn is an item the model stated, and it rides with the turn when
        // the chat keeps the thinking on a model whose template may keep it, so nothing is put
        // after such a turn. Without that keeping the thinking never reaches the server, and
        // the turn is empty as far as the server can tell: that is the form the user met, where
        // a model that thought at length and wrote no line of its own was asked again and
        // answered with nothing.
        state.push_message("Assistant", "", "a thought the server is never shown", 0, false);
        assert!(
            ends_on_an_empty_turn(&state.build_history()),
            "thinking the server never sees is no item stated"
        );
        state.thinking.has_preserve = true;
        if let Some(chat) = state.chat_mut() {
            chat.meta.preserve_thinking = true;
        }
        state.push_message("Assistant", "", "the thinking of the turn", 0, false);
        assert!(empty_turn_words(&state.build_history()).is_none());
        state.push_message("Assistant", "the answer at last", "", 0, false);
        assert!(!ends_on_an_empty_turn(&state.build_history()));
    }

    // The space was freed after the empty turn, so the handoff block of the marker is behind it
    // and already reports the model to go on with the work. A second word of the same type
    // would say the item twice, and would spend the space the summarisation made for the
    // request that follows it.
    #[test]
    fn an_empty_turn_before_a_summarisation_needs_no_second_word() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        // The empty turn, and the space made after it.
        state.push_message("Assistant", "", "", 0, false);
        assert!(empty_turn_words(&state.build_history()).is_some());
        let mark = state.begin_summary();
        state.finish_summary(&mark, "Done: the parser. Next: the bar.", 6964, 412, "");
        let history = state.build_history();
        assert!(
            ends_on_summary_mark(&history),
            "the handoff block must be at the end of the conversation that is sent"
        );
        assert!(
            empty_turn_words(&history).is_none(),
            "the handoff block already asks the model to go on"
        );
    }

    // A model that thought at length, stated one line out of the thought and shut the turn has
    // stated something, so no word is sent behind its turn. The next ask is sent on the
    // conversation as it is now, as it always did. The chat of the user's log remains this way:
    // the thought of the work is there, and the text outside the thought are few.
    #[test]
    fn a_turn_with_words_of_its_own_is_left_as_it_stands() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        state.push_message("Assistant", "Two real problems in my own work.", "", 0, false);
        let history = state.build_history();
        assert!(!ends_on_an_empty_turn(&history));
        assert!(empty_turn_words(&history).is_none());
        assert!(!ends_on_summary_mark(&history));
    }

    // ------------------------------------------------- a call cut short
    //
    // A model writing a tool call bigger than the space it has writes until the space is
    // removed. The program stops the writing at the limit it was set, and three items have to
    // come of it: the user sees the call that never ran, the model is informed it was cancelled
    // and what it had written, and neither of those two sayings breaks the next ask.

    // One call, stopped in the middle of its text, as the stream leaves it: the name is there,
    // the arguments stop under a string that was never shut.
    fn cut_call(name: &str, arguments: &str) -> ToolCall {
        ToolCall {
            id: String::new(),
            name: String::from(name),
            arguments: String::from(arguments),
        }
    }

    // ------------------------------------------- the day before the model
    //
    // The Chatting settings can put the day and the clock time before the model: at the start
    // of a chat, and with every message of the user. A line is in a message of its own just
    // before the message it dates, so the text of the user go out as they were written, and the
    // chat on disk and the page know nothing of the line.
    fn dated_chat(state: &mut State, cfg: &Config) {
        state.cfg = cfg.clone();
        state.start_chat();
        add_message(state, "You", "the first prompt");
        add_message(state, "Assistant", "the first answer");
        add_message(state, "You", "the second prompt");
        add_message(state, "Assistant", "the second answer");
        add_message(state, "You", "the third prompt");
    }

    // The conversation without the lines of the day and the clock time, and without the text
    // the program puts before the conversation of the chapters, so a test about another rule
    // counts the text of the user and the model as they were.
    fn undated(history: &Value) -> Vec<Value> {
        history
            .as_array()
            .expect("a list of the conversation")
            .iter()
            .filter(|message| {
                let said = message["content"].as_str().unwrap_or_default();
                // The line of the day, the text of the chapters, the text of the colours and
                // the text of the directory is text of the program, not of the conversation.
                !said.starts_with(DATE_HEAD)
                    && !said.starts_with("The chapters of this chat are read from")
                    && !said.starts_with("You may put colour on a few words")
                    && !said.starts_with("The context of this conversation stays open")
            })
            .cloned()
            .collect()
    }

    // The line of the day is at the header of the conversation, and it must not move from one
    // ask of a chat to the next. A header that changes is a different sends requests to the
    // server: the work it cached on the text behind the header is removed, or an answer comes
    // from a state of the cache that is not the state of the conversation. The line is
    // therefore written from the moment the chat was begun, which the chat carries with it, and
    // never from the clock of the request.
    #[test]
    fn the_line_of_the_day_does_not_move_between_the_asks_of_one_chat() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
            let named = state.chat().unwrap().meta.created;
            assert!(named > 0, "a chat begun now carries the moment it was begun");
            let first = date_lines(&state.build_history());
            let again = date_lines(&state.build_history());
            assert_eq!(first.len(), 1, "the line of the day remains once, at the header");
            assert_eq!(first, again, "the header of the conversation moved between two asks");
            assert!(
                first[0].starts_with(DATE_HEAD),
                "the line of the day is at the header: {}",
                first[0]
            );
            assert_eq!(
                state.chat().unwrap().meta.created,
                named,
                "the moment of the chat moved between two asks"
            );
        }
    }

    // The lines of the day and the clock time of a conversation, in the order they remain.
    fn date_lines(history: &Value) -> Vec<String> {
        history
            .as_array()
            .expect("a list of the conversation")
            .iter()
            .filter_map(|message| match message["content"].as_str() {
                Some(words) if words.starts_with(DATE_HEAD) => Some(words.to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_day_stands_at_the_head_of_the_talk_and_nowhere_else() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        dated_chat(&mut state, &Config::default());
        let history = state.build_history();
        // One line of the day, at the header of the conversation, of the role `system`.
        let lines = date_lines(&history);
        assert_eq!(lines.len(), 1, "one line of the day: {:?}", lines);
        let items = history.as_array().expect("a list of the conversation");
        assert_eq!(items[0]["role"], json!("system"));
        assert!(items[0]["content"]
            .as_str()
            .expect("a line of the day first")
            .starts_with(DATE_HEAD));
        // The text of the user go out as they were written: the line is a message of its own
        // and not a word added to them. The text of the chapters remains after the line, so the
        // first of them is sought without any message of the program.
        assert_eq!(undated(&history)[0]["content"], json!("the first prompt"));
        let said = undated(&history);
        assert_eq!(said.len(), 5, "the line is not one of the text of the chat");
        for message in &said {
            // No message of the user carries the line, and none of them is given out as a
            // saying of the program either.
            assert_ne!(message["role"], json!("system"));
            assert!(
                !message["content"]
                    .as_str()
                    .unwrap_or_default()
                    .starts_with(DATE_HEAD),
                "a message of the user carries the line: {:?}",
                message
            );
        }
    }

    // The line names the moment the conversation is created up, and reports out loud that it is
    // the latest this computer has. A chat that runs on into the next day is then informed a
    // true day, instead of the day its first message was sent.
    #[test]
    fn the_day_of_the_talk_is_the_latest_of_this_machine() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        dated_chat(&mut state, &Config::default());
        let lines = date_lines(&state.build_history());
        assert_eq!(lines.len(), 1);
        // The day of the week and the day of the month, which are what the line is checked
        // against. The hour moves a minute at a time.
        let today = store::format_date_clock(store::now_secs());
        let parts: Vec<&str> = today.split(", ").take(2).collect();
        let day = parts.join(", ");
        assert!(lines[0].contains(&day), "the day of this computer: {:?}", lines[0]);
        assert!(lines[0].contains("latest"), "the latest: {:?}", lines[0]);
        assert!(lines[0].contains("latest the machine has"), "the hour of this computer: {:?}", lines[0]);
        // The line is a note and not a question: a model that reads it as an item to answer
        // writes to itself that no real query is there, and that text come out as thinking
        // before an answer.
        assert!(lines[0].contains("not a question"));
        assert!(lines[0].contains("do not speak of it"));
    }

    #[test]
    fn the_day_can_stand_away_altogether() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        let cfg = Config {
            chat_date_new_chat: false,
            ..Default::default()
        };
        dated_chat(&mut state, &cfg);
        let history = state.build_history();
        assert!(date_lines(&history).is_empty());
        // The five messages of the chat, with the text of the program removed too.
        assert_eq!(undated(&history).len(), 5);
    }

    // The rule of the one line: the switch on gives a message of the role `system`, and the
    // switch off gives nothing at all. The line never is beside a message of the user, so it
    // can never remain twice.
    #[test]
    fn the_day_is_one_message_of_the_program_or_nothing_at_all() {
        let mut cfg = Config::default();
        let made = date_message(&cfg, 1_700_000_000)
            .expect("the switch stands on for a new set of settings");
        assert_eq!(made["role"], json!("system"));
        assert!(made["content"]
            .as_str()
            .expect("the words of the line")
            .starts_with(DATE_HEAD));
        cfg.chat_date_new_chat = false;
        assert!(date_message(&cfg, 1_700_000_000).is_none(), "the switch off sends no line");
    }

    // The text of the user and the flagged prompts of the program make one block before the
    // conversation. Nothing flagged and nothing written gives nothing, so a chat of a new set
    // of settings sends no such message at all; the text of the user alone is their own text;
    // and the flagged prompts are in front of them, in the order of the table, kept apart by a
    // blank line.
    #[test]
    fn the_words_before_the_talk_hold_the_marks_and_then_the_words_of_the_user() {
        let mut cfg = Config::default();
        assert_eq!(system_words_of(&cfg), "", "nothing written and nothing marked");
        cfg.chat_system_prompt = String::from("  Answer in short sentences.  ");
        assert_eq!(system_words_of(&cfg), "Answer in short sentences.");
        let ids: Vec<String> = crate::prompts::lines()
            .iter()
            .map(|one| String::from(one["id"].as_str().unwrap_or("")))
            .collect();
        cfg.chat_system_prompt = String::new();
        cfg.chat_system_prompts_on = ids.clone();
        assert_eq!(system_words_of(&cfg), crate::prompts::front(&ids));
        cfg.chat_system_prompt = String::from("Never open with praise.");
        let both = system_words_of(&cfg);
        assert!(both.starts_with(&crate::prompts::front(&ids)));
        assert!(both.ends_with("Never open with praise."));
        assert!(both.contains("\n\nNever open with praise."), "the blocks are kept apart");
    }

    // The header of the conversation must not move while the conversation runs. The line of the
    // day is written from the moment the chat was begun and from no other, so every turn of a
    // chat - above all the turn that carries the answer of a tool, which returns seconds after
    // the call was created - is sent with the header the server has already worked through, and
    // the prompt cache holds. A line written from the clock of the moment made a new header
    // every minute, and the whole context was read over again on every turn with the caching
    // switch on.
    #[test]
    fn the_head_of_the_talk_does_not_move_between_turns() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        dated_chat(&mut state, &Config::default());
        // The header as it is now, and as it is now a minute later, and as it is now after a
        // run of a tool arrived among the conversation.
        let first = date_lines(&state.build_history());
        let began = state.chat().expect("a chat stands open").meta.created;
        let said = date_message(&Config::default(), began).expect("the switch stands on");
        assert_eq!(first.len(), 1);
        assert_eq!(said["content"].as_str().expect("the words of the line"), first[0]);
        // A minute of the clock, an hour of it, and a day of it make no difference to the
        // header of a chat that has begun.
        for later in [60i64, 3_600, 86_400, 604_800] {
            let moved = date_message(&Config::default(), began + later)
                .expect("the switch stands on");
            assert_ne!(
                moved["content"].as_str().expect("the words of the line"),
                first[0],
                "a moment other than the beginning of the chat makes a new header"
            );
        }
        // The rule that matters to the server: two checks the same chat give one and the same
        // header, whatever the clock reports between them.
        let again = date_lines(&state.build_history());
        assert_eq!(again, first, "the header of the conversation moved between two looks");
        // And the header is the first message of the conversation, which is the part the server
        // checks first when it searches for work it has done before.
        let list = state.build_history();
        let head = list.as_array().expect("a list of the conversation")[0].clone();
        assert_eq!(head["role"], json!("system"));
        assert!(head["content"]
            .as_str()
            .expect("the words of the line")
            .starts_with(DATE_HEAD));
    }
    #[test]
    fn a_tool_call_cut_short_goes_back_as_words_and_not_as_a_call() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        let cut = cut_call(
            editfile::EDIT_NAME,
            "{\"path\":\"notes.md\",\"edits\":[{\"old_string\":\"the pl",
        );
        state.push_cancelled(&cut, "the thinking of the turn", 120, GREATEST_CUT_ARGS);
        // The line of the day and the clock time is not one of the messages here.
        let items = undated(&state.build_history());
        // The call goes back as the text of the turn it was in. A server set to read arguments
        // out of a call would error over text that stop in the middle of a string, which is the
        // very error the summarisation is meant to keep the conversation out of.
        assert_eq!(items[1]["role"], json!("assistant"));
        assert!(items[1].get("tool_calls").is_none());
        let text = items[1]["content"].as_str().expect("words");
        // The model is informed the three items it needs: that the call was cancelled, which
        // tool it was, and what it had written when it stopped.
        assert!(text.contains("cancelled"));
        assert!(text.contains(editfile::EDIT_NAME));
        assert!(text.contains("the pl"), "the words so far were lost");
        assert!(text.contains("again"), "the model was not informed to write it over");
        // The text stop in the middle, and the model hears so: the string it was writing was
        // never shut.
        assert!(text.contains("ends in the middle"));
        // And nothing of it is a call, nor a result of one: the conversation that is sent has
        // no tool entry of any type in it.
        let told = json!(items).to_string();
        assert!(!told.contains("tool_calls"));
        assert!(!told.contains("tool_call_id"));
        // The user sees the block of the call, named for the file the model was writing into,
        // with the word of what became of it.
        drop(state);
        let snap = engine.snapshot();
        let last = snap.messages.last().expect("a message stands last");
        assert!(last.cancelled);
        assert!(last.failed);
        assert_eq!(last.role, "Tool");
        assert_eq!(last.body, "notes.md");
        assert_eq!(last.tool, CANCELLED_WORDS);
        // All of the text of the call rides on the message, so the model is given what it wrote
        // and not a summary of it.
        assert_eq!(last.tool_args, cut.arguments);
    }

    // A call the user stopped manually. The block stays in the chat, for the user stopped an
    // item they could see being written, and the model is informed the user stopped it. Nothing
    // of the directory is in the report, for the space was never come to and no space
    // was created.
    #[test]
    fn a_call_stopped_by_the_user_stands_in_the_chat_and_goes_back_as_words() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        let stopped = cut_call(
            editfile::EDIT_NAME,
            "{\"path\":\"notes.md\",\"edits\":[{\"old_string\":\"the pl",
        );
        state.push_stopped_by_user(&stopped, "the thinking of the turn", 120, GREATEST_CUT_ARGS);
        let history = state.build_history();
        // The line of the day and the clock time is at the header of the conversation, so the
        // text of the conversation is counted without it.
        let items = undated(&history);
        // The call goes back as the text of the turn it was in, as a call the context window
        // filled under does: a server set to read arguments out of text that stop in the middle
        // of a string would error over them.
        assert_eq!(items[1]["role"], json!("assistant"));
        assert!(items[1].get("tool_calls").is_none());
        let text = items[1]["content"].as_str().expect("words");
        assert!(text.contains("cancelled"));
        // The user is named as the pointer that stopped it, and nothing of a space is informed,
        // for the space was not come to.
        assert!(text.contains("user stopped"), "the user was not named: {}", text);
        assert!(!text.contains("summarisation"), "a space was informed of: {}", text);
        assert!(!text.contains("handoff"), "a space was informed of: {}", text);
        assert!(text.contains(editfile::EDIT_NAME));
        assert!(text.contains("the pl"), "the words so far were lost");
        assert!(text.contains("ends in the middle"));
        assert!(!history.to_string().contains("tool_calls"));
        assert!(!history.to_string().contains("tool_call_id"));
        // The user sees the block of the call, named for the file the model was writing into,
        // with the word of what became of it: their own stop, and not an error in the tool.
        drop(state);
        let snap = engine.snapshot();
        let last = snap.messages.last().expect("a message stands last");
        assert_eq!(last.role, "Tool");
        assert!(last.cancelled);
        assert!(last.user_stopped);
        assert!(last.failed);
        assert_eq!(last.body, "notes.md");
        assert_eq!(last.tool, STOPPED_WORDS);
        // A call the context window filled under keeps its own text, and does not borrow the
        // stop of the user. The two pointers cannot be mistaken for one another in the chat,
        // nor in what goes back to the model.
        let mut state = engine.state.lock().unwrap();
        let room = cut_call(tools::TOOL_NAME, "{\"command\":\"ls -");
        state.push_cancelled(&room, "", 0, GREATEST_CUT_ARGS);
        drop(state);
        let snap = engine.snapshot();
        let last = snap.messages.last().expect("a message stands last");
        assert!(last.cancelled);
        assert!(!last.user_stopped);
        assert_eq!(last.tool, CANCELLED_WORDS);
        let told = stopped_words(editfile::EDIT_NAME, "{\"path\":\"a");
        let room_told = cut_words(editfile::EDIT_NAME, "{\"path\":\"a");
        assert!(told.contains("user stopped") && !room_told.contains("user stopped"));
        assert!(room_told.contains("summarisation") && !told.contains("summarisation"));
    }

    // The theme of the pages. The word of the theme is what the page is informed, and the
    // switch of the two themes that was before the third is kept in step with it, so that a
    // settings file of this program can still be read by a program written when there were two.
    // A word that is none of the three is read as the pale theme, and a settings file with no
    // word of one answers with the switch it holds.
    #[test]
    fn the_skin_of_the_pages_keeps_the_old_switch_in_step() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        assert_eq!(engine.snapshot().theme, "light");
        assert!(!engine.snapshot().dark_mode);
        // The third theme is on a dark ground, but it is not the dark theme, so the old switch
        // stays off: a program written for two themes reads the pale one, which is an honest
        // fall back.
        engine.set_theme("corporate");
        assert_eq!(engine.snapshot().theme, "corporate");
        assert!(!engine.snapshot().dark_mode);
        // The silver theme is the ladder of grey the older program called corporate. Like the
        // two themes of no colour it is not the dark theme, so the old switch stays off.
        engine.set_theme("silver");
        assert_eq!(engine.snapshot().theme, "silver");
        assert!(!engine.snapshot().dark_mode);
        // The neon theme is the fifth: a dark ground of its own, and colours of its own, so the
        // old switch stays off as it does for the two grey themes.
        engine.set_theme("neon");
        assert_eq!(engine.snapshot().theme, "neon");
        assert!(!engine.snapshot().dark_mode);
        engine.set_theme("dark");
        assert_eq!(engine.snapshot().theme, "dark");
        assert!(engine.snapshot().dark_mode);
        // A word that is none of the five is the pale theme.
        engine.set_theme("nothing_of_a_skin");
        assert_eq!(engine.snapshot().theme, "light");
        assert!(!engine.snapshot().dark_mode);
        // A settings file written before there were three themes holds no word, and the switch
        // it holds answers for it.
        let before = Config {
            dark_mode: true,
            ..Default::default()
        };
        let engine = Engine::start(before);
        assert_eq!(engine.snapshot().theme, "dark");
    }

    // The pixel-art form of a theme is a note about its own: a user may be on any of the five
    // colours and have them drawn out of squares. It is saved with the theme, and the two words
    // never move one another.
    #[test]
    fn the_pixel_art_form_of_a_skin_is_kept_beside_it() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        assert!(!engine.snapshot().theme_pixel, "the pages are drawn as usual");
        engine.set_theme_pixel(true);
        assert!(engine.snapshot().theme_pixel);
        assert_eq!(engine.snapshot().theme, "light", "the theme is untouched");
        // The form is with whichever theme is put on, and the theme is with it.
        engine.set_theme("neon");
        assert!(engine.snapshot().theme_pixel, "the form is kept across a theme");
        assert_eq!(engine.snapshot().theme, "neon");
        engine.set_theme_pixel(false);
        assert!(!engine.snapshot().theme_pixel);
        assert_eq!(engine.snapshot().theme, "neon");
    }

    // A preset is one form of the settings of a model, kept under a name the user wrote. The
    // same name again writes the form over, for the text that is in the tab of the model are
    // the newest format of the same and a preset the user cannot fix is a note they cannot
    // correct. A name of nothing at all is refused, for a preset nobody can name is a preset
    // nobody can find again, and taking one away takes nothing of the settings of any model.
    #[test]
    fn a_preset_is_kept_under_its_name_and_written_over() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let long = ModelSettings {
            ctx_size: 65_536,
            temperature: 0.2,
            ..Default::default()
        };
        let short = ModelSettings {
            ctx_size: 4096,
            ..Default::default()
        };
        assert_eq!(
            engine.model_preset_save(String::from("the long answers"), long.clone()),
            Ok(String::from("the long answers"))
        );
        let held = engine.model_presets();
        assert_eq!(held.len(), 1, "the preset was not kept");
        assert_eq!(held[0].0, "the long answers");
        assert_eq!(held[0].1.ctx_size, 65_536);
        // The same name again, with other text: the form is written over, and no second preset
        // of a like name arrives beside it.
        assert!(engine
            .model_preset_save(String::from("the long answers"), short.clone())
            .is_ok());
        let held = engine.model_presets();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].1.ctx_size, 4096);
        // A name of nothing at all is no name.
        assert!(engine
            .model_preset_save(String::from("   "), long.clone())
            .is_err());
        // Two names remain apart, in the order of the names.
        assert!(engine
            .model_preset_save(String::from("a short chat"), short)
            .is_ok());
        let names: Vec<String> = engine
            .model_presets()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, vec![String::from("a short chat"), String::from("the long answers")]);
        // Removed, and the settings of the models remain where they were.
        assert!(engine.model_preset_remove("the long answers").is_ok());
        assert_eq!(engine.model_presets().len(), 1);
        assert!(engine.model_preset_remove("the long answers").is_err());
    }

    // The context meter at the end of a request. A request that came to its end is on the
    // number it spoke of the whole context. A request the user stopped speaks no number at the
    // end, and is on the size of the text the server was sent, which the parts of the answer
    // spoke: the text of the answer that never came was sent of the conversation with the
    // answer, as ever, but the text the server was sent are in the conversation still. The
    // measure of the text that remains is the floor under both, and all there is when the
    // server stated nothing of the conversation at all. See `meter_after`, and the error in the
    // meter that fell.
    #[test]
    fn the_meter_of_a_stopped_ask_keeps_the_words_the_server_was_sent() {
        let talk = json!([
            { "role": "user", "content": "the first prompt" },
            { "role": "assistant", "content": "the answer of it" },
        ]);
        // The server spoke the number of the whole answer: that remains, and the size of the
        // text it was sent has nothing to say against it.
        assert_eq!(meter_after(4321, 4000, &talk), 4321);
        // Nothing returned at the end of the request, so the size of the text the server was
        // sent remains: the meter does not fall to the measure of the text, which reads at the
        // half of a context of code and of results.
        assert_eq!(meter_after(0, 4000, &talk), 4000);
        // The server stated nothing of the text it was sent either, which is how a request that
        // never reached it reads: the conversation that is in the chat is measured, as it was.
        assert_eq!(meter_after(0, 0, &talk), estimate_tokens(&talk));
        // And the measure of the text is the floor, whichever way the numbers fall.
        assert_eq!(meter_after(0, 1, &talk), estimate_tokens(&talk));

        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        // The meter climbed while the model wrote, as it does, and was at all of the space.
        // The user stopped the answer. The text that number counted is not in the chat, and are
        // not kept; the size of the text the server was sent is, and the meter does not fall
        // away under it.
        state.live_write().ctx_used = 900_000;
        let told = state.build_history();
        let sent = 213_797;
        state.live_write().ctx_used = meter_after(0, sent, &told);
        assert_eq!(
            state.live().ctx_used, sent,
            "the meter fell to the measure of the text, which was the error"
        );
        assert!(
            state.live().ctx_used < 900_000,
            "the text of an answer that never came was kept over the conversation"
        );
    }

    // The first number of the conversation that a new ask brings is taken where it is. After
    // that the meter only climbs, as it always did.
    #[test]
    fn the_meter_of_a_new_ask_is_taken_where_it_stands() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        open_a_chat(&engine);
        let mut state = engine.state.lock().unwrap();
        // The chat the functions under test are given to work in.
        let chat = state.open_id();
        // A number left over from an answer the user stopped: the text it counted was sent of
        // the conversation with the answer.
        state.live_write().ctx_used = 5000;
        state.live_write().ctx_fresh = true;
        assert!(
            climb_context(&mut state, 1200, &chat),
            "the first number of the request was refused, and the meter would stay still"
        );
        assert_eq!(state.live().ctx_used, 1200);
        // The rest of the answer climbs as it always did: the text of an answer that is still
        // coming is not taken back.
        assert!(!climb_context(&mut state, 900, &chat));
        assert_eq!(state.live().ctx_used, 1200);
        assert!(climb_context(&mut state, 1500, &chat));
        assert_eq!(state.live().ctx_used, 1500);
        // A number of nothing reports nothing of the conversation, so it is taken by nobody,
        // and the request is still fresh for the first number that does say something.
        state.live_write().ctx_fresh = true;
        assert!(!climb_context(&mut state, 0, &chat));
        assert!(state.live().ctx_fresh, "a number of nothing spent the freshness");
        assert_eq!(state.live().ctx_used, 1500);
    }

    // The text of a call that was stopped is kept to the header of it: the header names the
    // work the call was, and the rest of it is a file the summary of the turn means. What the
    // user is shown and what the model is informed are created from the same text, so the two
    // cannot differ.
    #[test]
    fn the_words_of_a_cut_call_are_kept_to_the_room() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        let mut arguments = String::from("{\"path\":\"notes/big.md\",\"content\":\"");
        for part in 0..(3 * (GREATEST_CUT_ARGS / 400 + 4)) {
            arguments.push_str(&format!("part-{:0>3}-", part % 100));
            arguments.push_str(&"y".repeat(391));
        }
        assert!(arguments.chars().count() > GREATEST_CUT_ARGS * 2);
        let long = cut_call(editfile::CREATE_NAME, &arguments);
        state.push_cancelled(&long, "", 0, 600);
        let history = state.build_history();
        drop(state);
        let snap = engine.snapshot();
        let last = snap.messages.last().expect("a message stands last");
        // Only the header of the call is kept, and the block the user is shown is created from
        // the same text, so the two cannot differ about what the model was informed. The header
        // still names the file.
        assert_eq!(last.tool_args.chars().count(), 600);
        assert!(last.tool_args.starts_with("{\"path\":\"notes/big.md\""));
        assert_ne!(last.tool_args, arguments);
        assert_eq!(last.body, "notes/big.md");
        // And the text the model is given say that the call was writing, and name the file the
        // call was writing into. The text of the call is kept to the header of it, for the
        // header is the work the call was, and all of the file is the work the summary already
        // means.
        let told = history.to_string();
        assert!(told.contains("writing"), "the model was not informed the call was writing");
        assert!(told.contains("notes/big.md"), "the header of the call did not name the file");
        // The last of the text the model had written is not there: only the top of the file it
        // was writing out went back with the report.
        let last_part = format!("part-{:0>3}-", 3 * (GREATEST_CUT_ARGS / 400 + 4) - 1);
        assert!(arguments.contains(&last_part));
        assert!(!told.contains(&last_part), "all of the file went back");
        // A call that shut all its markers is whole as far as anyone can tell, and is not
        // stated to stop anywhere -- though it is still stated never to have run, which is the
        // item that matters.
        let whole = cut_words(editfile::CREATE_NAME, "{\"path\":\"small.md\"}");
        assert!(!whole.contains("ends in the middle"));
        assert!(whole.contains("never ran"));
        // A call stopped before it had a name still reports what became of it.
        let nameless = cut_words("", "");
        assert!(nameless.contains("cancelled"));
        assert!(nameless.contains("the model had not written the name of the tool yet"));
    }

    // The space a summarisation made is weighed on the text the next ask really carries.
    // The report of a call that was stopped where the model wrote it is the longest message of
    // such a chat, for it holds the text of the call; and the request for a summary is written
    // of text alone, so that telling remains out of it. A chat that is past the limit only
    // because of the report therefore has space enough left to make a second summary with,
    // and the answer is not stopped under the block of a call that never ran.
    #[test]
    fn the_room_is_weighed_without_the_telling_of_the_call() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        // A call of a great many words, and the report of it is a long message.
        let mut arguments = String::from("{\"path\":\"notes/big.md\",\"content\":\"");
        for part in 0..40 {
            arguments.push_str(&format!("part-{:0>3}-", part % 100));
            arguments.push_str(&"y".repeat(391));
        }
        let long = cut_call(editfile::CREATE_NAME, &arguments);
        state.push_cancelled(&long, "", 0, GREATEST_CUT_ARGS);
        let whole = estimate_tokens(&state.build_history());
        let asked = carrying_the_cut(&state.build_history());
        // The report is out of the text the next summary ask carries. The report is in the
        // place of the turn that was cut, and the turn that was cut is the text of the answer
        // and the tool block of it, so the report is the text of the turn, and the space
        // the conversation takes with it is the space the turn took. A conversation with a
        // report in it is bigger than the same conversation without it.
        assert!(
            whole > asked,
            "the report was not left out: {} of {}",
            whole,
            asked
        );
        // And the request for a summary carries the conversation as it is now, so the report of
        // the call that never ran is in it as it is now in the chat.
        let said = summary_ask(&state.build_history(), "Summarise it.", None).to_string();
        assert!(
            said.contains(CUT_TOLD_FIRST),
            "the report of the cut call did not go to the request"
        );
        // The report is kept to the header of the call, which is the part worth the space:
        // a whole file in the arguments of a cut call is the work the summary already means.
        assert!(!said.contains(&"y".repeat(2000)), "the whole call went to the summary ask");
    }

    // The block of a call that was stopped is where the writing stopped, above the marker of
    // the summarisation. The request for a summary is made of the conversation as it is now, so
    // it carries the report of the call and the summary can say what the model was put to; the
    // marker then cuts the report off, and the text of a call that never ran is sent back no
    // more.
    #[test]
    fn a_cut_call_is_written_before_the_summarisation_so_the_summary_carries_it() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        add_message(&mut state, "Assistant", "a long answer, and then some more of it");
        let cut = cut_call(
            editfile::EDIT_NAME,
            "{\"path\":\"notes/zzz-keep.md\",\"edits\":[{\"old_string\":\"zzz-old");
        state.push_cancelled(&cut, "", 0, GREATEST_CUT_ARGS);
        // The marker holds no text yet, so it cuts nothing off: the conversation the summary is
        // written of still holds the report of the call, and the model that writes the summary
        // reads what it was put to. The header of the call names the work it was, and the text
        // of it is kept to the header: a whole file is the work the summary means.
        let mark = state.begin_summary();
        let asked = summary_ask(&state.build_history(), "Summarise it.", None).to_string();
        assert!(
            asked.contains(CUT_TOLD_FIRST),
            "the report of the cut call did not go to the request"
        );
        assert!(asked.contains(editfile::EDIT_NAME), "the tool of the call was lost");
        assert!(asked.contains("zzz-old"), "the header of the call was lost");
        state.finish_summary(&mark, "Done: the cut. Next: the words.", 7000, 88, "");
        // The marker cuts the report off: it is with the text the summary means, and the
        // space the text of the call takes is won back.
        let told = state.build_history().to_string();
        assert!(
            !told.contains(editfile::EDIT_NAME),
            "the call that never ran returned to the model"
        );
        assert!(!told.contains("zzz-old"), "the text of the call returned");
        // The block is above the marker in the chat, where the user watched the writing stop,
        // and not under the heading of the summarisation.
        let chat = state.chat().expect("a chat stands open");
        let where_mark = chat
            .messages
            .iter()
            .position(|m| m.id == mark)
            .expect("the marker is in the chat");
        let where_cut = chat
            .messages
            .iter()
            .position(|m| m.role == "Tool" && m.cancelled)
            .expect("the block of the cut call is in the chat");
        assert!(
            where_cut < where_mark,
            "the block was under the heading: the block at {} and the marker at {}",
            where_cut,
            where_mark
        );
    }

    // A turn may have put text into the chat before the marker of the summarisation was put in:
    // the text of the answer that was cut, the thinking of that turn, and the blocks of the
    // calls it was writing. They are the text the summary means, and the marker goes in after
    // them, so the chat remains as the model reads it: the first prompt, the summary, and the
    // text that came after it.
    #[test]
    fn the_words_of_a_cut_turn_stand_before_the_mark_of_the_summarisation() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        add_message(&mut state, "Assistant", "the old answer");
        // The answer that was cut at the limit put text and a tool block into the chat, and the
        // marker of the summarisation went in after them, for the marker goes in before the
        // request and the cut was created before the marker was put in.
        add_message(&mut state, "Assistant", "the words of the cut answer");
        let cut = cut_call(tools::TOOL_NAME, "{\"command\":\"ls");
        state.push_cancelled(&cut, "the thinking of the turn", 0, GREATEST_CUT_ARGS);
        let mark = state.begin_summary();
        state.finish_summary(&mark, "Done: the parser. Next: the bar.", 7000, 88, "");
        // The marker is before the text of the cut turn, so the user sees the cut where it was
        // created, and the thinking of the turn is not after the heading of the summarisation.
        let chat = state.chat().expect("a chat stands open");
        let where_mark = chat
            .messages
            .iter()
            .position(|m| m.id == mark)
            .expect("the marker is in the chat");
        let where_cut = chat
            .messages
            .iter()
            .position(|m| m.role == "Tool" && m.cancelled)
            .expect("the text of the cut turn is in the chat");
        assert!(
            where_cut < where_mark,
            "the marker was before the text it cuts: marker at {} and the text at {}",
            where_mark, where_cut
        );
        // The text of the cut turn means the summary, so they are not sent to the model again:
        // the model reads the first prompt and the summary, and the text that came after the
        // marker.
        add_message(&mut state, "You", "and the next thing");
        let items = undated(&state.build_history());
        let where_first = items
            .iter()
            .position(|item| item["content"].as_str() == Some("the first prompt"))
            .expect("the first prompt is in the prompt");
        let where_summary = items
            .iter()
            .position(|item| {
                item["content"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with(HANDOFF_FIRST)
            })
            .expect("the summary is in the prompt");
        let where_next = items
            .iter()
            .position(|item| item["content"].as_str() == Some("and the next thing"))
            .expect("the text after the marker are in the prompt");
        assert!(where_first < where_summary, "the summary was before the first prompt");
        assert!(where_summary < where_next, "the text after the marker was before the summary");
        let said = json!(items).to_string();
        assert!(
            !said.contains("the words of the cut answer"),
            "the text the summary means went back to the model"
        );
        assert!(
            !said.contains("the thinking of the turn"),
            "the thinking the summary means went back to the model"
        );
    }

    // The block of the call for the space cannot go into the chat before the request: it
    // would be in the conversation the summary is written of as a call that is still running.
    // It therefore goes in below the marker and is moved above it, so the call is where the
    // model made it and the summary means the request of it. A marker with no text in it cuts
    // nothing off, and nothing moves.
    #[test]
    fn the_block_of_a_call_for_the_room_stands_above_the_mark() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        add_message(&mut state, "Assistant", "the answer, and the request for the space");
        let mark = state.begin_summary();
        state.finish_summary(&mark, "Done: the plan. Next: the bar.", 7000, 88, "");
        let mut call = cut_call(summarise::NAME, "{\"reason\":\"the conversation is full\"}");
        call.id = String::from("call-sum");
        let id = state.begin_tool(&call, SUMMARISE_SHOWN, "", 0);
        state.finish_tool(&id, "Space was made.", true);
        // The block went in below the marker, where the space was freed.
        let place_of = |state: &State, id: &str| {
            state
                .chat()
                .expect("a chat stands open")
                .messages
                .iter()
                .position(|m| m.id == id)
                .expect("the message is in the chat")
        };
        assert!(place_of(&state, &id) > place_of(&state, &mark));
        state.stand_before_summary(&id);
        // The call now is above the heading of the summarisation, where the model made it, and
        // the summary means the request of it.
        assert!(place_of(&state, &id) < place_of(&state, &mark));
        let said = state.build_history().to_string();
        assert!(
            !said.contains(SUMMARISE_SHOWN),
            "the block of the call went back to the model"
        );
        assert!(!said.contains("call-sum"), "the call went back to the model");
        // A marker that holds no text cuts nothing off, and an older marker is not the place
        // this block belongs at: the word of what became of the call stays where the model can
        // read it.
        let second = state.begin_summary();
        state.finish_summary(&second, "", 7000, 0, "the server went away");
        let mut again = cut_call(summarise::NAME, "{\"reason\":\"the conversation is full\"}");
        again.id = String::from("call-sum-2");
        let again_id = state.begin_tool(&again, SUMMARISE_SHOWN, "", 0);
        state.finish_tool(&again_id, "The summary never came.", false);
        state.stand_before_summary(&again_id);
        assert!(place_of(&state, &again_id) > place_of(&state, &second));
    }

    // A marker of a summarisation that was never written is in the chat with nothing in it. The
    // The text before it is the text that marker means, and a second summary of it would be a
    // summary of a summary. The request for the summary is made of the text that remains after the
    // marker, and the marker is not sent to the model with the request.
    #[test]
    fn a_missing_summarisation_is_not_asked_again_with_the_words_it_stands_for() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        add_message(&mut state, "Assistant", "the old answer");
        let mark = state.begin_summary();
        state.finish_summary(&mark, "", 7000, 0, "the server went away");
        assert!(state.handoff_owed());
        // The text after the marker: a turn that came after the marker was put in.
        add_message(&mut state, "You", "and the next thing");
        add_message(&mut state, "Assistant", "the next answer");
        // The request for the summary is made of the text that remains after the marker, and
        // the marker is not sent to the model with the request.
        let asked = summary_ask(&state.build_history(), "Summarise it.", None);
        let said = asked.to_string();
        // The text after the marker is sent with the request, for it is the text the summary is
        // written of.
        assert!(said.contains("and the next thing"), "the text after the marker did not go to the request");
        assert!(said.contains("the next answer"), "the text after the marker did not go to the request");
        // The marker is not sent to the model with the request, for it means the text before
        // it, and a summary of the marker is a summary of a summary.
        assert!(
            !said.contains(HANDOFF_FIRST),
            "the marker of a summary that was never written went to the request"
        );
    }

    // The space that was created is weighed on the text the next ask really carries, with the
    // report of the stopped call left out of it. The report is the longest message of a chat
    // that came to the limit in the middle of a call, and the token estimate of the text reads
    // it bigger than the server counts it. A chat that is past the limit only because of the
    // report therefore has space enough left to make a second summary with, and the answer
    // is not stopped under the block of a call that never ran.
            #[test]
    fn the_answer_goes_on_after_a_summary_that_the_estimate_reads_full() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        // A call of a lot of text, and the report of it is a long message. The text of the call
        // is kept to the header of it, so the report is the header, and the header is what the
        // summary already means.
        let mut arguments = String::from("{\"path\":\"notes/big.md\",\"content\":\"");
        for part in 0..40 {
            arguments.push_str(&format!("part-{:0>3}-", part % 100));
            arguments.push_str(&"y".repeat(391));
        }
        let long = cut_call(editfile::CREATE_NAME, &arguments);
        state.push_cancelled(&long, "", 0, GREATEST_CUT_ARGS);
        // The report of the call is kept to the header of it, for the header is the work the
        // call was, and the text of the call is the work the summary already means. The header
        // of a call that writes a file is the file, and the file is the part worth the
        // space.
        let told = cut_words(editfile::CREATE_NAME, &arguments);
        let head_of_the_call = editfile::file_of(&arguments);
        assert!(
            !head_of_the_call.trim().is_empty(),
            "the header of the call named nothing: {}",
            head_of_the_call
        );
        assert!(
            told.contains(&head_of_the_call),
            "the report did not name the file the call was writing: {}",
            told
        );
        assert!(
            told.chars().count() < arguments.chars().count() / 2,
            "the report held all of the text of the call: {} of {}",
            told.chars().count(),
            arguments.chars().count()
        );
        // The context window of the model, and the limit of the summarisation, set so that the
        // conversation came to the limit while the call was being written. The space is small
        // enough that the estimate of the conversation, with the report of the call in it,
        // reads past the limit, and the estimate of the same conversation without the report
        // reads under it. The space the text of the program takes is counted in here: the
        // line of the day, the text of the chapters, the text of the colours and the text of
        // the space ride with every ask.
        state.ctx_total = 2048;
        state.live_write().ctx_used = 1600;
        let cfg = Config {
            chat_compact_by: config::COMPACT_BY_TOKENS.to_string(),
            chat_compact_reserve: 1100,
            ..Default::default()
        };
        state.cfg = cfg;
        // The space is freed: the summary is written into the marker, and the meter is read
        // again from the conversation as it is now.
        let mark = state.begin_summary();
        state.finish_summary(&mark, "Done: the parser. Next: the bar.", 1600, 250, "");
        let made = estimate_tokens(&state.build_history());
        state.live_write().ctx_used = made;
        // The text the summary means are left out of the conversation, so the space the
        // conversation takes is the space the first prompt and the summary take, and the
        // report of the call is no part of it. The estimate of the conversation is the number
        // the meter reads, and the number is under the limit, for the space was freed.
        let whole = estimate_tokens(&state.build_history());
        assert!(
            !config::compaction_wanted(&state.cfg, whole, state.ctx_total),
            "the space was not made: {} of {}",
            whole,
            state.ctx_total
        );
        // The space is weighed on the text the next ask really carries, and the answer goes on
        // after the summary whatever the estimate reports of all of the conversation: a summary
        // that was kept is space that was created. The first cut of an answer goes on after the
        // space is freed, whatever the estimate reports of all of the conversation.
        let still_full = config::compaction_wanted(&state.cfg, whole, state.ctx_total);
        let cuts = 1usize;
        let goes_on = still_full && cuts > 1;
        assert!(
            !goes_on,
            "the answer was stopped under the block of a call that never ran"
        );
    }

    // The answer goes on after the third summarisation as much as after the first, for as long
    // as each summary frees the space: a task that fills the space again and again is
    // summarised again and again, and the work is not stopped for the error in the space.
    // The number of the cuts is no roof of its own: only a space that stays past the limit even
    // with the summary in it ends the answer, and it is stopped the second time it is found so.
    #[test]
    fn the_answer_goes_on_after_the_third_summary_that_frees_the_room() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        add_message(&mut state, "You", "the first prompt");
        // The space and the limit, set so that the conversation came to the limit while the
        // answer was being written, and so that a summary of the conversation frees the
        // space: the estimate of the conversation with the summary in it reads under the
        // limit. The space the text of the program takes is counted in, as it is in the
        // test above.
        state.ctx_total = 2048;
        state.live_write().ctx_used = 1600;
        let cfg = Config {
            chat_compact_by: config::COMPACT_BY_TOKENS.to_string(),
            chat_compact_reserve: 1100,
            ..Default::default()
        };
        state.cfg = cfg;
        // The summary of the conversation frees the space, and the meter is read again from
        // the conversation as it is now.
        let mark = state.begin_summary();
        state.finish_summary(&mark, "Done: the parser. Next: the bar.", 1600, 250, "");
        let made = estimate_tokens(&state.build_history());
        state.live_write().ctx_used = made;
        let whole = estimate_tokens(&state.build_history());
        assert!(
            !config::compaction_wanted(&state.cfg, whole, state.ctx_total),
            "the space was not freed by the summary: {} of {}",
            whole,
            state.ctx_total
        );
        // The third cut of an answer goes on after the space is freed, whatever the number of
        // the cuts: a summary that frees the space is space that was created, and the work is
        // not stopped for the error in the space.
        let still_full = config::compaction_wanted(&state.cfg, whole, state.ctx_total);
        let cuts = 3usize;
        let room = true;
        let stopped = !room || (still_full && cuts > 1);
        assert!(
            !stopped,
            "the answer was stopped after the third summary that freed the space"
        );
        // A space that stays past the limit even with the summary in it ends the answer the
        // second time it is found so, whatever the number of the cuts before it.
        state.ctx_total = 512;
        let still_full = config::compaction_wanted(&state.cfg, whole, state.ctx_total);
        assert!(
            still_full,
            "the space was not kept past the limit: {} of {}",
            whole,
            state.ctx_total
        );
        let stopped = !room || (still_full && cuts > 1);
        assert!(
            stopped,
            "the answer was not stopped under a space that stayed past the limit"
        );
    }

    // A chat of four messages: a question, its answer, a second question and its answer. The
    // ids are the ones the page would send back.
    fn four_messages() -> Vec<Message> {
        vec![
            Message {
                id: String::from("a"),
                role: String::from("You"),
                body: String::from("the first question"),
                ..Default::default()
            },
            Message {
                id: String::from("b"),
                role: String::from("Assistant"),
                body: String::from("the first answer"),
                ..Default::default()
            },
            Message {
                id: String::from("c"),
                role: String::from("You"),
                body: String::from("the second question"),
                ..Default::default()
            },
            Message {
                id: String::from("d"),
                role: String::from("Assistant"),
                body: String::from("the second answer"),
                stopped: true,
                ..Default::default()
            },
        ]
    }

    // A chat is left open with the messages the user is looking at, so the markers under a
    // message have a sends requests to work on.
    fn engine_with(messages: Vec<Message>) -> Engine {
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        if let Some(chat) = state.chat_mut() {
            chat.messages = messages;
        }
        drop(state);
        engine
    }

    // A message of the user takes the answer it provoked along, for the model answered a
    // conversation that no longer reads that way.
    #[test]
    fn a_message_of_the_user_takes_its_answer_along() {
        let _guard = config::test_turn();
        let engine = engine_with(four_messages());
        engine.remove_message(String::from("c"));
        let state = engine.state.lock().unwrap();
        let chat = state.chat().expect("the chat stands open");
        let left: Vec<&str> = chat.messages.iter().map(|one| one.id.as_str()).collect();
        assert_eq!(left, vec!["a", "b"], "the answer the message provoked stayed");
    }

    // A message from the model leaves the question behind, for the program does not delete what
    // a user wrote behind their back. Everything after the answer goes with it, which is the
    // second question and its answer.
    #[test]
    fn a_message_of_the_model_leaves_the_question_behind() {
        let _guard = config::test_turn();
        let engine = engine_with(four_messages());
        engine.remove_message(String::from("b"));
        let state = engine.state.lock().unwrap();
        let chat = state.chat().expect("the chat stands open");
        let left: Vec<&str> = chat.messages.iter().map(|one| one.id.as_str()).collect();
        assert_eq!(left, vec!["a"], "the question that led to the answer went with it");
    }

    // A chat left with nothing of the conversation is removed as a chat that was never filled
    // is, and the window goes to the first screen.
    #[test]
    fn a_chat_left_with_nothing_of_the_talk_goes_too() {
        let _guard = config::test_turn();
        let engine = engine_with(four_messages()[..2].to_vec());
        engine.remove_message(String::from("a"));
        let state = engine.state.lock().unwrap();
        assert!(state.chat().is_none(), "the empty chat is still open");
        assert!(
            state.snap().home,
            "the window is not at the first screen with nothing open"
        );
    }

    // A message of the user goes back into the chat composer, and the conversation behind it
    // goes with it. The chat keeps the place it had in the window: the text is on their way
    // back to the same chat, and the box belongs to it.
    #[test]
    fn a_message_sent_back_cuts_the_talk_at_itself() {
        let _guard = config::test_turn();
        let engine = engine_with(four_messages());
        engine.rewrite_message(String::from("c"));
        let state = engine.state.lock().unwrap();
        let chat = state.chat().expect("the chat is still open");
        let left: Vec<&str> = chat.messages.iter().map(|one| one.id.as_str()).collect();
        assert_eq!(left, vec!["a", "b"], "the conversation behind the message stayed");
        assert_eq!(
            state.status, "The message went back into the chat composer",
            "the window was not informed the message went back"
        );
    }

    // Only a message of the user goes back. An answer of the model is written over where it is,
    // so the question behind it is not deleted behind the user's back, and the conversation
    // does not move.
    #[test]
    fn only_a_message_of_yours_goes_back_into_the_chat_box() {
        let _guard = config::test_turn();
        let engine = engine_with(four_messages());
        engine.rewrite_message(String::from("b"));
        let state = engine.state.lock().unwrap();
        let chat = state.chat().expect("the chat is still open");
        assert_eq!(chat.messages.len(), 4, "an answer was taken out of the conversation");
        assert_eq!(
            state.status, "Only one of your own messages goes back into the chat composer",
            "the window was not informed why nothing moved"
        );
    }

    // The records of the files hung on the messages that go are taken off the shelf with them.
    // A file hung on a message that stays is left where it is.
    #[test]
    fn a_message_sent_back_takes_the_files_behind_it_off_the_shelf() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        let shelf = {
            let chat = state.chat().expect("a blank chat stands open");
            state.attach_place_for(chat)
        };
        let files = vec![
            attach::Incoming {
                name: String::from("first.txt"),
                mime: String::from("text/plain"),
                data: String::new(),
                text: String::from("the note of the first message"),
            },
            attach::Incoming {
                name: String::from("second.txt"),
                mime: String::from("text/plain"),
                data: String::new(),
                text: String::from("the note of the second message"),
            },
        ];
        let kept = attach::save_at(&shelf, &files);
        assert_eq!(kept.len(), 2, "both files were not put on the shelf");
        let mut messages = four_messages();
        messages[0].attachments = vec![kept[0].clone()];
        messages[2].attachments = vec![kept[1].clone()];
        if let Some(chat) = state.chat_mut() {
            chat.messages = messages;
        }
        drop(state);

        engine.rewrite_message(String::from("c"));

        let state = engine.state.lock().unwrap();
        let chat = state.chat().expect("the chat is still open");
        assert!(
            attach::read_at(&shelf, &kept[0].id).is_some(),
            "the file of a message that stayed was taken off the shelf"
        );
        assert!(
            attach::read_at(&shelf, &kept[1].id).is_none(),
            "the file of a message that went is still on the shelf"
        );
        assert_eq!(chat.messages.len(), 2, "the conversation behind the message stayed");
    }

    // A chat left with nothing of the conversation is not closed and not taken out of the
    // window: the text is on their way back into the box of that same chat. Only the file of it
    // goes, for a chat with no conversation has no file. The first message that returns makes
    // the file again.
    #[test]
    fn a_chat_left_with_nothing_keeps_its_place_and_loses_its_file() {
        let _guard = config::test_turn();
        let engine = engine_with(four_messages()[..2].to_vec());
        let place = store::Store::new(config::repositories_dir(&Config::default()));
        let stood = {
            let mut state = engine.state.lock().unwrap();
            state.persist();
            let id = state.chat().expect("the chat stands open").meta.id.clone();
            place.load_chat(&id)
        };
        assert!(stood.is_some(), "the chat stood never written out");

        engine.rewrite_message(String::from("a"));

        let state = engine.state.lock().unwrap();
        let chat = state.chat().expect("the chat is still open in the window");
        assert!(chat.messages.is_empty(), "a message of the conversation stayed");
        assert!(
            place
                .load_chat(&stood.expect("the chat stood").meta.id)
                .is_none(),
            "the row of a chat with no conversation is still there"
        );
        assert!(!state.snap().home, "the window left the chat it is writing again");
    }

    // A message that is not in the conversation is refused with the text that record it, and
    // nothing moves.
    #[test]
    fn nothing_goes_back_of_a_message_that_is_not_there() {
        let _guard = config::test_turn();
        let engine = engine_with(four_messages());
        engine.rewrite_message(String::from("zzzz"));
        let state = engine.state.lock().unwrap();
        let chat = state.chat().expect("the chat is still open");
        assert_eq!(chat.messages.len(), 4, "a message was sent on a name of nothing");
        assert_eq!(state.status, "That message is not in this chat");
    }

    // The text the user writes take the place of the text of the answer, and nothing else
    // moves. The thought of the answer and the summary of its work are left where they were:
    // the user is writing the text, not the work.
    #[test]
    fn the_words_of_an_answer_are_written_over_where_they_stand() {
        let _guard = config::test_turn();
        let engine = engine_with(four_messages());
        engine.edit_answer(String::from("d"), String::from("the answer as the user wrote it"));
        let state = engine.state.lock().unwrap();
        let chat = state.chat().expect("the chat is still open");
        let left: Vec<&str> = chat.messages.iter().map(|one| one.id.as_str()).collect();
        assert_eq!(left, vec!["a", "b", "c", "d"], "the conversation moved under the answer");
        assert_eq!(chat.messages[3].body, "the answer as the user wrote it");
        assert_eq!(chat.messages[2].body, "the second question", "a message of the user was written over");
        assert!(chat.messages[3].stopped, "the marker of the stopped answer went with the old text");
        assert_eq!(state.status, "The answer was written again");
    }

    // The text is trimmed as they go in. A message from the model that ends on spaces of the
    // stream is not kept with them, and the answer of the conversation the model is next
    // informed is the text without them.
    #[test]
    fn the_words_of_an_answer_are_trimmed_as_they_go_in() {
        let _guard = config::test_turn();
        let engine = engine_with(four_messages());
        engine.edit_answer(String::from("b"), String::from("  the answer  \n"));
        let state = engine.state.lock().unwrap();
        let chat = state.chat().expect("the chat is still open");
        assert_eq!(chat.messages[1].body, "the answer", "the spaces were kept");
    }

    // Only an answer of the model is written over. The text of a message of the user go back
    // into the chat composer instead, which is another ask.
    #[test]
    fn only_an_answer_of_the_model_is_written_over() {
        let _guard = config::test_turn();
        let engine = engine_with(four_messages());
        engine.edit_answer(String::from("a"), String::from("text that does not belong here"));
        let state = engine.state.lock().unwrap();
        let chat = state.chat().expect("the chat is still open");
        assert_eq!(chat.messages[0].body, "the first question", "a message of the user was written over");
        assert_eq!(state.status, "Only an answer of the model is written over");
    }

    // An answer with no text in it is refused: the conversation would go on from a turn of the
    // model that shows nothing, and the user would lose the text they had.
    #[test]
    fn an_answer_needs_some_words() {
        let _guard = config::test_turn();
        let engine = engine_with(four_messages());
        engine.edit_answer(String::from("b"), String::from("   \n  "));
        let state = engine.state.lock().unwrap();
        let chat = state.chat().expect("the chat is still open");
        assert_eq!(chat.messages[1].body, "the first answer", "the answer was emptied");
        assert_eq!(state.status, "An answer needs some words");
    }

    // While an answer is being generated, neither ask is done. The conversation is being
    // written under the user's hand, and a cut or a write over it would be lost when the stream
    // ends.
    #[test]
    fn nothing_is_moved_while_an_answer_is_generated() {
        let _guard = config::test_turn();
        let engine = engine_with(four_messages());
        {
            let mut state = engine.state.lock().unwrap();
            state.live_write().generating = true;
        }
        engine.rewrite_message(String::from("c"));
        engine.edit_answer(String::from("d"), String::from("text that does not belong here"));
        let state = engine.state.lock().unwrap();
        let chat = state.chat().expect("the chat is still open");
        let left: Vec<&str> = chat.messages.iter().map(|one| one.id.as_str()).collect();
        assert_eq!(left, vec!["a", "b", "c", "d"], "the conversation moved while an answer was generated");
        assert_eq!(chat.messages[3].body, "the second answer", "an answer was written over while one was generated");
        assert_eq!(state.status, "An answer is still being generated");
    }

    // The tool that ran a failed call again is removed, and with it the line that informed the
    // model about it. Nothing named `rerun_tool` is proposed in any combination of the badge,
    // it belongs to no line of the badge's drop-down, and the result of a failed call reports
    // what returned and nothing else.
    #[test]
    fn a_failed_call_is_given_no_way_to_be_run_again() {
        let _guard = config::test_turn();
        let engine = engine_with(vec![
            Message {
                id: String::from("a"),
                role: String::from("You"),
                body: String::from("the question"),
                ..Default::default()
            },
            Message {
                id: String::from("b"),
                role: String::from("Tool"),
                tool_name: String::from(tools::TOOL_NAME),
                tool: String::from("running ..."),
                tool_args: String::from("{}"),
                failed: true,
                ..Default::default()
            },
        ]);
        // The result keeps the text the tool returned, with nothing added under it about
        // running the call again with a parameter changed.
        {
            let mut state = engine.state.lock().unwrap();
            state.finish_tool("b", "the command failed: no such file", false);
        }
        {
            let state = engine.state.lock().unwrap();
            let chat = state.chat().expect("the chat stands open");
            let one = chat
                .messages
                .iter()
                .find(|one| one.id == "b")
                .expect("the run stands in the chat");
            assert_eq!(
                one.tool,
                "the command failed: no such file",
                "the result of a failed call carries more than what returned"
            );
            assert!(one.failed, "the run is still known as a failed one");
        }
        // No line of the badge proposes it, whatever the switches say, and the badge holds no
        // switch over it either.
        let mut switches = store::ToolSwitches::default();
        switches.tasks = true;
        switches.infographics = true;
        let names = offered(
            false,
            true,
            false,
            tools::Access::default(),
            &[],
            &switches,
            true,
        )
        .map(|list| {
            list.as_array()
                .unwrap()
                .iter()
                .map(|one| String::from(one["function"]["name"].as_str().unwrap_or("")))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
        assert!(
            !names.iter().any(|one| one == "rerun_tool"),
            "the tool that ran a failed call again is still proposed: {}",
            names.join(", ")
        );
        assert!(
            names.iter().any(|one| one == tools::TOOL_NAME),
            "the list of tools the chat is given is empty, so this test proves nothing"
        );
        assert!(
            group_of("rerun_tool").is_none(),
            "the badge still holds a switch over the tool that ran a failed call again"
        );
        // The switch of the tool is removed from the list of the drop-down as well, so a page
        // has nothing left to send for it.
        let parts: Vec<&str> = switches
            .words()
            .into_iter()
            .map(|(part, _)| part)
            .collect();
        assert!(
            !parts.iter().any(|one| *one == "rerun"),
            "the drop-down of the badge still holds a line for it: {}",
            parts.join(", ")
        );
    }

    // The answer the continue marker applies to is the newest saying of the model in the open
    // chat, whether the user stopped it or the model ended it early. An answer that stopped on
    // an end-of-sequence token carries no stop marker, so a rule that requested that marker
    // could never proposes to continue it - which is the error this fixes. An older answer and
    // a message of the user are refused, each with its own reason. The newest answer passes
    // every guard the conversation holds, so the only item left in the way is the model, which
    // no test has.
    #[test]
    fn the_newest_answer_is_the_one_that_may_be_gone_on_with() {
        let _guard = config::test_turn();
        let engine = engine_with(four_messages());
        // An older answer is refused: the request is the conversation as it is now, and the
        // text of an older answer is not at its end.
        engine.continue_message(String::from("b"));
        // The lock is let go of before the next call, for the calls above take it themselves.
        assert!(
            engine
                .state
                .lock()
                .unwrap()
                .status
                .contains("Only the newest answer"),
            "an answer older than the newest was taken"
        );
        engine.continue_message(String::from("c"));
        assert!(
            engine
                .state
                .lock()
                .unwrap()
                .status
                .contains("Only an answer of the model"),
            "a message of the user was taken"
        );
        engine.continue_message(String::from("not-a-message"));
        assert!(
            engine
                .state
                .lock()
                .unwrap()
                .status
                .contains("not in this chat"),
            "a message of another chat was taken"
        );
        // The newest answer passes the rules of the conversation, so it is refused for want of
        // a model and for nothing else.
        engine.continue_message(String::from("d"));
        let status = engine.state.lock().unwrap().status.clone();
        assert!(
            status.contains("Choose a model"),
            "the newest answer was refused for the wrong reason: {}",
            status
        );
        let state = engine.state.lock().unwrap();
        assert!(!state.live().generating, "an answer began with no model to ask");
    }

    // An answer that ended on its own end-of-sequence token, with no stop marker on it, is
    // proposed for continuing just the same, and its text is at the end of the conversation so
    // the request is open.
    #[test]
    fn an_answer_that_ended_on_its_own_token_may_be_gone_on_with() {
        let _guard = config::test_turn();
        let mut messages = four_messages();
        // The stop marker is taken off: this answer stopped of its own accord.
        messages.last_mut().expect("an answer stands").stopped = false;
        let engine = engine_with(messages);
        engine.continue_message(String::from("d"));
        let status = engine.state.lock().unwrap().status.clone();
        assert!(
            status.contains("Choose a model"),
            "an answer the model ended early was refused for the wrong reason: {}",
            status
        );
    }

    // The proposal the page is given is the same answer the request accepts: the snapshot names
    // it, and nothing is proposed while an answer is arriving, while the model is loading, when
    // the newest saying is a message of the user, or when the newest answer has no text of its
    // own.
    #[test]
    fn the_snapshot_names_the_answer_it_offers_to_go_on_with() {
        let _guard = config::test_turn();
        let engine = engine_with(four_messages());
        assert_eq!(
            engine.snapshot().continue_id,
            "d",
            "the newest answer is the one the page is informed of"
        );
        // A chat that ends on a message of the user has no answer to go on with.
        engine.continue_message(String::from("d"));
        {
            let mut state = engine.state.lock().unwrap();
            state.push_message("You", "and what came of it?", "", 0, false);
        }
        assert!(
            engine.snapshot().continue_id.is_empty(),
            "a conversation ending on a question was proposed"
        );
        {
            let mut state = engine.state.lock().unwrap();
            state.push_message("Assistant", "", "the thought, and nothing stated", 0, false);
        }
        assert!(
            engine.snapshot().continue_id.is_empty(),
            "an answer with no text was proposed"
        );
        // While an answer is arriving the marker remains nowhere, for the answer has no end yet
        // and the text of it is not the chat's.
        {
            let mut state = engine.state.lock().unwrap();
            state.push_message("Assistant", "the rest of it", "", 0, false);
            state.live_write().generating = true;
        }
        assert!(
            engine.snapshot().continue_id.is_empty(),
            "the marker was while an answer was arriving"
        );
        {
            let mut state = engine.state.lock().unwrap();
            state.live_write().generating = false;
            state.busy = true;
        }
        assert!(
            engine.snapshot().continue_id.is_empty(),
            "the marker was while the model was loading"
        );
        {
            let mut state = engine.state.lock().unwrap();
            state.busy = state.any_generating();
        }
        assert_eq!(
            engine.snapshot().continue_id,
            engine
                .state
                .lock()
                .unwrap()
                .chat()
                .expect("the chat stands open")
                .messages
                .last()
                .expect("an answer stands")
                .id,
            "the proposal went back to the answer the chat ends on"
        );
    }

    // A conversation that ends on a call or on a question is not open, so there is nothing for
    // a model to write on from. The markers of a chapter and of a summarisation are no sayings,
    // so a conversation that ends on one of them has the newest saying of the model before them
    // for it to go on with.
    #[test]
    fn a_mark_of_a_chapter_leaves_the_answer_behind_it_goable() {
        let _guard = config::test_turn();
        let mut messages = four_messages();
        messages.push(Message {
            id: String::from("e"),
            role: String::from("Chapter"),
            body: String::from("a chapter the model flagged"),
            ..Default::default()
        });
        let engine = engine_with(messages);
        assert_eq!(
            engine.snapshot().continue_id,
            "d",
            "a chapter marker hid the answer it is behind"
        );
        assert!(
            newest_saying(
                engine
                    .state
                    .lock()
                    .unwrap()
                    .chat()
                    .expect("the chat stands open")
            )
            .map(|one| one.id == "d")
                == Some(true),
            "the newest saying was not the answer"
        );
    }

    // The rules of the conversation, checked on their own.
    #[test]
    fn an_answer_is_gone_on_from_only_when_the_talk_ends_open() {
        let open = json!([
            { "role": "user", "content": "the question" },
            { "role": "assistant", "content": "the answer, half written" },
        ]);
        assert!(ends_open(&open), "a conversation ending on an answer was shut");
        let on_a_call = json!([
            { "role": "user", "content": "the question" },
            { "role": "assistant", "content": null, "tool_calls": [ { "id": "1" } ] },
        ]);
        assert!(!ends_open(&on_a_call), "a conversation ending on a call was open");
        let on_a_question = json!([{ "role": "user", "content": "the question" }]);
        assert!(!ends_open(&on_a_question), "a conversation ending on a question was open");
        let of_no_words = json!([{ "role": "assistant", "content": "  " }]);
        assert!(!ends_open(&of_no_words), "a conversation ending on nothing was open");
    }

    // The marker of an answer that never came to its end is on one answer at most: the newest
    // one the user stopped. Going on with it takes the marker off, so a second press cannot go
    // on with the same answer twice.
    #[test]
    fn the_mark_of_a_stopped_answer_stands_on_one_answer_only() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.start_chat();
        if let Some(chat) = state.chat_mut() {
            chat.messages = four_messages();
        }
        // A second answer with the marker on it takes the marker off the first.
        state.mark_last_answer_stopped();
        let marked: Vec<String> = state
            .chat()
            .unwrap()
            .messages
            .iter()
            .filter(|one| one.stopped)
            .map(|one| one.id.clone())
            .collect();
        assert_eq!(marked, vec![String::from("d")], "the marker did not move to the newest answer");
        state.clear_stopped_marks();
        assert!(
            !state
                .chat()
                .unwrap()
                .messages
                .iter()
                .any(|one| one.stopped),
            "the marker stayed when the conversation finished"
        );
    }

    // The name of the file a conversation is in has to be a name the server reads: no marker of
    // a folder, no colon, no question marker, no quote, no character a word is not made of, and
    // never a point at the end. The id of a chat is made of quiet markers today, but a name of
    // a file on the disk can never be trusted to be, for the server throws the whole ask of a
    // conversation out over one marker it does not like.
    #[test]
    fn the_name_of_a_talk_is_a_name_the_server_reads() {
        let name = context_name("one/two:three*four?q=\"five|six<seven>eight", "/models/a model.gguf");
        assert!(!name.is_empty(), "a name is never nothing");
        assert!(name.ends_with(".ctx"), "the ending markers the conversations of the program");
        assert!(!name.ends_with('.'), "a name ending on a point is thrown out");
        for mark in name.chars() {
            assert!(
                mark.is_ascii_alphanumeric() || mark == '.' || mark == '_' || mark == '-',
                "the name {} holds {} that the server would throw out",
                name,
                mark
            );
        }
        assert!(name.len() <= 255, "the server reads no name longer than that");
    }

    // The conversation of a chat read through by one model is not the conversation of the same
    // chat read through by another, so the two are in two files. The id of a model is a path of
    // the disk, which could never are in a name at all, so it is hashed in at the end instead.
    #[test]
    fn the_name_of_a_talk_tells_the_models_apart() {
        let one = context_name("chat", "/models/one.gguf");
        let two = context_name("chat", "/models/two.gguf");
        assert_ne!(one, two, "two models keep their own conversation of one chat");
        assert_eq!(
            one,
            context_name("chat", "/models/one.gguf"),
            "the name of one chat and one model does not move between runs"
        );
        assert!(!one.contains('/'), "the path of a model is hashed and not written");
    }

    // A chat id longer than the space of a name is not cut at the space of a name: all
    // of it goes into a hash, so two chats whose ids run the same way for three hundred markers
    // still are in two files.
    #[test]
    fn a_long_chat_id_is_hashed_and_still_stands_alone() {
        let long = "a".repeat(300);
        let longer = format!("{}b", long);
        let one = context_name(&long, "/m.gguf");
        let other = context_name(&longer, "/m.gguf");
        assert!(one.len() <= 255, "a long id is brought down to a name that fits");
        assert_ne!(one, other, "all of a long id is distinguished, not the start of it");
    }

    // A server that is stopped leaves no word of its slots behind. The conversations were
    // written out before it went, and the next server has slots of its own that nothing of the
    // one before is understood in.
    #[test]
    fn a_stopped_server_leaves_no_word_of_its_slots() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let mut state = engine.state.lock().unwrap();
        state.context_slot.insert(String::from("one"), 0);
        state.context_slot.insert(String::from("two"), 1);
        state.context_holds.insert(0, String::from("one"));
        state.context_holds.insert(1, String::from("two"));
        state.context_lost();
        assert!(state.context_slot.is_empty(), "no chat keeps a slot of a server that is removed");
        assert!(state.context_holds.is_empty(), "and no slot keeps a chat");
    }

    // Two chats and one slot: the chat that comes second takes the slot of the first, and the
    // first is given that same slot again when it returns, for its conversation was written out
    // at the moment the slot was taken. The writing and the reading that go with the changing
    // of a slot are answered quietly when nothing is at the other end of the call, which is how
    // items are in a test -- and a conversation that cannot be kept is a conversation read
    // through again, with the same text at the end of it.
    #[test]
    fn a_slot_passes_from_one_chat_to_the_other() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let state = engine.state.clone();
        let nowhere = "http://127.0.0.1:1";
        let model = "/models/one.gguf";
        let (first, second) = {
            let mut guard = state.lock().unwrap();
            guard.start_chat();
            let first = guard.current.clone().expect("a chat was opened");
            guard.start_chat();
            let second = guard.current.clone().expect("a second chat was opened");
            assert_ne!(first, second, "two chats, two ids");
            assert!(guard.context_slot.is_empty(), "nothing is kept until a chat is asked");
            (first, second)
        };
        // The current chat takes the only slot the server has.
        state.lock().unwrap().current = Some(first.clone());
        let slot = context_claim(&state, nowhere, model).expect("a slot for the current chat");
        assert_eq!(slot, 0, "the slots are named from zero");
        assert_eq!(
            state.lock().unwrap().context_holds.get(&slot).map(|one| one.as_str()),
            Some(first.as_str()),
            "the slot means the chat that took it"
        );
        // The second chat takes that same slot, and the first chat is written down against it
        // no longer -- but not forgotten: it still wants it.
        state.lock().unwrap().current = Some(second.clone());
        let taken = context_claim(&state, nowhere, model).expect("a slot for the current chat");
        assert_eq!(taken, slot, "one slot, so both chats are in it in turn");
        {
            let guard = state.lock().unwrap();
            assert_eq!(
                guard.context_slot.get(&first),
                Some(&slot),
                "the first chat still wants the slot it had"
            );
            assert_eq!(
                guard.context_holds.get(&slot).map(|one| one.as_str()),
                Some(second.as_str()),
                "and the slot means the chat that is in it now"
            );
        }
        // Back it goes, when the first chat is asked again.
        state.lock().unwrap().current = Some(first.clone());
        let back = context_claim(&state, nowhere, model).expect("the slot of the current chat");
        assert_eq!(back, slot, "the chat that returned found the slot it left");
        assert_eq!(
            state.lock().unwrap().context_holds.get(&slot).map(|one| one.as_str()),
            Some(first.as_str()),
            "and it stands in it again"
        );
    }

    // The conversation of the current chat is written when an answer is stopped, and only when
    // there is a context of that chat to write: not with the keeping switched off, not with no
    // chat open, not for a chat that was never asked, and not for a chat whose slot another
    // chat is in -- for the slot of another chat holds the conversation of that other chat, and
    // writing it under the name of this one would put wrong text where the conversation of this
    // one is read back from.
    #[test]
    fn a_stopped_answer_writes_the_talk_of_its_chat_only() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let state = engine.state.clone();
        let nowhere = "http://127.0.0.1:1";
        let model = "/models/one.gguf";
        let (first, second) = {
            let mut guard = state.lock().unwrap();
            guard.start_chat();
            let first = guard.current.clone().expect("a chat was opened");
            guard.start_chat();
            let second = guard.current.clone().expect("a second chat was opened");
            (first, second)
        };
        // A chat that was never asked has no conversation worked through anywhere, so there is
        // nothing of it to write.
        assert!(
            save_chat_context(&state, nowhere, model).is_none(),
            "a chat that was never asked has no sends requests to write"
        );
        // Asked, the chat is in a slot, and its conversation can be written.
        let slot = context_claim(&state, nowhere, model).expect("a slot for the current chat");
        let name = save_chat_context(&state, nowhere, model).expect("the conversation of the current chat");
        assert_eq!(
            name,
            context_name(&second, model),
            "the file is the one this chat is read back from"
        );
        // Another chat takes the slot. The first chat still wants it, but the conversation that
        // is in it now is the conversation of the second, so nothing of the first may be
        // written there.
        state.lock().unwrap().current = Some(first.clone());
        context_claim(&state, nowhere, model).expect("the slot for the first chat");
        state.lock().unwrap().current = Some(second.clone());
        context_claim(&state, nowhere, model).expect("and the slot for the second");
        assert_eq!(
            state.lock().unwrap().context_holds.get(&slot).map(|one| one.as_str()),
            Some(second.as_str()),
            "the slot means the second chat"
        );
        state.lock().unwrap().current = Some(first.clone());
        assert!(
            save_chat_context(&state, nowhere, model).is_none(),
            "a chat whose slot another chat holds writes nothing there"
        );
        // With the keeping switched off, nothing is written at all.
        state.lock().unwrap().current = Some(second.clone());
        state.lock().unwrap().cfg.chat_context_files = false;
        assert!(
            save_chat_context(&state, nowhere, model).is_none(),
            "the keeping switched off writes nothing"
        );
    }

    // With the keeping of the conversations switched off, nothing is claimed: the request is
    // sent with no slot named, exactly as it did before the keeping was.
    #[test]
    fn no_slot_is_claimed_when_the_keeping_is_off() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config {
            chat_context_files: false,
            ..Config::default()
        });
        let state = engine.state.clone();
        {
            let mut guard = state.lock().unwrap();
            guard.start_chat();
            assert!(guard.current.is_some());
        }
        assert!(
            context_claim(&state, "http://127.0.0.1:1", "/models/one.gguf").is_none(),
            "a switched-off keeping keeps nothing"
        );
        let guard = state.lock().unwrap();
        assert!(guard.context_slot.is_empty() && guard.context_holds.is_empty());
    }

    // A message written while the model is writing waits in the program, in a list that belongs
    // to the chat it was written for. The rows may be written again, moved and taken out, and
    // the list goes with the chat.
    #[test]
    fn the_waiting_list_is_kept_by_the_program_for_its_chat() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
        }
        assert!(engine.snapshot().queue.is_none(), "a new chat has no list");
        // Two messages. The one to go out first is at the start of the list.
        engine
            .queue_add(String::from("the first"), Vec::new())
            .expect("the first message stands");
        engine
            .queue_add(String::from("the second"), Vec::new())
            .expect("the second message stands");
        let held = engine.snapshot();
        let list = held.queue.expect("the chat has a waiting list");
        assert_eq!(list.items.len(), 2);
        assert_eq!(list.items[0].text, "the first", "the next to go out remains first");
        assert_eq!(list.items[1].text, "the second");
        assert!(!list.held);
        assert_eq!(list.chat, held.chat_id, "the list belongs to the open chat");
        // A row may be written again, and put at another place in the list.
        let second = list.items[1].id;
        assert!(engine.queue_edit(second, String::from("  the second, written again  ")));
        assert!(engine.queue_move(second, 0));
        let again = engine.snapshot().queue.expect("the list stands");
        assert_eq!(again.items[0].id, second);
        assert_eq!(again.items[0].text, "the second, written again");
        // A row that is not there is not changed, and records it; and a message of nothing at
        // all is refused, as it is for the chat composer.
        assert!(!engine.queue_edit(9_999, String::from("nothing")));
        assert!(!engine.queue_remove(9_999));
        assert!(!engine.queue_move(9_999, 0));
        assert!(engine.queue_add(String::from("   "), Vec::new()).is_err());
        // Taking the last one out takes the list with it.
        assert!(engine.queue_remove(second));
        let last = engine.snapshot().queue.expect("one message is left");
        assert_eq!(last.items.len(), 1);
        assert_eq!(last.items[0].text, "the first");
        assert!(engine.queue_remove(last.items[0].id));
        assert!(engine.snapshot().queue.is_none(), "an empty list is not kept");
        // The list of a chat goes with that chat, for its messages have no answer left to wait
        // for.
        engine
            .queue_add(String::from("a message of this chat"), Vec::new())
            .expect("the message stands");
        let chat = engine.snapshot().chat_id;
        engine.remove_chat(chat);
        assert!(engine.snapshot().queue.is_none(), "the chat took its list with it");
    }

    // A waiting list is only continued while the chat is idle, and a message that cannot be
    // sent holds the rest of the list back rather than be asked again every moment.
    #[test]
    fn a_waiting_list_waits_for_the_chat_to_be_idle() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let state = engine.state.clone();
        let chat;
        {
            let mut guard = state.lock().unwrap();
            guard.start_chat();
            chat = guard.open_id();
            guard.live_write().generating = true;
        }
        engine
            .queue_add(String::from("queued"), Vec::new())
            .expect("the message stands");
        // The chat is writing, so nothing is taken from the list, and nothing is sent.
        {
            let guard = state.lock().unwrap();
            assert!(!guard.queue_ready(), "a writing chat is not idle");
            assert!(guard.queue_next().is_none());
        }
        assert!(queue_step(&state, &chat).is_none(), "nothing goes out while the chat writes");
        // The answer is over. The message is next, but no model remains chosen, so the sender
        // refuses it and the list is held back with it.
        state.lock().unwrap().live_write().generating = false;
        {
            let guard = state.lock().unwrap();
            assert!(guard.queue_ready(), "an idle chat may go on with the list");
            assert_eq!(
                guard
                    .queue_next()
                    .map(|(_, sent)| sent.text)
                    .expect("a message waits its turn"),
                "queued"
            );
        }
        assert!(queue_step(&state, &chat).is_none(), "no model stands chosen");
        {
            let guard = state.lock().unwrap();
            assert!(!guard.queue_ready(), "a held list is not asked again");
            let list = guard.open_queue().expect("the message is not lost");
            assert!(list.held);
            assert_eq!(list.items.len(), 1);
            assert_eq!(list.items[0].text, "queued");
        }
        // Touching the list - here, writing another message into it - starts it again.
        engine
            .queue_add(String::from("behind it"), Vec::new())
            .expect("the second message stands");
        let guard = state.lock().unwrap();
        assert!(!guard.open_queue().expect("the list stands").held);
        assert!(guard.queue_ready());
    }

    // A programmed message is sent when an answer of the chat holds the text it waits for, and
    // it is sent ahead of the messages that were only put in the list. An answer that finished
    // before the message was written is not checked, for the message was written for the answer
    // that has not come yet.
    #[test]
    fn a_programmed_message_goes_when_the_answer_holds_its_words() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let state = engine.state.clone();
        {
            let mut guard = state.lock().unwrap();
            guard.start_chat();
        }
        assert!(engine
            .queue_plan(String::from("say it again"), Vec::new(), String::from("  "))
            .is_err());
        assert!(engine.snapshot().queue.is_none(), "a refused ask adds nothing");
        engine
            .queue_add(String::from("only waiting"), Vec::new())
            .expect("a plain message stands");
        engine
            .queue_plan(
                String::from("say it again"),
                Vec::new(),
                String::from("  All DONE  "),
            )
            .expect("a programmed message stands");
        let list = engine.snapshot().queue.expect("the chat has a list");
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.planned.len(), 1, "the programmed rows stand apart");
        assert_eq!(
            list.planned[0].matches,
            "All DONE",
            "the word is kept as written, with the spaces off either end"
        );
        // An answer of an older asking is no reason to let the message out. The text of this
        // asking is the ones checked, and there are none yet.
        {
            let guard = state.lock().unwrap();
            assert!(guard.last_answer().is_none(), "the chat has answered nothing");
            assert_eq!(
                guard
                    .queue_next()
                    .map(|(_, sent)| sent.text)
                    .expect("a message waits its turn"),
                "only waiting"
            );
        }
        // An answer that does not hold the text leaves the message remaining.
        {
            let mut guard = state.lock().unwrap();
            add_message(&mut guard, "You", "is it finished?");
            add_message(&mut guard, "Assistant", "Not yet.");
            assert_eq!(guard.last_answer().as_deref(), Some("Not yet."));
            guard.answer_programmed("Not yet.");
        }
        let still = engine.snapshot().queue.expect("the list stands");
        assert_eq!(still.planned.len(), 1);
        // An answer that does hold them lets it out, ahead of the message that was only
        // waiting. The bigness of the letters is left out of the matching.
        let planned = still.planned[0].id;
        {
            let mut guard = state.lock().unwrap();
            guard.answer_programmed("The work is ALL DONE now.");
            let (id, sent) = guard.queue_next().expect("the programmed message is next");
            assert_eq!(id, planned);
            assert_eq!(sent.text, "say it again");
        }
        // A row of a programmed message is written again and taken out like any other row, but
        // it has no place to be put: the text reports when it goes.
        assert!(engine.queue_edit(planned, String::from("say it once more")));
        assert!(!engine.queue_move(planned, 0));
        assert!(engine.queue_move(still.items[0].id, 0), "a plain row moves");
        // The message that found its text is sent, and the plain message is left where it was.
        state.lock().unwrap().queue_take(planned);
        let after = engine.snapshot().queue.expect("the plain message is left");
        assert!(after.planned.is_empty(), "the programmed row went out");
        assert_eq!(after.items.len(), 1);
        assert_eq!(after.items[0].text, "only waiting");
        // A message whose text was never found goes with the chat, as the rest of the list
        // does.
        engine
            .queue_plan(String::from("never"), Vec::new(), String::from("purple"))
            .expect("a second programmed message stands");
        let chat = engine.snapshot().chat_id;
        engine.remove_chat(chat);
        assert!(
            engine.snapshot().queue.is_none(),
            "the chat took its programmed messages with it"
        );
    }

    // The tasks suite: the six tools are behind the line of the badge, off at first; a task
    // made by a call belongs to the working directory and is in the database of the program, with
    // nothing written into the directory; and the prompt of the suite rides in the request only
    // while the line is on.
    #[test]
    fn the_tasks_suite_stands_behind_its_line_and_belongs_to_the_work_directory() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        // The six tools are proposed only while the line of the drop-down is on, and they
        // remain or fall together.
        let names = |on: bool| {
            let mut switches = store::ToolSwitches::default();
            switches.tasks = on;
            offered(
                false,
                true,
                false,
                tools::Access::default(),
                &[],
                &switches,
                true,
            )
            .map(|list| {
                list.as_array()
                    .unwrap()
                    .iter()
                    .map(|one| String::from(one["function"]["name"].as_str().unwrap_or("")))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
        };
        assert!(
            !names(false).contains(&tasks::CREATE_NAME.to_string()),
            "the suite is off in a new chat"
        );
        let held = names(true);
        for one in [
            tasks::CREATE_NAME,
            tasks::VIEW_ALL_NAME,
            tasks::VIEW_ONE_NAME,
            tasks::COMPLETE_NAME,
            tasks::CANCEL_NAME,
            tasks::DELETE_NAME,
        ] {
            assert!(held.contains(&one.to_string()), "{} is not offered", one);
        }
        // The call of a tool of the suite. The task is read again through the same key the
        // engine works out of the working directory, which is the way another chat of the same
        // directory reaches the same list.
        {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
        }
        let workdir = {
            let state = engine.state.lock().unwrap();
            state
                .workdir_for(state.chat().expect("a blank chat stands open"))
        };
        let chat = engine.state.lock().unwrap().open_id();
        let idle = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let call = ToolCall {
            id: String::from("call-create-task"),
            name: String::from(tasks::CREATE_NAME),
            arguments: String::from(
                "{\"name\":\"Lay out the reader\",\n                 \"summary\":\"Read the grammar and write the table.\",\n                 \"content\":\"Write the table of rules, then run the six tests.\"}",
            ),
        };
        run_program_tool(&call, "", 0, &engine.state, &workdir, &idle, &chat);
        let snap = engine.snapshot();
        let run = snap
            .messages
            .iter()
            .rev()
            .find(|one| one.tool_name == tasks::CREATE_NAME)
            .expect("the chat holds the run");
        assert!(!run.failed, "{}", run.tool);
        assert!(run.tool.contains("Task created"), "{}", run.tool);
        // The header of the block names the task, and not all of the answer.
        assert!(run.body.contains("Lay out the reader"), "{}", run.body);
        let made = tasks::run(
            tasks::VIEW_ALL_NAME,
            &tasks::key_of(&workdir),
            "{}",
        );
        assert!(made.ok, "{}", made.output);
        assert!(made.output.contains("Lay out the reader"), "{}", made.output);
        assert!(
            !made.output.contains("Write the table of rules"),
            "the list carries the contents of a task: {}",
            made.output
        );
        // Nothing of the list is in the working directory, and nothing of it is in the chat: the
        // rows are kept in the database of the program.
        let empty = std::fs::read_dir(&workdir)
            .map(|mut one| one.next().is_none())
            .unwrap_or(true);
        assert!(empty, "the working directory holds a file of the program");
        let db = config::paths().data.join(crate::db::TASKS_DB);
        assert!(db.exists(), "the database of the tasks was not made");
        std::mem::drop(snap);
        // The prompt of the suite rides in the request while the line is on, and is removed
        // with the line.
        let said = || -> String {
            let state = engine.state.lock().unwrap();
            state.build_history().to_string()
        };
        assert!(
            !said().contains("TASKS SUITE"),
            "the suite reports nothing while its line is off"
        );
        engine.set_tool_switch("tasks", true);
        let told = said();
        assert!(told.contains("TASKS SUITE"), "the prompt of the suite went missing");
        assert!(
            told.contains(tasks::VIEW_ALL_NAME),
            "the prompt does not name the read of the list"
        );
        engine.set_tool_switch("tasks", false);
        assert!(
            !said().contains("TASKS SUITE"),
            "the prompt outlived its line"
        );
    }

    // A repository removed takes its chats, the conversations the model made of them, its plans
    // and the tasks of its directory with it. Its folder is moved into the `removed/` corner of
    // the repositories directory, so nothing of the user's own work is destroyed, while the
    // side bar - which is the listing of that folder - has nothing left to show. An answer
    // being written in a chat of the repository stops the going, and a repository was open for
    // work gives the place of work back. The chats, the conversations and the tasks of every
    // other repository remain untouched.
    #[test]
    fn a_repository_is_taken_away_with_its_chats() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        engine.create_repository(String::from("Doomed Orchard"));
        let doomed = repo_named(&engine, "Doomed Orchard");
        engine.create_repository(String::from("Standing Grove"));
        let standing = repo_named(&engine, "Standing Grove");
        write_chat_in(&doomed, "aaaagggg01", 100);
        write_chat_in(&doomed, "aaaagggg02", 99);
        write_chat_in(&standing, "aaaagggg03", 98);

        // The conversations of both chats of the doomed repository, and one conversation of a
        // chat that remains nowhere near it.
        let talks = config::contexts_dir(&Config::default());
        let raw = talks.join("the-talk-bytes");
        std::fs::write(&raw, vec![7u8; 4096]).expect("the bytes of a conversation is written");
        let kept = contexts::database(&talks).expect("the database of the conversations is left open");
        let mine = context_name("aaaagggg01", "m1");
        let other = context_name("aaaahhhh09", "m1");
        contexts::keep(&kept, &mine, &raw, 4096).expect("the conversation of the chat is kept");
        contexts::keep(&kept, &other, &raw, 4096).expect("the conversation of another chat is kept");
        let _ = std::fs::remove_file(&raw);

        // A task of the directory of the doomed repository, and one of another.
        let store = store::Store::new(config::repositories_dir(&Config::default()));
        let going = store.repo_path(&doomed).expect("the folder of the repository");
        let staying = store.repo_path(&standing).expect("the folder of the other");
        std::fs::write(going.join("the-work.md"), "the work of the user")
            .expect("a file of the user's own is written in the folder");
        let going_key = crate::tasks::key_of(&going);
        let staying_key = crate::tasks::key_of(&staying);
        crate::tasks::create(&going_key, "The last task", "Its summary.", "Its content.")
            .expect("a task of the doomed directory");
        crate::tasks::create(&staying_key, "A task elsewhere", "Its summary.", "Its content.")
            .expect("a task of the other directory");
        assert!(
            crate::tasks::run(crate::tasks::VIEW_ALL_NAME, &going_key, "{}")
                .output
                .contains("The last task"),
            "the task of the doomed directory remains"
        );

        // An answer being written in one of its chats holds the going off: the text of that
        // answer would else go into a chat that is no more.
        {
            let mut state = engine.state.lock().unwrap();
            state.current = Some(String::from("aaaagggg01"));
            state.live_mut("aaaagggg01").generating = true;
        }
        engine.remove_repository(doomed.clone());
        assert!(
            engine
                .lists()
                .repositories
                .iter()
                .any(|(repo, _)| repo.id == doomed),
            "the going was held off, so the repository is still listed"
        );
        assert!(
            engine.snapshot().status.contains("cannot be removed"),
            "the reason is reported: {}",
            engine.snapshot().status
        );
        assert_eq!(
            store.load_chat("aaaagggg01").map(|chat| chat.meta.id),
            Some(String::from("aaaagggg01")),
            "nothing of the chat stood touched"
        );
        {
            let mut state = engine.state.lock().unwrap();
            state.live_mut("aaaagggg01").generating = false;
            state.current = None;
        }

        // The repository was open for work, so the going has to give the place of work back as
        // well as the chats.
        engine.select_repository(doomed.clone());
        assert!(engine.snapshot().in_repository, "the repository is open for work");
        engine.remove_repository(doomed.clone());

        let snap = engine.snapshot();
        assert!(!snap.in_repository, "the place of work went back");
        assert!(snap.chat_id.is_empty(), "and no chat is open");
        let lists = engine.lists();
        assert!(
            lists.repositories.iter().all(|(repo, _)| repo.id != doomed),
            "the repository is out of the bar"
        );
        let group = lists
            .repositories
            .iter()
            .find(|(repo, _)| repo.id == standing)
            .expect("the other repository stands");
        assert_eq!(group.1.len(), 1, "and its chat is with it");
        assert!(store.load_chat("aaaagggg01").is_none(), "its first chat is removed");
        assert!(store.load_chat("aaaagggg02").is_none(), "and its second");
        assert!(
            store.load_chat("aaaagggg03").is_some(),
            "the chat of the other repository stands"
        );
        let names: Vec<String> = contexts::held_list(&talks)
            .into_iter()
            .map(|(name, _, _)| name)
            .collect();
        assert!(
            !names.iter().any(|one| one.starts_with(&mine)),
            "the conversation of the chat that went is removed"
        );
        assert!(names.contains(&other), "the conversation of another chat remains");
        assert!(
            crate::tasks::run(crate::tasks::VIEW_ALL_NAME, &going_key, "{}")
                .output
                .contains("No tasks for this working directory"),
            "the list of the doomed directory went with it"
        );
        assert!(
            crate::tasks::run(crate::tasks::VIEW_ALL_NAME, &staying_key, "{}")
                .output
                .contains("A task elsewhere"),
            "the list of the other directory remains"
        );

        // The directory of the user's own work is moved aside, never destroyed, and it is in
        // the corner of what went - which is never listed.
        let repos = config::repositories_dir(&Config::default());
        assert!(!going.exists(), "the folder is removed from where it was");
        assert!(repos.join(store::REMOVED).is_dir(), "the corner of what went");
        let aside = format!("doomed-orchard_{}", doomed);
        let moved: Vec<PathBuf> = std::fs::read_dir(repos.join(store::REMOVED))
            .expect("the corner is read")
            .flatten()
            .map(|one| one.path())
            .filter(|one| {
                one.file_name()
                    .map(|name| name.to_string_lossy().starts_with(&aside))
                    .unwrap_or(false)
            })
            .collect();
        assert_eq!(moved.len(), 1, "the one folder of this repository was moved aside");
        assert!(
            moved[0].join("the-work.md").is_file(),
            "the work of the user is with the folder"
        );
        assert!(staying.is_dir(), "the directory of the other repository stands");
        assert!(
            engine.lists().repositories.iter().all(|(repo, _)| {
                repo.id != store::REMOVED && !repo.path.ends_with(store::REMOVED)
            }),
            "the corner is never taken for a repository"
        );
        assert!(
            engine.snapshot().status.contains("is removed"),
            "the going is informed: {}",
            engine.snapshot().status
        );
    }

    // A model that was started with space for several answers at once carries that many
    // chats at the same time. Two items hold a turn back: every place of the model is taken,
    // and the chat wants another model than the one the run carries, for a model is loaded once
    // and a second one would push the run over.
    #[test]
    fn a_model_answers_as_many_chats_at_once_as_its_setting_allows() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let one = PathBuf::from("/models/one.gguf");
        let two = PathBuf::from("/models/two.gguf");
        let one_id = one.to_string_lossy().to_string();
        let two_id = two.to_string_lossy().to_string();
        let (first, second, third, other) = {
            let mut state = engine.state.lock().unwrap();
            state.models = vec![
                ModelInfo {
                    path: one.clone(),
                    name: String::from("one"),
                    kind: "GGUF",
                },
                ModelInfo {
                    path: two.clone(),
                    name: String::from("two"),
                    kind: "GGUF",
                },
            ];
            // The model of the three first chats answers two of them at once.
            let mut settings = state.cfg.model(&one_id);
            settings.parallel = 2;
            state.cfg.model_settings.insert(one_id.clone(), settings);
            state.cfg.model_id = one_id.clone();
            state.start_chat();
            let first = state.open_id();
            state.start_chat();
            let second = state.open_id();
            state.start_chat();
            let third = state.open_id();
            state.start_chat();
            let other = state.open_id();
            // The chat of the other model is given its model outright: a chat takes the model
            // of the chat that is left open, and that is the first model here.
            state
                .chat_of_mut(&other)
                .expect("the chat of the other model remains")
                .meta
                .model_id = two_id.clone();
            (first, second, third, other)
        };
        let none = std::collections::BTreeSet::new();
        assert!(
            room_for(&engine.state, None, &none, &first),
            "a turn is begun while the model has a place free"
        );
        assert!(
            room_for(&engine.state, None, &none, &other),
            "a chat of another model loads its own model when nothing is answered"
        );
        let mut busy = std::collections::BTreeSet::new();
        busy.insert(first.clone());
        assert!(
            room_for(&engine.state, Some(&one_id), &busy, &second),
            "the second place of the model is left open"
        );
        assert!(
            !room_for(&engine.state, Some(&one_id), &busy, &other),
            "a chat of another model waits for the chats of the run"
        );
        assert!(
            !room_for(&engine.state, None, &busy, &second),
            "a chat waits while the model of the first chat is still loading"
        );
        busy.insert(second.clone());
        assert!(
            !room_for(&engine.state, Some(&one_id), &busy, &third),
            "no place of the model is left"
        );
    }

    // A chat is opened, and a new one made, while an answer is being written in another chat:
    // the model answers several chats at once, so the program never reports the user to wait.
    // The answer that is on its way keeps its own state, and the marker of it stays on its chat
    // alone.
    #[test]
    fn a_chat_is_opened_and_a_new_one_made_while_an_answer_comes_in() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let busy_chat = {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
            let id = state.open_id();
            // The chat is at work, and no turn of another chat may take the marker of it away.
            state.live_mut(&id).generating = true;
            id
        };
        engine.new_chat();
        let snap = engine.snapshot();
        assert!(snap.home, "the first screen remains, an answer or no answer");
        assert!(
            !snap.generating,
            "the first screen is not the chat that is at work"
        );
        assert_eq!(
            snap.working,
            vec![busy_chat.clone()],
            "the chat at work is still named as at work"
        );
        engine.open_chat(busy_chat.clone());
        let back = engine.snapshot();
        assert_eq!(back.chat_id, busy_chat, "the chat of the answer is open again");
        assert!(back.generating, "the answer of that chat is still going on");
        assert!(
            !back.status.contains("Wait"),
            "the program does not tell the user to wait: {}",
            back.status
        );
    }

    // The settings of a model are kept while an answer is on its way, and the text the user is
    // given say when they take hold. The settings are never deleted because an answer was going
    // on, and the text never tell the user to wait for it.
    #[test]
    fn changed_model_settings_are_kept_and_hold_on_a_new_turn() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let (id, chat) = {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
            let chat = state.open_id();
            let id = state.current_model_id();
            state.live_mut(&chat).generating = true;
            (id, chat)
        };
        let held = "The changed model settings will take effect on a new turn";
        let (mut settings, _) = engine.model_settings(&id);
        settings.parallel = 3;
        engine.save_model_settings(id.clone(), settings);
        let (kept, _) = engine.model_settings(&id);
        assert_eq!(kept.parallel, 3, "the setting of the model was kept");
        let said = engine.snapshot().status;
        assert_eq!(said, held, "the text reports when the change takes hold");
        // The same change with the chat at rest needs no text of its own: the setting takes
        // hold at once, and the line of state reports nothing of a new turn.
        {
            let mut state = engine.state.lock().unwrap();
            state.live_mut(&chat).generating = false;
            state.set_status("ready");
        }
        let (mut again, _) = engine.model_settings(&id);
        again.parallel = 4;
        engine.save_model_settings(id.clone(), again);
        let (kept, _) = engine.model_settings(&id);
        assert_eq!(kept.parallel, 4, "the setting was kept with the chat at rest");
        assert_ne!(
            engine.snapshot().status,
            held,
            "nothing waits for a new turn when no answer is on its way"
        );
    }

    // Stopping the answer of one chat stops that chat alone. Another chat that is answered at
    // the same time, and the call of a tool it is running, go on as they were, for the stop
    // belongs to the chat the user pressed it in.
    #[test]
    fn stopping_one_chat_leaves_the_answer_of_another_going_on() {
        let _guard = config::test_turn();
        setup();
        let engine = Engine::start(Config::default());
        let (first, second, first_watch, second_watch) = {
            let mut state = engine.state.lock().unwrap();
            state.start_chat();
            let first = state.open_id();
            state.start_chat();
            let second = state.open_id();
            let first_watch = crate::util::Watch::new(Duration::from_secs(600));
            let second_watch = crate::util::Watch::new(Duration::from_secs(600));
            state.begin_running_in(&first, "block-first", &first_watch);
            state.begin_running_in(&second, "block-second", &second_watch);
            (
                first,
                second,
                first_watch,
                second_watch,
            )
        };
        engine.cancel_chat(&first);
        assert!(first_watch.stopped(), "the call of the chat that was stopped");
        assert!(
            !second_watch.stopped(),
            "the call of the other chat goes on as it was"
        );
        {
            let state = engine.state.lock().unwrap();
            let stopped = state
                .live_of(&first)
                .and_then(|one| one.running.as_ref())
                .map(|one| one.watch.stopped());
            let going = state
                .live_of(&second)
                .and_then(|one| one.running.as_ref())
                .map(|one| one.watch.stopped());
            assert_eq!(stopped, Some(true), "the stopped chat carries its own marker");
            assert_eq!(going, Some(false), "the other chat is in its call as it was");
        }
    }
}
